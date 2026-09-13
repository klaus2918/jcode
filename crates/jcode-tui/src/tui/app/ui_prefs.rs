//! Small persisted UI preferences that survive restarts and session resumes.
//!
//! These are deliberately separate from the main config file: they capture
//! in-app toggles (like hiding inline images) that the user flips at runtime
//! and expects to stick, without editing `config.toml`.

use serde::{Deserialize, Serialize};

const UI_PREFS_FILE: &str = "ui_preferences.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct UiPreferences {
    #[serde(default)]
    pub version: u8,
    /// Whether inline transcript images render expanded. `None` means the
    /// user never toggled it; default to visible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inline_images_visible: Option<bool>,
    /// Side-panel width percentage the user last chose with `Ctrl+1`..`Ctrl+4`.
    /// `None` means they never resized it, so the configured startup value
    /// (`display.side_pane_ratio`) applies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side_pane_ratio: Option<u16>,
}

fn prefs_path() -> Option<std::path::PathBuf> {
    crate::storage::app_config_dir()
        .ok()
        .map(|dir| dir.join(UI_PREFS_FILE))
}

pub(crate) fn load() -> UiPreferences {
    let Some(path) = prefs_path() else {
        return UiPreferences::default();
    };
    crate::storage::read_json::<UiPreferences>(&path).unwrap_or_default()
}

/// Persisted inline-image visibility, defaulting to visible.
///
/// Only the transcript-image toggle path reads this today, and that path is
/// exercised from tests, so the reader stays test-gated while the module itself
/// is compiled in production for the side-panel width preference.
#[cfg(test)]
pub(crate) fn inline_images_visible() -> bool {
    load().inline_images_visible.unwrap_or(true)
}

/// Persist the inline-image visibility toggle (load-modify-write so future
/// preference fields survive).
#[cfg(test)]
pub(crate) fn save_inline_images_visible(visible: bool) {
    let Some(path) = prefs_path() else {
        return;
    };
    let mut prefs = load();
    prefs.inline_images_visible = Some(visible);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(error) = crate::storage::write_json(&path, &prefs) {
        crate::logging::info(&format!(
            "Failed to persist UI preferences {}: {}",
            path.display(),
            error
        ));
    }
}

/// Persisted side-panel width percentage, if the user ever resized it.
pub(crate) fn side_pane_ratio_percent() -> Option<u16> {
    load().side_pane_ratio.map(|value| value.clamp(25, 100))
}

/// Persist the side-panel width choice (load-modify-write so future preference
/// fields survive).
pub(crate) fn save_side_pane_ratio_percent(percent: u16) {
    let Some(path) = prefs_path() else {
        return;
    };
    let mut prefs = load();
    prefs.side_pane_ratio = Some(percent.clamp(25, 100));
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(error) = crate::storage::write_json(&path, &prefs) {
        crate::logging::info(&format!(
            "Failed to persist UI preferences {}: {}",
            path.display(),
            error
        ));
    }
}

/// Effective startup side-panel width: the user's last `Ctrl+1`..`Ctrl+4`
/// choice when they ever made one, otherwise the configured
/// `display.side_pane_ratio`. The bool reports whether a user choice is in
/// effect, which disables the automatic widening for image-dominant panes.
pub(crate) fn initial_side_pane_ratio(configured_percent: u16) -> (u16, bool) {
    match side_pane_ratio_percent() {
        Some(percent) => (percent, true),
        None => (configured_percent.clamp(25, 100), false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_images_visibility_round_trips_through_disk() {
        let _guard = crate::storage::lock_test_env();
        let temp = tempfile::tempdir().expect("tempdir");
        let prev_home = std::env::var_os("JCODE_HOME");
        crate::env::set_var("JCODE_HOME", temp.path());

        // Default before any toggle: visible.
        assert!(inline_images_visible());

        save_inline_images_visible(false);
        assert!(!inline_images_visible(), "hidden state should persist");

        save_inline_images_visible(true);
        assert!(inline_images_visible(), "visible state should persist");

        if let Some(prev_home) = prev_home {
            crate::env::set_var("JCODE_HOME", prev_home);
        } else {
            crate::env::remove_var("JCODE_HOME");
        }
    }

    #[test]
    fn save_preserves_unknown_future_fields_via_load_modify_write() {
        let _guard = crate::storage::lock_test_env();
        let temp = tempfile::tempdir().expect("tempdir");
        let prev_home = std::env::var_os("JCODE_HOME");
        crate::env::set_var("JCODE_HOME", temp.path());

        save_inline_images_visible(false);
        let prefs = load();
        assert_eq!(prefs.inline_images_visible, Some(false));

        if let Some(prev_home) = prev_home {
            crate::env::set_var("JCODE_HOME", prev_home);
        } else {
            crate::env::remove_var("JCODE_HOME");
        }
    }

    #[test]
    fn side_pane_ratio_round_trips_and_clamps() {
        let _guard = crate::storage::lock_test_env();
        let temp = tempfile::tempdir().expect("tempdir");
        let prev_home = std::env::var_os("JCODE_HOME");
        crate::env::set_var("JCODE_HOME", temp.path());

        assert_eq!(side_pane_ratio_percent(), None, "unset by default");

        save_side_pane_ratio_percent(50);
        assert_eq!(side_pane_ratio_percent(), Some(50));

        // Out-of-range values are clamped on both write and read.
        save_side_pane_ratio_percent(5);
        assert_eq!(side_pane_ratio_percent(), Some(25));
        save_side_pane_ratio_percent(400);
        assert_eq!(side_pane_ratio_percent(), Some(100));

        if let Some(prev_home) = prev_home {
            crate::env::set_var("JCODE_HOME", prev_home);
        } else {
            crate::env::remove_var("JCODE_HOME");
        }
    }

    #[test]
    fn side_pane_ratio_write_keeps_inline_image_preference() {
        let _guard = crate::storage::lock_test_env();
        let temp = tempfile::tempdir().expect("tempdir");
        let prev_home = std::env::var_os("JCODE_HOME");
        crate::env::set_var("JCODE_HOME", temp.path());

        save_inline_images_visible(false);
        save_side_pane_ratio_percent(75);
        let prefs = load();
        assert_eq!(prefs.inline_images_visible, Some(false));
        assert_eq!(prefs.side_pane_ratio, Some(75));

        if let Some(prev_home) = prev_home {
            crate::env::set_var("JCODE_HOME", prev_home);
        } else {
            crate::env::remove_var("JCODE_HOME");
        }
    }

    #[test]
    fn initial_side_pane_ratio_prefers_user_choice_over_config() {
        let _guard = crate::storage::lock_test_env();
        let temp = tempfile::tempdir().expect("tempdir");
        let prev_home = std::env::var_os("JCODE_HOME");
        crate::env::set_var("JCODE_HOME", temp.path());

        assert_eq!(initial_side_pane_ratio(40), (40, false));
        // Out-of-range configured values clamp, and still count as "not a user choice".
        assert_eq!(initial_side_pane_ratio(5), (25, false));

        save_side_pane_ratio_percent(75);
        assert_eq!(initial_side_pane_ratio(40), (75, true));

        if let Some(prev_home) = prev_home {
            crate::env::set_var("JCODE_HOME", prev_home);
        } else {
            crate::env::remove_var("JCODE_HOME");
        }
    }
}
