//! Hidden-session index under `~/.jcode/hidden_sessions`.
//!
//! One marker file per session the user removed from the session board
//! (Ctrl+X twice). The index is user intent, not lifecycle state: it survives
//! restarts and is the authority that keeps a removed row from reappearing
//! when presence snapshots or finish events refresh.
//!
//! Removing from the board never deletes the transcript; restoring a session
//! is removing its marker file (see the board's hidden filter view).

use crate::jcode_dir;
use std::path::PathBuf;

/// Directory holding one marker file per hidden session.
pub fn hidden_sessions_dir() -> Option<PathBuf> {
    jcode_dir().ok().map(|d| d.join("hidden_sessions"))
}

/// Hide `session_id` from the session board.
pub fn hide_session(session_id: &str) {
    if let Some(dir) = hidden_sessions_dir() {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join(session_id), "");
    }
}

/// Restore a hidden session to the board.
pub fn unhide_session(session_id: &str) {
    if let Some(dir) = hidden_sessions_dir() {
        let _ = std::fs::remove_file(dir.join(session_id));
    }
}

/// Whether `session_id` is currently hidden from the board.
pub fn session_is_hidden(session_id: &str) -> bool {
    hidden_sessions_dir().is_some_and(|dir| dir.join(session_id).exists())
}

/// Snapshot of all hidden session ids.
pub fn hidden_session_ids() -> Vec<String> {
    let Some(dir) = hidden_sessions_dir() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialize tests that mutate `JCODE_HOME` (shared across the crate so
    /// parallel modules cannot clobber each other's sandbox).
    fn lock_env() -> std::sync::MutexGuard<'static, ()> {
        crate::test_env_lock::lock_test_env()
    }

    #[test]
    fn hide_unhide_roundtrip() {
        let _guard = lock_env();
        let temp = tempfile::tempdir().expect("tempdir");
        jcode_core::env::set_var("JCODE_HOME", temp.path());

        assert!(!session_is_hidden("session_a"));
        hide_session("session_a");
        hide_session("session_b");
        assert!(session_is_hidden("session_a"));

        let mut ids = hidden_session_ids();
        ids.sort();
        assert_eq!(ids, vec!["session_a", "session_b"]);

        unhide_session("session_a");
        assert!(!session_is_hidden("session_a"));
        assert!(session_is_hidden("session_b"));
        // Unhiding a non-hidden session is a no-op.
        unhide_session("session_never_hidden");

        jcode_core::env::remove_var("JCODE_HOME");
    }
}
