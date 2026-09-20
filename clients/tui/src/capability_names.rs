//! Nomes curtos de apresentacao para capabilities do Runtime.
//!
//! O transcript e a linha de atividade usam rotulos humanos no lugar do id
//! tecnico da capability, sem alterar o protocolo publico.

/// Rotulo curto exibido nas cells de tool, como "Read" ou "Search".
pub(crate) fn capability_label(capability_id: &str) -> String {
    match capability_id {
        "filesystem.read" => "Read".to_owned(),
        "filesystem.list" => "List".to_owned(),
        "filesystem.search" => "Search".to_owned(),
        "filesystem.glob" => "Find".to_owned(),
        "filesystem.patch" => "Patch".to_owned(),
        "web.search" => "Search".to_owned(),
        "web.fetch" => "Fetch".to_owned(),
        "web.browser" => "Browser".to_owned(),
        "web.crawl" => "Crawl".to_owned(),
        "shell.exec" => "Run".to_owned(),
        "system.info" => "System info".to_owned(),
        "lsp.diagnostics" => "Diagnostics".to_owned(),
        "git.status" => "Git status".to_owned(),
        "git.diff" => "Git diff".to_owned(),
        other => humanize_capability_id(other),
    }
}

/// Frase de atividade no gerundio, como "Reading" ou "Applying patch".
pub(crate) fn capability_activity_with_target(capability_id: &str, target: Option<&str>) -> String {
    let activity = match capability_id {
        "filesystem.read" => "Reading".to_owned(),
        "filesystem.list" => "Listing".to_owned(),
        "filesystem.search" => "Searching".to_owned(),
        "filesystem.glob" => "Finding".to_owned(),
        "filesystem.patch" => "Applying patch".to_owned(),
        "web.search" => "Searching".to_owned(),
        "web.fetch" => "Fetching".to_owned(),
        "web.browser" => "Browsing".to_owned(),
        "web.crawl" => "Crawling".to_owned(),
        "shell.exec" => "Running".to_owned(),
        "system.info" => "Reading system info".to_owned(),
        "lsp.diagnostics" => "Checking diagnostics".to_owned(),
        "git.status" => "Checking git status".to_owned(),
        "git.diff" => "Reading diff".to_owned(),
        other => format!("Using {}", humanize_capability_id(other)),
    };
    target
        .filter(|value| !value.trim().is_empty())
        .map_or(activity.clone(), |value| format!("{activity} {value}"))
}

/// Atividade de uma execucao de shell, com o comando interpretado.
pub(crate) fn execution_activity(program: &str, args: &[String]) -> String {
    let command = crate::markdown::format_process_command(program, args);
    if command.trim().is_empty() {
        "Running".to_owned()
    } else {
        format!("Running {command}")
    }
}

// Remove o prefixo de grupo e troca separadores por espacos, preservando o nome
// tecnico quando nao houver um segmento final melhor.
fn humanize_capability_id(capability_id: &str) -> String {
    let tail = capability_id.rsplit('.').next().unwrap_or(capability_id);
    let tail = if tail.is_empty() { capability_id } else { tail };
    let mut words = tail.replace(['_', '-'], " ");
    if let Some(first) = words.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_known_capabilities_to_short_labels() {
        assert_eq!(capability_label("filesystem.read"), "Read");
        assert_eq!(capability_label("filesystem.search"), "Search");
        assert_eq!(capability_label("filesystem.glob"), "Find");
        assert_eq!(capability_label("filesystem.patch"), "Patch");
    }

    #[test]
    fn maps_known_capabilities_to_activity_phrases() {
        assert_eq!(
            capability_activity_with_target("filesystem.read", None),
            "Reading"
        );
        assert_eq!(
            capability_activity_with_target("filesystem.search", None),
            "Searching"
        );
        assert_eq!(
            capability_activity_with_target("filesystem.patch", None),
            "Applying patch"
        );
        assert_eq!(
            capability_activity_with_target("filesystem.read", Some("tsconfig.json")),
            "Reading tsconfig.json"
        );
    }

    #[test]
    fn humanizes_unknown_capabilities_instead_of_leaking_the_raw_id() {
        assert_eq!(capability_label("sandbox.run"), "Run");
        assert_eq!(capability_label("custom_tool"), "Custom tool");
        assert_eq!(
            capability_activity_with_target("sandbox.run", None),
            "Using Run"
        );
    }

    #[test]
    fn derives_execution_activity_from_the_interpreted_command() {
        let args = vec!["-c".to_owned(), "npm test".to_owned()];
        assert_eq!(execution_activity("sh", &args), "Running npm test");
        assert_eq!(
            execution_activity("node", &["--version".to_owned()]),
            "Running node --version"
        );
    }
}
