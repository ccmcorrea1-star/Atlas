//! Metadata de blocos Markdown coletada em uma única passagem do parser.
//!
//! Este módulo adapta somente a parte genérica do streaming Codex. Não conhece
//! RuntimeEvent nem tipos do core Codex; o chamador decide como materializar o
//! prefixo estável e o tail mutável no transcript Atlas.

use std::ops::Range;

use pulldown_cmark::CodeBlockKind;
use pulldown_cmark::Event;
use pulldown_cmark::Options;
use pulldown_cmark::Parser;
use pulldown_cmark::Tag;

#[derive(Debug, Default, Eq, PartialEq)]
pub(crate) struct StreamingMarkdownMetadata {
    /// Início em bytes do último bloco top-level, quando existe um bloco anterior.
    pub(crate) last_top_level_block_start: Option<usize>,
    /// A fonte contém uma definição de referência que pode alterar blocos anteriores.
    pub(crate) has_reference_link_definition: bool,
    /// O primeiro bloco é HTML bruto e não deve receber separador artificial.
    pub(crate) first_top_level_block_is_html: bool,
    /// Início do último bloco Mermaid, mantido mutável até o fechamento.
    pub(crate) mermaid_start: Option<usize>,
}

pub(crate) fn scan(input: &str) -> StreamingMarkdownMetadata {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    let parser = Parser::new_ext(input, options);
    let has_reference_link_definition = parser.reference_definitions().iter().next().is_some();
    let mut tracker = TopLevelBlockTracker {
        depth: 0,
        block_count: 0,
        last_start: 0,
        first_is_html: false,
        mermaid_start: None,
    };
    for (event, range) in parser.into_offset_iter() {
        tracker.observe(&event, range);
    }

    StreamingMarkdownMetadata {
        last_top_level_block_start: (tracker.block_count > 1).then_some(tracker.last_start),
        has_reference_link_definition,
        first_top_level_block_is_html: tracker.first_is_html,
        mermaid_start: tracker.mermaid_start,
    }
}

#[derive(Debug, Default)]
struct TopLevelBlockTracker {
    depth: usize,
    block_count: usize,
    last_start: usize,
    first_is_html: bool,
    mermaid_start: Option<usize>,
}

impl TopLevelBlockTracker {
    fn observe(&mut self, event: &Event<'_>, range: Range<usize>) {
        if self.depth == 0 && matches!(event, Event::Start(_) | Event::Rule | Event::Html(_)) {
            self.mermaid_start = None;
            self.block_count += 1;
            self.last_start = range.start;
            if self.block_count == 1 {
                self.first_is_html = matches!(event, Event::Start(Tag::HtmlBlock) | Event::Html(_));
            }
        }
        if let Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) = event
            && info
                .split([',', ' ', '\t'])
                .next()
                .is_some_and(|language| language.eq_ignore_ascii_case("mermaid"))
        {
            self.mermaid_start.get_or_insert(self.last_start);
        }
        match event {
            Event::Start(_) => self.depth += 1,
            Event::End(_) => self.depth = self.depth.saturating_sub(1),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_last_top_level_block_mutable() {
        let metadata = scan("first\n\nsecond\n");
        assert_eq!(metadata.last_top_level_block_start, Some(7));
    }

    #[test]
    fn tracks_mermaid_and_reference_definitions() {
        let metadata = scan("[ref]: https://example.test\n\n```mermaid\ngraph TD\n```\n");
        assert!(metadata.has_reference_link_definition);
        assert_eq!(metadata.mermaid_start, Some(29));
    }

    #[test]
    fn does_not_split_nested_list_blocks_into_top_level_blocks() {
        let metadata = scan("- first\n  - nested\n");
        assert_eq!(metadata.last_top_level_block_start, None);
    }
}
