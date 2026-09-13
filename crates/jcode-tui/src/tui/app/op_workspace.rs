//! Shared access to a workspace's op change directory.
//!
//! Both derived side-panel pages (the progress page and the requirement map)
//! read the same `.op/changes/<newest>/` directory, so the "which change is
//! this about" rule and the file-reading policy live here once instead of being
//! duplicated — and so both pages always describe the same change.

use std::path::{Path, PathBuf};

/// Read a file that may legitimately be absent.
///
/// `NotFound` is a normal outcome (optional companions like `tasks.md`). Any
/// other error is logged rather than dropped on the floor: a derived page
/// silently losing its source is exactly what the swallowed-error ratchet is
/// there to prevent.
pub(super) fn read_text_optional(path: &Path) -> Option<String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            crate::logging::info(&format!("cannot read {}: {error}", path.display()));
            None
        }
    }
}

/// The newest `<working_dir>/.op/changes/<name>/` that has a `change.yaml`.
///
/// Recency is the best available stand-in for "the change being worked on": the
/// active change is the one whose files are being edited. A directory without
/// `change.yaml` is not a change and is skipped.
pub(super) fn newest_change_dir(working_dir: &Path) -> Option<PathBuf> {
    let changes_dir = working_dir.join(".op").join("changes");
    let Ok(entries) = std::fs::read_dir(changes_dir) else {
        return None;
    };

    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        let change_yaml = path.join("change.yaml");
        if !path.is_dir() || !change_yaml.is_file() {
            continue;
        }
        let modified = std::fs::metadata(&change_yaml)
            .and_then(|meta| meta.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        let is_newer = match best.as_ref() {
            Some((best_time, _)) => modified >= *best_time,
            None => true,
        };
        if is_newer {
            best = Some((modified, path));
        }
    }
    best.map(|(_, path)| path)
}

/// The change directory to report on for a working directory, if any.
pub(super) fn active_change_dir(working_dir: Option<&Path>) -> Option<PathBuf> {
    newest_change_dir(working_dir?)
}

#[cfg(test)]
#[path = "op_workspace_tests.rs"]
mod tests;
