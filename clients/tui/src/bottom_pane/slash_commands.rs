//! Small command registry used by the composer popup.
//!
//! Only commands implemented by the Atlas TUI belong here. Provider/session
//! commands stay in the runtime boundary instead of appearing as dead UI.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SlashCommand {
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
}

pub(crate) const COMMANDS: &[SlashCommand] = &[
    SlashCommand {
        name: "/help",
        description: "show keyboard shortcuts",
    },
    SlashCommand {
        name: "/quit",
        description: "close the Atlas TUI",
    },
];

pub(crate) fn matching(prefix: &str) -> impl Iterator<Item = SlashCommand> + '_ {
    COMMANDS
        .iter()
        .copied()
        .filter(move |command| command.name.starts_with(prefix))
}

pub(crate) fn exact(input: &str) -> Option<SlashCommand> {
    COMMANDS
        .iter()
        .copied()
        .find(|command| command.name == input)
}

#[cfg(test)]
mod tests {
    use super::{exact, matching};

    #[test]
    fn filters_commands_by_typed_prefix() {
        let names = matching("/he")
            .map(|command| command.name)
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["/help"]);
    }

    #[test]
    fn resolves_only_registered_commands() {
        assert_eq!(exact("/quit").unwrap().name, "/quit");
        assert!(exact("/model").is_none());
    }
}
