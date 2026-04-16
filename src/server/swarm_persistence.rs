use super::VersionedPlan;
use crate::storage;
use std::collections::HashMap;
use std::path::PathBuf;

const SWARM_STATE_DIR: &str = "jcode-swarm-state";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PersistedSwarmState {
    swarm_id: String,
    plan: PersistedVersionedPlan,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    coordinator_session_id: Option<String>,
    updated_at_unix_ms: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PersistedVersionedPlan {
    items: Vec<crate::plan::PlanItem>,
    version: u64,
    participants: Vec<String>,
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn state_dir() -> PathBuf {
    storage::runtime_dir().join(SWARM_STATE_DIR)
}

fn state_path(swarm_id: &str) -> PathBuf {
    let sanitized: String = swarm_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    state_dir().join(format!("{}.json", sanitized))
}

fn from_persisted_plan(mut plan: PersistedVersionedPlan) -> VersionedPlan {
    for item in &mut plan.items {
        if item.status == "running" {
            item.status = "running_stale".to_string();
        }
    }
    VersionedPlan {
        items: plan.items,
        version: plan.version,
        participants: plan.participants.into_iter().collect(),
    }
}

fn to_persisted_plan(plan: &VersionedPlan) -> PersistedVersionedPlan {
    let mut participants: Vec<String> = plan.participants.iter().cloned().collect();
    participants.sort();
    PersistedVersionedPlan {
        items: plan.items.clone(),
        version: plan.version,
        participants,
    }
}

pub(super) fn load_runtime_state() -> (HashMap<String, VersionedPlan>, HashMap<String, String>) {
    let dir = state_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return (HashMap::new(), HashMap::new());
    };

    let mut plans = HashMap::new();
    let mut coordinators = HashMap::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Ok(state) = storage::read_json::<PersistedSwarmState>(&path) else {
            continue;
        };
        let swarm_id = state.swarm_id.clone();
        plans.insert(swarm_id.clone(), from_persisted_plan(state.plan));
        if let Some(coordinator_session_id) = state.coordinator_session_id {
            coordinators.insert(swarm_id, coordinator_session_id);
        }
    }
    (plans, coordinators)
}

pub(super) fn persist_swarm_state(
    swarm_id: &str,
    swarm_plans: &HashMap<String, VersionedPlan>,
    swarm_coordinators: &HashMap<String, String>,
) {
    let Some(plan) = swarm_plans.get(swarm_id) else {
        let _ = std::fs::remove_file(state_path(swarm_id));
        return;
    };

    let state = PersistedSwarmState {
        swarm_id: swarm_id.to_string(),
        plan: to_persisted_plan(plan),
        coordinator_session_id: swarm_coordinators.get(swarm_id).cloned(),
        updated_at_unix_ms: now_unix_ms(),
    };

    if let Err(err) = storage::write_json_fast(&state_path(swarm_id), &state) {
        crate::logging::warn(&format!(
            "Failed to persist swarm state {}: {}",
            swarm_id, err
        ));
    }
}

pub(super) fn remove_swarm_state(swarm_id: &str) {
    let _ = std::fs::remove_file(state_path(swarm_id));
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EnvGuard {
        runtime: Option<std::ffi::OsString>,
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(value) = self.runtime.take() {
                crate::env::set_var("JCODE_RUNTIME_DIR", value);
            } else {
                crate::env::remove_var("JCODE_RUNTIME_DIR");
            }
        }
    }

    fn test_env(dir: &tempfile::TempDir) -> EnvGuard {
        let _guard = storage::lock_test_env();
        let previous = std::env::var_os("JCODE_RUNTIME_DIR");
        crate::env::set_var("JCODE_RUNTIME_DIR", dir.path());
        EnvGuard { runtime: previous }
    }

    #[test]
    fn persisted_swarm_state_round_trips_and_marks_running_stale() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let _env = test_env(&dir);

        let mut plans = HashMap::new();
        plans.insert(
            "swarm-alpha".to_string(),
            VersionedPlan {
                items: vec![crate::plan::PlanItem {
                    content: "do thing".to_string(),
                    status: "running".to_string(),
                    priority: "high".to_string(),
                    id: "task-1".to_string(),
                    blocked_by: Vec::new(),
                    assigned_to: Some("session-1".to_string()),
                }],
                version: 3,
                participants: ["session-1".to_string(), "session-2".to_string()]
                    .into_iter()
                    .collect(),
            },
        );
        let coordinators = HashMap::from([("swarm-alpha".to_string(), "session-2".to_string())]);

        persist_swarm_state("swarm-alpha", &plans, &coordinators);
        let (loaded_plans, loaded_coords) = load_runtime_state();

        let loaded = loaded_plans.get("swarm-alpha").expect("loaded plan");
        assert_eq!(loaded.version, 3);
        assert_eq!(loaded.items.len(), 1);
        assert_eq!(loaded.items[0].status, "running_stale");
        assert_eq!(
            loaded_coords.get("swarm-alpha"),
            Some(&"session-2".to_string())
        );
    }

    #[test]
    fn remove_swarm_state_deletes_persisted_snapshot() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let _env = test_env(&dir);

        let plans = HashMap::from([(
            "swarm-beta".to_string(),
            VersionedPlan {
                items: Vec::new(),
                version: 1,
                participants: Default::default(),
            },
        )]);
        let coordinators = HashMap::new();
        persist_swarm_state("swarm-beta", &plans, &coordinators);
        assert!(state_path("swarm-beta").exists());

        remove_swarm_state("swarm-beta");
        assert!(!state_path("swarm-beta").exists());
    }
}
