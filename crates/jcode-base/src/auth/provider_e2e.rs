//! Provider strict end-to-end diagnostic runner.
//!
//! This powers `jcode provider-doctor`: it walks the same strict provider/model
//! checkpoints that the coverage ledger tracks, but as a user-facing diagnostic
//! so anyone can answer "why is my provider/model or model picker broken?".
//!
//! Three tiers trade off safety vs. coverage:
//! - [`DoctorTier::Offline`]: no API key, no network, no spend. Validates jcode's
//!   own wiring (catalog reload, picker rendering, fallback labeling, model-switch
//!   routing, auth-lifecycle transcript) against a synthetic catalog.
//! - [`DoctorTier::Catalog`]: needs a key, ~no spend. Everything in offline plus the
//!   live `GET /models` fetch (validates the key, the endpoint, and that the model
//!   exists in the live catalog).
//! - [`DoctorTier::Full`]: needs a key, spends balance. Everything in catalog plus a
//!   non-streaming completion, a streaming completion, and a tool-call loop.
//!
//! Only the [`DoctorTier::Full`] tier can earn strict coverage; the lighter tiers
//! intentionally record the API-dependent checkpoints as skipped so nothing is
//! over-credited in the ledger.

use crate::auth::lifecycle::{
    activate_auth_change, validate_catalog_invariants, AuthActivationRequest,
};
use crate::auth::live_provider_probes::{
    fetch_live_openai_compatible_models, run_live_openai_compatible_smoke,
    run_live_openai_compatible_stream_smoke, run_live_openai_compatible_tool_smoke,
};
use crate::live_tests::{
    self, checkpoints, LiveVerificationAuth, LiveVerificationEvent, LiveVerificationResult,
    LiveVerificationStage, LiveVerificationStageStatus,
};
use crate::protocol::{AuthChanged, CatalogNamespace, RuntimeProviderKey};
use crate::provider::ModelRoute;
use crate::provider_catalog::OpenAiCompatibleProfile;

/// How much of the strict pipeline to exercise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DoctorTier {
    /// No key, no network, no spend. Validates jcode-side wiring only.
    Offline,
    /// Needs a key, negligible spend. Adds the live model catalog fetch.
    Catalog,
    /// Needs a key, spends balance. Adds chat, streaming, and tool-call checkpoints.
    Full,
}

impl DoctorTier {
    pub fn requires_api_key(self) -> bool {
        !matches!(self, DoctorTier::Offline)
    }

    pub fn spends_balance(self) -> bool {
        matches!(self, DoctorTier::Full)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            DoctorTier::Offline => "offline",
            DoctorTier::Catalog => "catalog",
            DoctorTier::Full => "full",
        }
    }
}

impl std::str::FromStr for DoctorTier {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "offline" => Ok(DoctorTier::Offline),
            "catalog" => Ok(DoctorTier::Catalog),
            "full" => Ok(DoctorTier::Full),
            other => Err(format!(
                "unknown tier `{other}` (expected offline, catalog, or full)"
            )),
        }
    }
}

/// One checkpoint result in a doctor run.
#[derive(Clone, Debug)]
pub struct DoctorCheck {
    pub checkpoint: &'static str,
    pub label: &'static str,
    pub status: LiveVerificationStageStatus,
    /// Human-readable detail (failure reason, evidence summary, or skip reason).
    pub detail: String,
}

impl DoctorCheck {
    fn passed(checkpoint: &'static str, label: &'static str, detail: impl Into<String>) -> Self {
        Self {
            checkpoint,
            label,
            status: LiveVerificationStageStatus::Passed,
            detail: detail.into(),
        }
    }

    fn failed(checkpoint: &'static str, label: &'static str, detail: impl Into<String>) -> Self {
        Self {
            checkpoint,
            label,
            status: LiveVerificationStageStatus::Failed,
            detail: detail.into(),
        }
    }

    fn skipped(checkpoint: &'static str, label: &'static str, detail: impl Into<String>) -> Self {
        Self {
            checkpoint,
            label,
            status: LiveVerificationStageStatus::Skipped,
            detail: detail.into(),
        }
    }

    pub fn is_failure(&self) -> bool {
        matches!(
            self.status,
            LiveVerificationStageStatus::Failed | LiveVerificationStageStatus::Blocked
        )
    }
}

/// The complete result of a doctor run for one provider/model.
#[derive(Clone, Debug)]
pub struct DoctorReport {
    pub provider_id: String,
    pub provider_label: String,
    pub model: String,
    pub tier: DoctorTier,
    pub checks: Vec<DoctorCheck>,
    /// True when every required checkpoint for the chosen tier passed.
    pub tier_passed: bool,
    /// True when every strict checkpoint passed (only possible on the full tier).
    pub strict_passed: bool,
}

impl DoctorReport {
    pub fn first_failure(&self) -> Option<&DoctorCheck> {
        self.checks.iter().find(|check| check.is_failure())
    }
}

const FULL_PIPELINE_LABELS: &[(&str, &str)] = &[
    (checkpoints::AUTH_CREDENTIAL_LOADED, "Credential loaded"),
    (
        checkpoints::MODEL_CATALOG_LIVE_ENDPOINT,
        "Live model catalog endpoint",
    ),
    (
        checkpoints::CATALOG_HOT_RELOAD_CURRENT_SESSION,
        "Catalog hot reload in current session",
    ),
    (checkpoints::PICKER_LIVE_MODELS, "Picker shows live models"),
    (
        checkpoints::PICKER_FALLBACK_LABELING,
        "Picker fallback labeling",
    ),
    (checkpoints::MODEL_SWITCH_ROUTE, "Model switch route"),
    (
        checkpoints::NON_STREAMING_CHAT_COMPLETION,
        "Non-streaming chat completion",
    ),
    (
        checkpoints::STREAMING_CHAT_COMPLETION,
        "Streaming chat completion",
    ),
    (checkpoints::TOOL_CALL_PARSE, "Tool-call parse"),
    (checkpoints::TOOL_EXECUTION_LOOP, "Tool execution loop"),
    (checkpoints::TOOL_RESULT_FOLLOWUP, "Tool-result followup"),
    (checkpoints::REAL_JCODE_TOOL_SMOKE, "Real Jcode tool smoke"),
];

fn label_for(checkpoint: &str) -> &'static str {
    FULL_PIPELINE_LABELS
        .iter()
        .find(|(id, _)| *id == checkpoint)
        .map(|(_, label)| *label)
        .unwrap_or("Checkpoint")
}

/// Checkpoints that require a real API response and are therefore skipped on the
/// offline/catalog tiers.
const API_DEPENDENT_CHECKPOINTS: &[&str] = &[
    checkpoints::NON_STREAMING_CHAT_COMPLETION,
    checkpoints::STREAMING_CHAT_COMPLETION,
    checkpoints::TOOL_CALL_PARSE,
    checkpoints::TOOL_EXECUTION_LOOP,
    checkpoints::TOOL_RESULT_FOLLOWUP,
    checkpoints::REAL_JCODE_TOOL_SMOKE,
];

/// Run the strict provider/model diagnostic.
///
/// `api_key` may be `None` only when `tier == DoctorTier::Offline`.
pub async fn run_provider_e2e(
    profile: OpenAiCompatibleProfile,
    api_key: Option<&str>,
    requested_model: Option<&str>,
    tier: DoctorTier,
) -> anyhow::Result<DoctorReport> {
    let resolved = crate::provider_catalog::resolve_openai_compatible_profile(profile);
    let provider_id = profile.id.to_string();
    let provider_label = profile.display_name.to_string();
    let mut checks: Vec<DoctorCheck> = Vec::new();

    if tier.requires_api_key() && api_key.map(str::trim).unwrap_or("").is_empty() {
        anyhow::bail!(
            "tier `{}` requires an API key for provider `{}` but none was supplied",
            tier.as_str(),
            provider_id
        );
    }

    // --- Stage 1: credential loaded ---
    match api_key.map(str::trim).filter(|key| !key.is_empty()) {
        Some(_) => checks.push(DoctorCheck::passed(
            checkpoints::AUTH_CREDENTIAL_LOADED,
            label_for(checkpoints::AUTH_CREDENTIAL_LOADED),
            format!("Loaded credential from {}", resolved.api_key_env),
        )),
        None => checks.push(DoctorCheck::skipped(
            checkpoints::AUTH_CREDENTIAL_LOADED,
            label_for(checkpoints::AUTH_CREDENTIAL_LOADED),
            "offline tier: no credential required".to_string(),
        )),
    }

    // --- Stage 2: live model catalog (or synthetic for offline) ---
    let catalog_models: Vec<String> = if tier.requires_api_key() {
        match fetch_live_openai_compatible_models(profile, api_key.unwrap_or_default()).await {
            Ok(models) => {
                checks.push(DoctorCheck::passed(
                    checkpoints::MODEL_CATALOG_LIVE_ENDPOINT,
                    label_for(checkpoints::MODEL_CATALOG_LIVE_ENDPOINT),
                    format!("{} live model(s) returned", models.len()),
                ));
                models
            }
            Err(error) => {
                checks.push(DoctorCheck::failed(
                    checkpoints::MODEL_CATALOG_LIVE_ENDPOINT,
                    label_for(checkpoints::MODEL_CATALOG_LIVE_ENDPOINT),
                    error.to_string(),
                ));
                return Ok(finish_report(
                    provider_id,
                    provider_label,
                    requested_model.unwrap_or("").to_string(),
                    tier,
                    checks,
                    api_key,
                    resolved.api_key_env.clone(),
                    resolved.env_file.clone(),
                ));
            }
        }
    } else {
        // Offline tier: synthesize a small catalog so we can still validate wiring.
        checks.push(DoctorCheck::skipped(
            checkpoints::MODEL_CATALOG_LIVE_ENDPOINT,
            label_for(checkpoints::MODEL_CATALOG_LIVE_ENDPOINT),
            "offline tier: using synthetic catalog (no network)".to_string(),
        ));
        let default_model = profile.default_model.unwrap_or("fixture-model");
        vec![
            default_model.to_string(),
            format!("{}-alternate-fixture-model", profile.id),
        ]
    };

    // Pick the model under test.
    let selected = match requested_model.map(str::trim).filter(|m| !m.is_empty()) {
        Some(model) => {
            if tier.requires_api_key() && !catalog_models.iter().any(|m| m == model) {
                checks.push(DoctorCheck::failed(
                    checkpoints::MODEL_CATALOG_LIVE_ENDPOINT,
                    label_for(checkpoints::MODEL_CATALOG_LIVE_ENDPOINT),
                    format!(
                        "requested model `{model}` is not in the live catalog ({} model(s): {})",
                        catalog_models.len(),
                        truncate_list(&catalog_models)
                    ),
                ));
                return Ok(finish_report(
                    provider_id,
                    provider_label,
                    model.to_string(),
                    tier,
                    checks,
                    api_key,
                    resolved.api_key_env.clone(),
                    resolved.env_file.clone(),
                ));
            }
            model.to_string()
        }
        None => profile
            .default_model
            .filter(|default| catalog_models.iter().any(|m| m == default))
            .map(ToString::to_string)
            .or_else(|| catalog_models.first().cloned())
            .unwrap_or_else(|| "fixture-model".to_string()),
    };

    // --- Stage 3: auth-lifecycle wiring (catalog reload, picker, fallback, switch) ---
    run_wiring_checks(profile, &selected, &catalog_models, &mut checks);

    // --- Stage 4: API-dependent checkpoints ---
    if tier == DoctorTier::Full {
        run_full_api_checks(profile, api_key.unwrap_or_default(), &selected, &mut checks).await;
    } else {
        for checkpoint in API_DEPENDENT_CHECKPOINTS {
            checks.push(DoctorCheck::skipped(
                checkpoint,
                label_for(checkpoint),
                format!(
                    "{} tier: requires --tier full (spends balance)",
                    tier.as_str()
                ),
            ));
        }
    }

    Ok(finish_report(
        provider_id,
        provider_label,
        selected,
        tier,
        checks,
        api_key,
        resolved.api_key_env.clone(),
        resolved.env_file.clone(),
    ))
}

fn run_wiring_checks(
    profile: OpenAiCompatibleProfile,
    selected: &str,
    catalog_models: &[String],
    checks: &mut Vec<DoctorCheck>,
) {
    // Build the live-catalog routes the same way the runtime does after auth,
    // then drive the production auth-activation + catalog-invariant logic. This
    // exercises jcode's real wiring without the test sandbox.
    let api_method = format!("openai-compatible:{}", profile.id);
    let catalog_routes: Vec<ModelRoute> = catalog_models
        .iter()
        .map(|model| ModelRoute {
            model: model.clone(),
            provider: profile.display_name.to_string(),
            api_method: api_method.clone(),
            available: true,
            detail: "live-catalog route".to_string(),
            cheapness: None,
        })
        .collect();

    let auth = AuthChanged {
        provider: crate::protocol::AuthProviderId::new(profile.id),
        credential_source: None,
        auth_method: None,
        expected_runtime: Some(RuntimeProviderKey::new("openai-compatible")),
        expected_catalog_namespace: Some(CatalogNamespace::new(profile.id)),
    };
    let activation = activate_auth_change(&AuthActivationRequest::new(None, Some(auth)));

    // Provider-matched, available routes are what the picker would surface.
    let provider_entries: Vec<String> = catalog_routes
        .iter()
        .filter(|route| {
            route.available
                && (route.api_method.eq_ignore_ascii_case(&api_method)
                    || route.api_method.eq_ignore_ascii_case(profile.id))
        })
        .map(|route| route.model.clone())
        .collect();

    let catalog_report =
        validate_catalog_invariants(&activation, Some(selected), &catalog_routes);

    // Catalog hot reload.
    if catalog_report.ok() {
        checks.push(DoctorCheck::passed(
            checkpoints::CATALOG_HOT_RELOAD_CURRENT_SESSION,
            label_for(checkpoints::CATALOG_HOT_RELOAD_CURRENT_SESSION),
            format!("{} catalog route(s) reloaded", catalog_routes.len()),
        ));
    } else {
        checks.push(DoctorCheck::failed(
            checkpoints::CATALOG_HOT_RELOAD_CURRENT_SESSION,
            label_for(checkpoints::CATALOG_HOT_RELOAD_CURRENT_SESSION),
            catalog_report
                .warning_message()
                .unwrap_or_else(|| "catalog hot-reload invariant failed".to_string()),
        ));
    }

    // Picker shows live models.
    if provider_entries.is_empty() {
        checks.push(DoctorCheck::failed(
            checkpoints::PICKER_LIVE_MODELS,
            label_for(checkpoints::PICKER_LIVE_MODELS),
            "picker had no provider entries after auth".to_string(),
        ));
    } else if provider_entries.iter().any(|entry| entry == selected) {
        checks.push(DoctorCheck::passed(
            checkpoints::PICKER_LIVE_MODELS,
            label_for(checkpoints::PICKER_LIVE_MODELS),
            format!(
                "{} model(s) in picker, selected `{selected}`",
                provider_entries.len()
            ),
        ));
    } else {
        checks.push(DoctorCheck::failed(
            checkpoints::PICKER_LIVE_MODELS,
            label_for(checkpoints::PICKER_LIVE_MODELS),
            format!("selected model `{selected}` not present in picker entries"),
        ));
    }

    // Picker fallback labeling: every provider-matched route must be live-catalog
    // backed, never a static fallback.
    let matching_routes: Vec<&ModelRoute> = catalog_routes
        .iter()
        .filter(|route| route.available && route.provider == profile.display_name)
        .collect();
    let from_live_catalog = matching_routes
        .iter()
        .all(|route| route.detail.contains("live-catalog"));
    let has_static_fallback = matching_routes
        .iter()
        .any(|route| route.detail.to_ascii_lowercase().contains("static fallback"));
    if matching_routes.is_empty() {
        checks.push(DoctorCheck::failed(
            checkpoints::PICKER_FALLBACK_LABELING,
            label_for(checkpoints::PICKER_FALLBACK_LABELING),
            "no provider-matched catalog routes to label".to_string(),
        ));
    } else if from_live_catalog && !has_static_fallback {
        checks.push(DoctorCheck::passed(
            checkpoints::PICKER_FALLBACK_LABELING,
            label_for(checkpoints::PICKER_FALLBACK_LABELING),
            "all routes backed by live catalog (no static fallback)".to_string(),
        ));
    } else {
        checks.push(DoctorCheck::failed(
            checkpoints::PICKER_FALLBACK_LABELING,
            label_for(checkpoints::PICKER_FALLBACK_LABELING),
            "found static-fallback routes where live-catalog routes were expected".to_string(),
        ));
    }

    // Model switch route: switching to another model must produce a provider-explicit
    // request routed through this provider's api method.
    let switch_target = provider_entries
        .iter()
        .find(|model| model.as_str() != selected)
        .or_else(|| provider_entries.first());
    match switch_target {
        Some(target) => {
            let request = activation.model_switch_request("mock-auth", target);
            let request_ok = request.starts_with(&format!("{}:", profile.id));
            if request_ok {
                checks.push(DoctorCheck::passed(
                    checkpoints::MODEL_SWITCH_ROUTE,
                    label_for(checkpoints::MODEL_SWITCH_ROUTE),
                    format!("switch request `{request}` routed via `{api_method}`"),
                ));
            } else {
                checks.push(DoctorCheck::failed(
                    checkpoints::MODEL_SWITCH_ROUTE,
                    label_for(checkpoints::MODEL_SWITCH_ROUTE),
                    format!(
                        "model switch produced non-provider-explicit request `{request}` (expected `{}:`)",
                        profile.id
                    ),
                ));
            }
        }
        None => checks.push(DoctorCheck::failed(
            checkpoints::MODEL_SWITCH_ROUTE,
            label_for(checkpoints::MODEL_SWITCH_ROUTE),
            "no switch target available from picker entries".to_string(),
        )),
    }
}

async fn run_full_api_checks(
    profile: OpenAiCompatibleProfile,
    api_key: &str,
    selected: &str,
    checks: &mut Vec<DoctorCheck>,
) {
    // Non-streaming completion.
    match run_live_openai_compatible_smoke(profile, api_key, selected).await {
        Ok(_) => checks.push(DoctorCheck::passed(
            checkpoints::NON_STREAMING_CHAT_COMPLETION,
            label_for(checkpoints::NON_STREAMING_CHAT_COMPLETION),
            "received expected completion".to_string(),
        )),
        Err(error) => checks.push(DoctorCheck::failed(
            checkpoints::NON_STREAMING_CHAT_COMPLETION,
            label_for(checkpoints::NON_STREAMING_CHAT_COMPLETION),
            error.to_string(),
        )),
    }

    // Streaming completion.
    match run_live_openai_compatible_stream_smoke(profile, api_key, selected).await {
        Ok(_) => checks.push(DoctorCheck::passed(
            checkpoints::STREAMING_CHAT_COMPLETION,
            label_for(checkpoints::STREAMING_CHAT_COMPLETION),
            "received expected streamed completion".to_string(),
        )),
        Err(error) => checks.push(DoctorCheck::failed(
            checkpoints::STREAMING_CHAT_COMPLETION,
            label_for(checkpoints::STREAMING_CHAT_COMPLETION),
            error.to_string(),
        )),
    }

    // Tool call + derived execution/result/smoke checkpoints (one round-trip).
    match run_live_openai_compatible_tool_smoke(profile, api_key, selected).await {
        Ok(_) => {
            for checkpoint in [
                checkpoints::TOOL_CALL_PARSE,
                checkpoints::TOOL_EXECUTION_LOOP,
                checkpoints::TOOL_RESULT_FOLLOWUP,
                checkpoints::REAL_JCODE_TOOL_SMOKE,
            ] {
                checks.push(DoctorCheck::passed(
                    checkpoint,
                    label_for(checkpoint),
                    "tool call parsed and executed".to_string(),
                ));
            }
        }
        Err(error) => {
            for checkpoint in [
                checkpoints::TOOL_CALL_PARSE,
                checkpoints::TOOL_EXECUTION_LOOP,
                checkpoints::TOOL_RESULT_FOLLOWUP,
                checkpoints::REAL_JCODE_TOOL_SMOKE,
            ] {
                checks.push(DoctorCheck::failed(
                    checkpoint,
                    label_for(checkpoint),
                    error.to_string(),
                ));
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn finish_report(
    provider_id: String,
    provider_label: String,
    model: String,
    tier: DoctorTier,
    checks: Vec<DoctorCheck>,
    api_key: Option<&str>,
    api_key_env: String,
    env_file: String,
) -> DoctorReport {
    // A tier passes when none of its non-skipped checks failed.
    let tier_passed = !checks.iter().any(|check| check.is_failure());
    // Strict passes only on the full tier with every strict checkpoint passed.
    let strict_passed = tier == DoctorTier::Full
        && live_tests::strict_provider_model_coverage_checkpoint_ids().all(|checkpoint| {
            checks.iter().any(|check| {
                check.checkpoint == checkpoint
                    && check.status == LiveVerificationStageStatus::Passed
            })
        });

    record_event(
        &provider_id,
        &provider_label,
        &model,
        tier,
        &checks,
        api_key,
        &api_key_env,
        &env_file,
        strict_passed || tier_passed,
    );

    DoctorReport {
        provider_id,
        provider_label,
        model,
        tier,
        checks,
        tier_passed,
        strict_passed,
    }
}

#[allow(clippy::too_many_arguments)]
fn record_event(
    provider_id: &str,
    provider_label: &str,
    model: &str,
    tier: DoctorTier,
    checks: &[DoctorCheck],
    api_key: Option<&str>,
    api_key_env: &str,
    env_file: &str,
    overall_passed: bool,
) {
    let mut stages: Vec<LiveVerificationStage> = Vec::new();
    let mut expected: Vec<&'static str> = Vec::new();
    let mut capabilities: Vec<&'static str> = Vec::new();
    for check in checks {
        expected.push(check.checkpoint);
        let stage = match check.status {
            LiveVerificationStageStatus::Passed => {
                capabilities.push(check.checkpoint);
                LiveVerificationStage::passed(check.checkpoint)
                    .with_evidence("detail", serde_json::json!(check.detail))
            }
            LiveVerificationStageStatus::Failed => {
                LiveVerificationStage::failed(check.checkpoint, check.detail.clone())
            }
            LiveVerificationStageStatus::Skipped => {
                LiveVerificationStage::skipped(check.checkpoint, check.detail.clone())
            }
            LiveVerificationStageStatus::Blocked => {
                LiveVerificationStage::blocked(check.checkpoint, check.detail.clone())
            }
            LiveVerificationStageStatus::NotRun => {
                LiveVerificationStage::not_run(check.checkpoint, check.detail.clone())
            }
        };
        stages.push(stage);
    }

    let auth = match api_key {
        Some(key) if !key.trim().is_empty() => {
            LiveVerificationAuth::from_secret(format!("{api_key_env} via {env_file}"), Some(api_key_env), key)
        }
        _ => LiveVerificationAuth::non_secret("provider-doctor (offline)", Some(api_key_env)),
    };

    let result = if overall_passed {
        LiveVerificationResult::Passed
    } else {
        LiveVerificationResult::Failed
    };

    let mut event = LiveVerificationEvent::new(
        "provider_doctor_strict_e2e",
        provider_id,
        provider_label,
        auth,
        result,
    )
    .with_expected_checkpoints(expected)
    .with_capabilities(capabilities)
    .with_stages(stages)
    .with_metadata("doctor_tier", serde_json::json!(tier.as_str()))
    .with_metadata(
        "checkpoint_taxonomy_version",
        serde_json::json!(live_tests::CHECKPOINT_TAXONOMY_VERSION),
    );
    if !model.trim().is_empty() {
        event = event.with_model(model.to_string());
    }
    if let Err(error) = live_tests::append_event(&event) {
        eprintln!("provider-doctor: failed to record live verification event: {error}");
    }
}

fn truncate_list(models: &[String]) -> String {
    let shown: Vec<&str> = models.iter().take(8).map(String::as_str).collect();
    let mut out = shown.join(", ");
    if models.len() > shown.len() {
        out.push_str(&format!(", +{} more", models.len() - shown.len()));
    }
    out
}
