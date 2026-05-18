use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

const SCHEMA_VERSION: u32 = 1;
const DEFAULT_RETEST_DAYS: i64 = 14;
const LEDGER_ENV: &str = "JCODE_LIVE_TEST_LEDGER";
const COVERAGE_ENV: &str = "JCODE_LIVE_TEST_COVERAGE";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LiveVerificationResult {
    Passed,
    Failed,
    Blocked,
    Skipped,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LiveVerificationStageStatus {
    Passed,
    Failed,
    Blocked,
    Skipped,
    NotRun,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LiveVerificationStage {
    pub name: String,
    pub status: LiveVerificationStageStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub evidence: Map<String, Value>,
}

impl LiveVerificationStage {
    pub fn new(name: impl Into<String>, status: LiveVerificationStageStatus) -> Self {
        Self {
            name: name.into(),
            status,
            duration_ms: None,
            evidence: Map::new(),
        }
    }

    pub fn passed(name: impl Into<String>) -> Self {
        Self::new(name, LiveVerificationStageStatus::Passed)
    }

    pub fn failed(name: impl Into<String>, error: impl Into<String>) -> Self {
        Self::new(name, LiveVerificationStageStatus::Failed).with_evidence(
            "error",
            Value::String(redact_secret_like_text(&error.into())),
        )
    }

    pub fn blocked(name: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::new(name, LiveVerificationStageStatus::Blocked).with_evidence(
            "reason",
            Value::String(redact_secret_like_text(&reason.into())),
        )
    }

    pub fn skipped(name: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::new(name, LiveVerificationStageStatus::Skipped).with_evidence(
            "reason",
            Value::String(redact_secret_like_text(&reason.into())),
        )
    }

    pub fn with_duration_ms(mut self, duration_ms: u64) -> Self {
        self.duration_ms = Some(duration_ms);
        self
    }

    pub fn with_evidence(mut self, key: impl Into<String>, value: Value) -> Self {
        self.evidence.insert(key.into(), sanitize_json_value(value));
        self
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LiveVerificationAuth {
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
}

impl LiveVerificationAuth {
    pub fn from_secret(
        source: impl Into<String>,
        env_key: Option<impl Into<String>>,
        secret: &str,
    ) -> Self {
        Self {
            source: source.into(),
            env_key: env_key.map(Into::into),
            fingerprint: fingerprint_secret(secret),
        }
    }

    pub fn non_secret(source: impl Into<String>, env_key: Option<impl Into<String>>) -> Self {
        Self {
            source: source.into(),
            env_key: env_key.map(Into::into),
            fingerprint: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LiveVerificationBuild {
    pub jcode_version: String,
    pub jcode_git_hash: String,
    pub jcode_git_date: String,
    pub jcode_git_dirty: bool,
    pub jcode_semver: String,
    pub os: String,
    pub arch: String,
    pub pid: u32,
}

impl LiveVerificationBuild {
    pub fn current() -> Self {
        let version = env!("JCODE_VERSION").to_string();
        Self {
            jcode_git_dirty: version.contains("dirty"),
            jcode_version: version,
            jcode_git_hash: env!("JCODE_GIT_HASH").to_string(),
            jcode_git_date: env!("JCODE_GIT_DATE").to_string(),
            jcode_semver: env!("JCODE_SEMVER").to_string(),
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            pid: std::process::id(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LiveVerificationEvent {
    pub schema_version: u32,
    pub event_id: String,
    pub recorded_at: DateTime<Utc>,
    pub test_name: String,
    pub provider_id: String,
    pub provider_label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
    pub result: LiveVerificationResult,
    pub auth: LiveVerificationAuth,
    pub build: LiveVerificationBuild,
    pub retest_after: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stages: Vec<LiveVerificationStage>,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub metadata: Map<String, Value>,
}

impl LiveVerificationEvent {
    pub fn new(
        test_name: impl Into<String>,
        provider_id: impl Into<String>,
        provider_label: impl Into<String>,
        auth: LiveVerificationAuth,
        result: LiveVerificationResult,
    ) -> Self {
        let recorded_at = Utc::now();
        let test_name = test_name.into();
        let provider_id = provider_id.into();
        let provider_label = provider_label.into();
        let event_id = event_id(&recorded_at, &test_name, &provider_id);
        Self {
            schema_version: SCHEMA_VERSION,
            event_id,
            recorded_at,
            test_name,
            provider_id,
            provider_label,
            endpoint: None,
            model: None,
            capabilities: Vec::new(),
            result,
            auth,
            build: LiveVerificationBuild::current(),
            retest_after: recorded_at + Duration::days(DEFAULT_RETEST_DAYS),
            stages: Vec::new(),
            metadata: Map::new(),
        }
    }

    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = Some(endpoint.into());
        self
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    pub fn with_capabilities<I, S>(mut self, capabilities: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.capabilities = capabilities.into_iter().map(Into::into).collect();
        self.capabilities.sort();
        self.capabilities.dedup();
        self
    }

    pub fn with_retest_days(mut self, days: i64) -> Self {
        self.retest_after = self.recorded_at + Duration::days(days.max(1));
        self
    }

    pub fn with_stage(mut self, stage: LiveVerificationStage) -> Self {
        self.stages.push(stage);
        self
    }

    pub fn with_stages<I>(mut self, stages: I) -> Self
    where
        I: IntoIterator<Item = LiveVerificationStage>,
    {
        self.stages.extend(stages);
        self
    }

    pub fn with_metadata(mut self, key: impl Into<String>, value: Value) -> Self {
        self.metadata.insert(key.into(), sanitize_json_value(value));
        self
    }

    pub fn coverage_key(&self) -> String {
        let model = self.model.as_deref().unwrap_or("*");
        let capabilities = if self.capabilities.is_empty() {
            "unspecified".to_string()
        } else {
            self.capabilities.join("+")
        };
        format!("{}::{model}::{capabilities}", self.provider_id)
    }
}

pub fn append_event(event: &LiveVerificationEvent) -> Result<LiveVerificationPaths> {
    let paths = LiveVerificationPaths::resolve()?;
    append_event_to_paths(event, &paths)?;
    Ok(paths)
}

fn append_event_to_paths(
    event: &LiveVerificationEvent,
    paths: &LiveVerificationPaths,
) -> Result<()> {
    if let Some(parent) = paths.events.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create live test ledger dir {}", parent.display()))?;
    }
    let line = serde_json::to_string(event).context("serialize live verification event")?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&paths.events)
        .with_context(|| format!("open live test ledger {}", paths.events.display()))?;
    writeln!(file, "{line}").context("append live verification event")?;
    update_coverage(event, &paths.coverage)?;
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveVerificationPaths {
    pub events: PathBuf,
    pub coverage: PathBuf,
}

impl LiveVerificationPaths {
    pub fn resolve() -> Result<Self> {
        let events = std::env::var(LEDGER_ENV)
            .ok()
            .map(PathBuf::from)
            .unwrap_or(crate::storage::app_config_dir()?.join("live-tests/events.jsonl"));
        let coverage = std::env::var(COVERAGE_ENV)
            .ok()
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                events
                    .parent()
                    .map(|parent| parent.join("coverage.json"))
                    .unwrap_or_else(|| PathBuf::from("coverage.json"))
            });
        Ok(Self { events, coverage })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LiveVerificationCoverage {
    pub schema_version: u32,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub latest: BTreeMap<String, LiveVerificationCoverageEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LiveVerificationCoverageEntry {
    pub event_id: String,
    pub recorded_at: DateTime<Utc>,
    pub test_name: String,
    pub provider_id: String,
    pub provider_label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
    pub result: LiveVerificationResult,
    pub retest_after: DateTime<Utc>,
    pub jcode_version: String,
    pub jcode_git_hash: String,
    pub jcode_git_dirty: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stage_statuses: Vec<String>,
}

fn update_coverage(event: &LiveVerificationEvent, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create live test coverage dir {}", parent.display()))?;
    }
    let mut coverage = if path.exists() {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("read live test coverage {}", path.display()))?;
        serde_json::from_str::<LiveVerificationCoverage>(&raw).unwrap_or_else(|_| {
            LiveVerificationCoverage {
                schema_version: SCHEMA_VERSION,
                updated_at: Utc::now(),
                latest: BTreeMap::new(),
            }
        })
    } else {
        LiveVerificationCoverage {
            schema_version: SCHEMA_VERSION,
            updated_at: Utc::now(),
            latest: BTreeMap::new(),
        }
    };

    coverage.schema_version = SCHEMA_VERSION;
    coverage.updated_at = Utc::now();
    coverage.latest.insert(
        event.coverage_key(),
        LiveVerificationCoverageEntry {
            event_id: event.event_id.clone(),
            recorded_at: event.recorded_at,
            test_name: event.test_name.clone(),
            provider_id: event.provider_id.clone(),
            provider_label: event.provider_label.clone(),
            model: event.model.clone(),
            capabilities: event.capabilities.clone(),
            result: event.result.clone(),
            retest_after: event.retest_after,
            jcode_version: event.build.jcode_version.clone(),
            jcode_git_hash: event.build.jcode_git_hash.clone(),
            jcode_git_dirty: event.build.jcode_git_dirty,
            stage_statuses: event
                .stages
                .iter()
                .map(|stage| format!("{}:{:?}", stage.name, stage.status))
                .collect(),
        },
    );
    let serialized = serde_json::to_string_pretty(&coverage)
        .context("serialize live verification coverage summary")?;
    std::fs::write(path, serialized)
        .with_context(|| format!("write live test coverage {}", path.display()))?;
    Ok(())
}

pub fn fingerprint_secret(secret: &str) -> Option<String> {
    let trimmed = secret.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut hasher = Sha256::new();
    hasher.update(trimmed.as_bytes());
    Some(format!("sha256:{}", hex::encode(hasher.finalize())))
}

fn event_id(recorded_at: &DateTime<Utc>, test_name: &str, provider_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(recorded_at.to_rfc3339().as_bytes());
    hasher.update(b"\0");
    hasher.update(test_name.as_bytes());
    hasher.update(b"\0");
    hasher.update(provider_id.as_bytes());
    let digest = hex::encode(hasher.finalize());
    format!("live_{}", &digest[..16])
}

fn sanitize_json_value(value: Value) -> Value {
    match value {
        Value::String(text) => Value::String(redact_secret_like_text(&text)),
        Value::Array(items) => Value::Array(items.into_iter().map(sanitize_json_value).collect()),
        Value::Object(object) => Value::Object(
            object
                .into_iter()
                .map(|(key, value)| (key, sanitize_json_value(value)))
                .collect(),
        ),
        other => other,
    }
}

fn redact_secret_like_text(text: &str) -> String {
    let trimmed = text.trim();
    if looks_secret_like(trimmed) {
        "[REDACTED_SECRET]".to_string()
    } else {
        text.to_string()
    }
}

fn looks_secret_like(text: &str) -> bool {
    if text.len() < 16 {
        return false;
    }
    let lower = text.to_ascii_lowercase();
    lower.starts_with("sk-")
        || lower.starts_with("sk_")
        || lower.starts_with("oc_")
        || lower.starts_with("ghp_")
        || lower.starts_with("github_pat_")
        || lower.starts_with("ya29.")
        || lower.contains("api_key=")
        || lower.contains("authorization: bearer")
        || lower.contains("bearer ")
}

pub fn concise_model_sample(models: &[String], limit: usize) -> Value {
    let sample = models.iter().take(limit).cloned().collect::<Vec<String>>();
    json!({
        "count": models.len(),
        "sample": sample,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    struct EnvGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &Path) -> Self {
            let previous = std::env::var_os(key);
            crate::env::set_var(key, value.as_os_str());
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => crate::env::set_var(self.key, value),
                None => crate::env::remove_var(self.key),
            }
        }
    }

    #[test]
    fn live_verification_ledger_writes_events_and_coverage_without_secret() {
        let temp = TempDir::new().expect("temp dir");
        let events_path = temp.path().join("events.jsonl");
        let coverage_path = temp.path().join("coverage.json");
        let _events = EnvGuard::set(LEDGER_ENV, &events_path);
        let _coverage = EnvGuard::set(COVERAGE_ENV, &coverage_path);
        let secret = "sk-live-secret-that-must-not-appear";

        let event = LiveVerificationEvent::new(
            "live_test",
            "opencode",
            "OpenCode Zen",
            LiveVerificationAuth::from_secret("test env", Some("OPENCODE_API_KEY"), secret),
            LiveVerificationResult::Passed,
        )
        .with_model("kimi-k2.6")
        .with_capabilities(["models_endpoint", "tool_call"])
        .with_stage(
            LiveVerificationStage::passed("models_endpoint")
                .with_evidence("authorization", Value::String(format!("Bearer {secret}"))),
        );

        let paths = append_event(&event).expect("append event");
        assert_eq!(paths.events, events_path);
        assert_eq!(paths.coverage, coverage_path);

        let raw_events = std::fs::read_to_string(&paths.events).expect("events raw");
        assert!(!raw_events.contains(secret));
        assert!(raw_events.contains("[REDACTED_SECRET]"));
        assert!(raw_events.contains("sha256:"));

        let raw_coverage = std::fs::read_to_string(&paths.coverage).expect("coverage raw");
        assert!(!raw_coverage.contains(secret));
        assert!(raw_coverage.contains("opencode::kimi-k2.6::models_endpoint+tool_call"));
    }

    #[test]
    fn auth_fingerprint_is_stable_and_non_reversible() {
        let a = fingerprint_secret("secret-value");
        let b = fingerprint_secret("secret-value");
        let c = fingerprint_secret("different-secret");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(!a.unwrap().contains("secret-value"));
        assert_eq!(fingerprint_secret("   "), None);
    }
}
