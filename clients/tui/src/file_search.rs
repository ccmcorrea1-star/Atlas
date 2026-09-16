//! Bounded local file search used by the composer `@` picker.

use std::path::{Path, PathBuf};

const MAX_RESULTS: usize = 24;
const MAX_DEPTH: usize = 3;
const MAX_VISITED: usize = 2_000;

pub(crate) fn search(root: &Path, query: &str) -> Vec<PathBuf> {
    let normalized = query.to_lowercase();
    let mut results = Vec::new();
    let mut visited = 0usize;
    visit(root, root, &normalized, 0, &mut visited, &mut results);
    results.sort_by(|left, right| left.to_string_lossy().cmp(&right.to_string_lossy()));
    results.truncate(MAX_RESULTS);
    results
}

fn visit(
    root: &Path,
    directory: &Path,
    query: &str,
    depth: usize,
    visited: &mut usize,
    results: &mut Vec<PathBuf>,
) {
    if depth > MAX_DEPTH || *visited >= MAX_VISITED || results.len() >= MAX_RESULTS {
        return;
    }
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        if *visited >= MAX_VISITED || results.len() >= MAX_RESULTS {
            break;
        }
        *visited += 1;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            visit(root, &path, query, depth + 1, visited, results);
        } else if file_type.is_file() {
            let relative = path.strip_prefix(root).unwrap_or(&path);
            let display = relative.to_string_lossy();
            if query.is_empty() || display.to_lowercase().contains(query) {
                results.push(relative.to_path_buf());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::search;

    #[test]
    fn finds_matching_files_with_a_bounded_relative_path() {
        let root = std::env::temp_dir().join(format!("atlas-file-search-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src/nested")).unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();
        std::fs::write(root.join("src/nested/lib.rs"), "pub fn lib() {}").unwrap();
        std::fs::write(root.join("README.md"), "readme").unwrap();

        let results = search(&root, "lib");
        assert_eq!(results, vec![std::path::PathBuf::from("src/nested/lib.rs")]);
        let _ = std::fs::remove_dir_all(root);
    }
}
