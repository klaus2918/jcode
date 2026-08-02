//! User model capability registry (`modelcap.json`) support.
//!
//! Users can extend or correct the embedded capability registry without
//! recompiling: `~/.jcode/modelcap.json` (or `$JCODE_MODELCAP_PATH`) is
//! loaded on every config load and installed into the provider-core user
//! registry. Resolution order stays `explicit config > registry (user first,
//! then embedded) > heuristic > default`, and entries are validated leniently
//! — unknown JSON fields are ignored, an unparseable file or an entry without
//! a model id is skipped with a warning (mirroring the Reasonix design
//! 03 §3.3.4 / §3.9).

use jcode_provider_core::{UserRegistryEntry, UserRegistryFile};
use std::path::PathBuf;

pub const MODELCAP_FILENAME: &str = "modelcap.json";

/// Env override so tests (and advanced users) can point at a registry file
/// outside the config dir.
pub const MODELCAP_ENV_KEY: &str = "JCODE_MODELCAP_PATH";

/// Cache of the last successfully loaded registry file's (path, mtime) so
/// frequent `Config::load()` calls (e.g. every `/model` default-model write)
/// do not re-read and re-parse modelcap.json on every touch. A missing file is
/// deliberately not cached: it costs one stat to rediscover a newly created
/// registry.
static LAST_LOADED: std::sync::Mutex<Option<(PathBuf, std::time::SystemTime)>> =
    std::sync::Mutex::new(None);

pub fn modelcap_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(MODELCAP_ENV_KEY) {
        return Some(PathBuf::from(path));
    }
    crate::storage::jcode_dir()
        .ok()
        .map(|dir| dir.join(MODELCAP_FILENAME))
}

/// Load the user registry file (if any) and install it into the capability
/// layer. Missing file / parse failure clears the registry and logs a warning,
/// so stale entries never survive a config reload.
pub fn load_user_modelcap_registry() {
    let path = match modelcap_path() {
        Some(path) => path,
        None => {
            jcode_provider_core::clear_user_registry_entries();
            return;
        }
    };

    if let Ok(metadata) = std::fs::metadata(&path)
        && let Ok(mtime) = metadata.modified()
        && let Ok(last) = LAST_LOADED.lock()
        && last.as_ref().is_some_and(|(cached_path, cached_mtime)| {
            *cached_path == path && *cached_mtime == mtime
        })
    {
        return;
    }

    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            jcode_provider_core::clear_user_registry_entries();
            return;
        }
        Err(error) => {
            crate::logging::warn(&format!(
                "Failed to read modelcap registry {}: {}",
                path.display(),
                error
            ));
            jcode_provider_core::clear_user_registry_entries();
            return;
        }
    };

    let file: UserRegistryFile = match serde_json::from_str(&content) {
        Ok(file) => file,
        Err(error) => {
            crate::logging::warn(&format!(
                "Failed to parse modelcap registry {}: {} (registry ignored)",
                path.display(),
                error
            ));
            jcode_provider_core::clear_user_registry_entries();
            return;
        }
    };

    let mut installed = 0usize;
    let entries: Vec<UserRegistryEntry> = file
        .entries
        .into_iter()
        .filter(|entry| {
            if entry.model.trim().is_empty() {
                crate::logging::warn("modelcap.json entry with an empty model id ignored");
                return false;
            }
            true
        })
        .inspect(|_| installed += 1)
        .collect();

    if installed > 0 {
        crate::logging::info(&format!(
            "Loaded {} model capability entr{} from {}",
            installed,
            if installed == 1 { "y" } else { "ies" },
            path.display()
        ));
    }
    if let Ok(mut last) = LAST_LOADED.lock() {
        *last = std::fs::metadata(&path)
            .and_then(|metadata| metadata.modified())
            .ok()
            .map(|mtime| (path.clone(), mtime));
    }
    jcode_provider_core::set_user_registry_entries(entries);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static MODELCAP_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn with_temp_home<T>(f: impl FnOnce(&std::path::Path) -> T) -> T {
        let _guard = MODELCAP_TEST_LOCK.lock().unwrap();
        let _env_guard = crate::storage::lock_test_env();
        let temp = tempfile::tempdir().expect("tempdir");
        crate::env::set_var("JCODE_HOME", temp.path());
        crate::env::remove_var(MODELCAP_ENV_KEY);
        let result = f(temp.path());
        crate::env::remove_var("JCODE_HOME");
        result
    }

    #[test]
    fn modelcap_file_installs_user_registry_entries() {
        with_temp_home(|home| {
            std::fs::create_dir_all(home).expect("home dir");
            std::fs::write(
                home.join(MODELCAP_FILENAME),
                r#"{
                  "entries": [
                    {
                      "model": "my-gateway-vlm",
                      "provider": "my-gateway",
                      "vision": true,
                      "tools": false,
                      "context_window": 128000,
                      "efforts": ["low", "high"],
                      "default_effort": "high"
                    }
                  ]
                }"#,
            )
            .expect("write modelcap.json");

            load_user_modelcap_registry();

            let resolved =
                jcode_provider_core::resolve_capability("my-gateway-vlm", Some("my-gateway"), None);
            assert!(resolved.capability.supports_image());
            assert!(!resolved.capability.supports_tools());
            assert_eq!(resolved.capability.context_window, Some(128_000));
            assert_eq!(resolved.capability.reasoning.efforts, ["low", "high"]);
            assert_eq!(
                resolved.capability.reasoning.default_effort.as_deref(),
                Some("high")
            );
            assert_eq!(
                resolved.trace.vision,
                Some(jcode_provider_core::CapabilitySource::Registry)
            );

            jcode_provider_core::clear_user_registry_entries();
        });
    }

    #[test]
    fn malformed_modelcap_file_is_ignored_with_warning() {
        with_temp_home(|home| {
            std::fs::write(home.join(MODELCAP_FILENAME), "not json {").expect("write");
            load_user_modelcap_registry();

            let resolved =
                jcode_provider_core::resolve_capability("deepseek-v4-pro", Some("deepseek"), None);
            assert_eq!(
                resolved.trace.context_window,
                Some(jcode_provider_core::CapabilitySource::Registry),
                "embedded registry still serves defaults after a malformed user file"
            );
            assert_eq!(resolved.capability.context_window, Some(1_000_000));

            jcode_provider_core::clear_user_registry_entries();
        });
    }

    #[test]
    fn missing_modelcap_file_clears_previous_entries() {
        with_temp_home(|home| {
            std::fs::write(
                home.join(MODELCAP_FILENAME),
                r#"{"entries":[{"model":"ghost-model","tools":false}]}"#,
            )
            .expect("write");
            load_user_modelcap_registry();
            assert!(
                !jcode_provider_core::resolve_capability("ghost-model", None, None)
                    .capability
                    .supports_tools()
            );

            std::fs::remove_file(home.join(MODELCAP_FILENAME)).expect("remove");
            load_user_modelcap_registry();
            assert!(
                jcode_provider_core::resolve_capability("ghost-model", None, None)
                    .capability
                    .supports_tools(),
                "removing the user file must clear the user registry"
            );
        });
    }
}
