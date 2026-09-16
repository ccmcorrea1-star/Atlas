//! External editor bridge for the Codex-style composer.
//!
//! The editor is deliberately outside the runtime adapter: it edits only the
//! local draft and returns text to the TUI. Provider/session state is untouched.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::process::Command;

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum EditorError {
    MissingEditor,
    EmptyCommand,
    InvalidCommand,
    Io(String),
    Failed(String),
}

impl std::fmt::Display for EditorError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingEditor => formatter.write_str("neither VISUAL nor EDITOR is set"),
            Self::EmptyCommand => formatter.write_str("editor command is empty"),
            Self::InvalidCommand => formatter.write_str("failed to parse editor command"),
            Self::Io(error) => write!(formatter, "editor file error: {error}"),
            Self::Failed(status) => write!(formatter, "editor exited with status {status}"),
        }
    }
}

impl std::error::Error for EditorError {}

pub(crate) fn resolve_editor_command() -> Result<Vec<String>, EditorError> {
    let raw = env::var("VISUAL")
        .or_else(|_| env::var("EDITOR"))
        .map_err(|_| EditorError::MissingEditor)?;
    let command = split_command(&raw)?;
    if command.is_empty() {
        return Err(EditorError::EmptyCommand);
    }
    Ok(command)
}

pub(crate) async fn run_editor(
    seed: &str,
    editor_command: &[String],
) -> Result<String, EditorError> {
    if editor_command.is_empty() {
        return Err(EditorError::EmptyCommand);
    }
    let path = temporary_editor_path();
    fs::write(&path, seed).map_err(|error| EditorError::Io(error.to_string()))?;

    let mut command = Command::new(&editor_command[0]);
    command
        .args(&editor_command[1..])
        .arg(&path)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    let status = command
        .status()
        .await
        .map_err(|error| EditorError::Io(error.to_string()))?;
    let result = if status.success() {
        fs::read_to_string(&path).map_err(|error| EditorError::Io(error.to_string()))
    } else {
        Err(EditorError::Failed(status.to_string()))
    };
    let _ = fs::remove_file(&path);
    result
}

fn temporary_editor_path() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    env::temp_dir().join(format!("atlas-tui-{}-{nonce}.md", std::process::id()))
}

fn split_command(raw: &str) -> Result<Vec<String>, EditorError> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    for character in raw.chars() {
        if escaped {
            current.push(character);
            escaped = false;
        } else if character == '\\' && quote != Some('\'') {
            escaped = true;
        } else if let Some(active_quote) = quote {
            if character == active_quote {
                quote = None;
            } else {
                current.push(character);
            }
        } else if character == '\'' || character == '"' {
            quote = Some(character);
        } else if character.is_whitespace() {
            if !current.is_empty() {
                parts.push(std::mem::take(&mut current));
            }
        } else {
            current.push(character);
        }
    }
    if escaped || quote.is_some() {
        return Err(EditorError::InvalidCommand);
    }
    if !current.is_empty() {
        parts.push(current);
    }
    Ok(parts)
}

#[cfg(test)]
mod tests {
    use super::{EditorError, split_command};

    #[test]
    fn splits_editor_arguments_and_quotes() {
        assert_eq!(
            split_command("code --wait 'file name'").unwrap(),
            vec!["code", "--wait", "file name"]
        );
    }

    #[test]
    fn rejects_unclosed_quotes_and_trailing_escape() {
        assert_eq!(
            split_command("code 'file"),
            Err(EditorError::InvalidCommand)
        );
        assert_eq!(split_command("code \\"), Err(EditorError::InvalidCommand));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn run_editor_returns_modified_draft() {
        use std::os::unix::fs::PermissionsExt;

        let script = std::env::temp_dir().join(format!(
            "atlas-editor-test-{}-{}.sh",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&script, "#!/bin/sh\nprintf edited > \"$1\"\n").unwrap();
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&script, permissions).unwrap();

        let command = vec![script.to_string_lossy().into_owned()];
        let edited = super::run_editor("seed", &command).await.unwrap();
        assert_eq!(edited, "edited");
        let _ = std::fs::remove_file(script);
    }
}
