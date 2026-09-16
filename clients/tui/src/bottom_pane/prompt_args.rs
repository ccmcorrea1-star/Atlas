//! Parsing shared by the Codex-style slash-command composer.

/// Parse a first-line slash command as `(name, rest, rest_offset)`.
///
/// `rest_offset` points into the original line after the command name and
/// leading whitespace. The parser deliberately does not validate command
/// names; validation belongs to the command registry.
pub(crate) fn parse_slash_name(line: &str) -> Option<(&str, &str, usize)> {
    let stripped = line.strip_prefix('/')?;
    let name_end = stripped
        .char_indices()
        .find_map(|(index, character)| character.is_whitespace().then_some(index))
        .unwrap_or(stripped.len());
    let name = &stripped[..name_end];
    if name.is_empty() {
        return None;
    }
    let rest_untrimmed = &stripped[name_end..];
    let rest = rest_untrimmed.trim_start();
    let rest_offset = name_end + (rest_untrimmed.len() - rest.len()) + 1;
    Some((name, rest, rest_offset))
}

#[cfg(test)]
mod tests {
    use super::parse_slash_name;

    #[test]
    fn parses_name_and_trimmed_arguments_with_original_offset() {
        let line = "/help   now";
        let (name, rest, offset) = parse_slash_name(line).unwrap();
        assert_eq!(name, "help");
        assert_eq!(rest, "now");
        assert_eq!(&line[offset..], "now");
    }

    #[test]
    fn rejects_non_slash_and_empty_commands() {
        assert!(parse_slash_name("help").is_none());
        assert!(parse_slash_name("/").is_none());
    }
}
