use std::collections::HashSet;
use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

const MAX_HISTORY_ENTRIES: usize = 1_000;
const MAX_HISTORY_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone)]
pub(crate) struct HistoryStore {
    path: Option<PathBuf>,
}

#[derive(Debug, Deserialize, Serialize)]
struct HistoryRecord<'a> {
    message: &'a str,
}

#[derive(Debug, Deserialize)]
struct LoadedHistoryRecord {
    message: String,
}

impl HistoryStore {
    #[cfg(not(test))]
    pub(crate) fn for_conversation(conversation_id: &str) -> Self {
        let root = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state"))
            })
            .unwrap_or_else(|| std::env::temp_dir().join("atlas-state"))
            .join("atlas")
            .join("history");
        Self::new(root, conversation_id)
    }

    pub(crate) fn new(root: impl Into<PathBuf>, conversation_id: &str) -> Self {
        let encoded_id = conversation_id
            .as_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        Self {
            path: Some(root.into().join(format!("conversation-{encoded_id}.jsonl"))),
        }
    }

    #[cfg(test)]
    pub(crate) fn disabled() -> Self {
        Self { path: None }
    }

    pub(crate) fn load(&self) -> Vec<String> {
        let Some(path) = self.path.as_deref() else {
            return Vec::new();
        };
        let Ok(contents) = fs::read_to_string(path) else {
            return Vec::new();
        };
        if contents.len() > MAX_HISTORY_BYTES {
            return Vec::new();
        }
        let entries = contents
            .lines()
            .filter_map(|line| serde_json::from_str::<LoadedHistoryRecord>(line).ok())
            .map(|record| record.message)
            .collect::<Vec<_>>();
        Self::bounded_entries(&entries)
    }

    pub(crate) fn bounded_entries(entries: &[String]) -> Vec<String> {
        let mut selected = Vec::new();
        let mut seen = HashSet::new();
        let mut bytes: usize = 0;
        for entry in entries.iter().rev() {
            if selected.len() >= MAX_HISTORY_ENTRIES || !seen.insert(entry) {
                continue;
            }
            let line_bytes = serde_json::to_string(&HistoryRecord { message: entry })
                .map(|record| record.len() + 1)
                .unwrap_or(usize::MAX);
            if line_bytes > MAX_HISTORY_BYTES
                || bytes.saturating_add(line_bytes) > MAX_HISTORY_BYTES
            {
                continue;
            }
            bytes += line_bytes;
            selected.push(entry.clone());
        }
        selected.reverse();
        selected
    }

    pub(crate) fn save(&self, entries: &[String]) -> std::io::Result<()> {
        let Some(path) = self.path.as_deref() else {
            return Ok(());
        };
        let parent = path
            .parent()
            .ok_or_else(|| std::io::Error::other("history path has no parent"))?;
        fs::create_dir_all(parent)?;
        let temporary_path = path.with_extension("jsonl.tmp");
        let mut options = OpenOptions::new();
        options.create(true).truncate(true).write(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut file = options.open(&temporary_path)?;
        for entry in Self::bounded_entries(entries) {
            let record = serde_json::to_string(&HistoryRecord { message: &entry })
                .map_err(std::io::Error::other)?;
            file.write_all(record.as_bytes())?;
            file.write_all(b"\n")?;
        }
        file.sync_all()?;
        drop(file);
        fs::rename(temporary_path, path)?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::HistoryStore;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn deduplicates_entries_and_enforces_entry_and_byte_limits() {
        let root =
            std::env::temp_dir().join(format!("atlas-history-limits-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let store = HistoryStore::new(&root, "limits");
        let mut entries = vec![
            "repeat".to_owned(),
            "middle".to_owned(),
            "repeat".to_owned(),
        ];
        entries.extend((0..1_100).map(|index| format!("entry-{index}")));
        entries.push("x".repeat(2 * 1024 * 1024));
        store.save(&entries).expect("bounded history should save");

        let loaded = store.load();
        assert!(loaded.len() <= 1_000);
        assert!(!loaded.contains(&"repeat".to_owned()));
        assert!(loaded.last().is_some_and(|entry| entry == "entry-1099"));
        assert!(
            std::fs::metadata(store.path().expect("history path should exist"))
                .expect("history file should exist")
                .len()
                <= 2 * 1024 * 1024
        );
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn stores_each_conversation_in_a_private_bounded_jsonl_file() {
        let root = std::env::temp_dir().join(format!("atlas-history-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let store = HistoryStore::new(&root, "conversation/a");
        let entries = vec!["first\nline".to_owned(), "second".to_owned()];
        store.save(&entries).expect("history should save");

        assert_eq!(store.load(), entries);
        let path = store.path().expect("history path should exist");
        assert!(path.ends_with("conversation-636f6e766572736174696f6e2f61.jsonl"));
        #[cfg(unix)]
        assert_eq!(
            std::fs::metadata(path)
                .expect("history file should exist")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );

        let other = HistoryStore::new(&root, "other");
        assert!(other.load().is_empty());
        let _ = std::fs::remove_dir_all(root);
    }
}
