//! Anthropic Messages API runtime for user-defined named provider profiles
//! (`[providers.<name>]` with `api = "anthropic"` in `config.toml`).
//!
//! Unlike the primary [`super::AnthropicProvider`] 鈥?which is hardwired to
//! `api.anthropic.com` and the Claude OAuth/API-key credential setup 鈥?this
//! runtime points the Anthropic wire format at an arbitrary endpoint and uses
//! the named profile's own auth (bearer / api-key header / none). It reuses the
//! same request shaping (`jcode_provider_anthropic`) and SSE parsing
//! ([`super::process_sse_event`]) as the primary provider, so Anthropic-compatible
//! gateways and routers (e.g. self-hosted Claude-code relays, claude-format
//! proxies) get first-class jcode support.

use super::*;
use jcode_base::config::{NamedProviderAuth, NamedProviderConfig};
use jcode_base::provider_catalog::normalize_api_base_relaxed;
use reqwest::header::HeaderName;
use std::collections::HashMap;
use std::sync::Arc;

/// Auth mode for a named Anthropic-format endpoint.
#[derive(Debug, Clone)]
pub enum NamedAnthropicAuth {
    None {
        label: String,
    },
    Bearer {
        token: String,
        label: String,
    },
    Header {
        header_name: HeaderName,
        value: String,
        label: String,
    },
    /// Credential-requiring named profile constructed while the API key was
    /// absent. Construction succeeds so the provider is installed and its
    /// configured models stay switchable; the first request reports the
    /// missing key through `apply`.
    Missing {
        label: String,
    },
}

impl NamedAnthropicAuth {
    async fn apply(&self, req: reqwest::RequestBuilder) -> Result<reqwest::RequestBuilder> {
        match self {
            Self::None { .. } => Ok(req),
            Self::Bearer { token, .. } => Ok(req.bearer_auth(token)),
            Self::Header {
                header_name, value, ..
            } => Ok(req.header(header_name, value)),
            Self::Missing { label } => anyhow::bail!("{} not found in environment", label),
        }
    }

    fn label(&self) -> &str {
        match self {
            Self::None { label }
            | Self::Bearer { label, .. }
            | Self::Header { label, .. }
            | Self::Missing { label } => label,
        }
    }
}

/// GET `{base}/v1/models` 拉取网关模型目录，返回模型 id 列表。
///
/// 兼容两种目录格式：
/// - Anthropic 标准：`{"data": [{"id": "..."}]}`
/// - cc-switch 本地网关：`{"models": [{"slug": "...", "display_name": "..."}]}`
///
/// 独立于 `NamedAnthropicProvider` 的方法，便于重试任务（`tokio::spawn`
/// 需要 `'static`）内直接刷新目录，跟随 cc-switch 面板切换的 provider。
async fn fetch_gateway_catalog(
    client: &Client,
    api_base: &str,
    auth: &NamedAnthropicAuth,
) -> Result<Vec<String>> {
    let url = jcode_base::provider::anthropic::models_url_from_api_base(api_base);
    let response = auth
        .apply(client.get(&url).header("anthropic-version", "2023-06-01"))
        .await?
        .send()
        .await
        .with_context(|| {
            format!(
                "Failed to send Anthropic-compatible model catalog request\n  endpoint: {}",
                url
            )
        })?;
    if !response.status().is_success() {
        let status = response.status();
        let body = jcode_base::util::http_error_body(response, "HTTP error").await;
        anyhow::bail!(
            "Anthropic-compatible model catalog request failed\n  endpoint: {}\n  auth: {}\n  status: {}\n  response: {}\nHint: verify the base URL resolves to the Anthropic `/v1/models` endpoint and the key is valid.",
            url,
            auth.label(),
            status,
            body
        );
    }

    let data: Value = response
        .json()
        .await
        .context("Failed to parse Anthropic model catalog")?;
    let models = data
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.get("id").and_then(Value::as_str))
        .chain(
            data.get("models")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|entry| entry.get("slug").and_then(Value::as_str)),
        )
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if models.is_empty() {
        anyhow::bail!("Anthropic-compatible model catalog response did not contain any model ids");
    }
    Ok(models)
}

/// Anthropic Messages API runtime for a user-defined named provider profile.
pub struct NamedAnthropicProvider {
    client: Client,
    model: Arc<std::sync::RwLock<String>>,
    reasoning_effort: Arc<std::sync::RwLock<Option<String>>>,
    /// Normalized base URL (e.g. `https://gateway.example.com/v1`). The
    /// Messages endpoint is derived as `{base}/v1/messages` (or used verbatim
    /// when `base` already ends in `/messages`).
    api_base: String,
    /// Current auth (provider-level or the active model's override), refreshed
    /// on every `set_model`.
    auth: Arc<std::sync::RwLock<NamedAnthropicAuth>>,
    /// Provider-level fallback auth used by models without their own override.
    fallback_auth: NamedAnthropicAuth,
    /// Per-model auth overrides (keyed by lowercased model id), built from
    /// each `[[providers.<name>.models]]` entry's `api_key_env` / `auth` /
    /// `auth_header` / `env_file`.
    per_model_auth: HashMap<String, NamedAnthropicAuth>,
    profile_name: String,
    supports_model_catalog: bool,
    static_models: Vec<String>,
    static_context_limits: HashMap<String, usize>,
    models_cache: Arc<RwLock<Vec<String>>>,
    max_tokens_override: Option<u32>,
    /// Extra top-level request-body fields merged into every Messages request.
    extra_body: Option<serde_json::Map<String, Value>>,
}

impl NamedAnthropicProvider {
    /// Derive the Messages endpoint from a normalized profile `base_url`.
    ///
    /// Accepts any of:
    /// - `https://host`                     -> `https://host/v1/messages`
    /// - `https://host/v1`                  -> `https://host/v1/messages`
    /// - `https://host/v1/messages`         -> verbatim
    /// - `https://host/anthropic`           -> `https://host/anthropic/v1/messages`
    fn messages_url(base: &str) -> String {
        let trimmed = base.trim_end_matches('/');
        if trimmed.ends_with("/messages") {
            trimmed.to_string()
        } else if trimmed.ends_with("/v1") {
            format!("{trimmed}/messages")
        } else {
            format!("{trimmed}/v1/messages")
        }
    }

    /// Snapshot of the currently active auth (provider-level or the active
    /// model's override).
    fn auth(&self) -> NamedAnthropicAuth {
        self.auth
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    async fn fetch_models(&self) -> Result<Vec<String>> {
        let models = fetch_gateway_catalog(&self.client, &self.api_base, &self.auth()).await?;
        if let Ok(mut cache) = self.models_cache.try_write() {
            *cache = models.clone();
        }
        Ok(models)
    }

    /// 未配置默认模型（`default_model` 为空）时，从模型目录自动选择当前
    /// 可用的第一个模型。
    ///
    /// 典型场景：cc-switch 本地网关。模型名跟随 cc-switch 面板切换的
    /// provider 自动变化，jcode 无需在配置里写死；显式指定 `default_model`
    /// 或 `--model` 后本方法直接返回 `false`，不会覆盖显式选择。
    async fn auto_select_model(&self) -> Result<bool> {
        if !self.model().trim().is_empty() {
            return Ok(false);
        }
        let models = self.fetch_models().await?;
        let first = models
            .first()
            .ok_or_else(|| anyhow::anyhow!("model catalog reported no models"))?;
        self.set_model(first)?;
        Ok(true)
    }

    fn resolve_key(
        env_key: Option<&str>,
        inline_key: Option<&str>,
        env_file: Option<&str>,
    ) -> Option<String> {
        if let Some(env_key) = env_key.map(str::trim).filter(|v| !v.is_empty()) {
            // The unified `<jcode home>/.env` is the authoritative key source
            // (resonix-aligned): even without an explicit env_file, consult it
            // first via a safe fallback filename so
            // `load_api_key_from_env_or_config`'s lookup order applies
            // (unified .env -> process env -> legacy scattered files).
            let env_file = env_file
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .unwrap_or("named-provider.env");
            return jcode_base::provider_catalog::load_api_key_from_env_or_config(
                env_key, env_file,
            );
        }
        inline_key
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(ToString::to_string)
    }

    fn resolve_named_key(profile: &NamedProviderConfig) -> Option<String> {
        Self::resolve_key(
            profile.api_key_env.as_deref(),
            profile.api_key.as_deref(),
            profile.env_file.as_deref(),
        )
    }

    /// Build the runtime for a named Anthropic-format profile.
    pub fn new_named(profile_name: &str, profile: &NamedProviderConfig) -> Result<Self> {
        jcode_base::env::set_var("JCODE_OPENROUTER_CACHE_NAMESPACE", profile_name);
        // Named profiles are explicit user endpoint choices; accept arbitrary
        // http:// gateways (not just localhost/private-LAN).
        let api_base = normalize_api_base_relaxed(&profile.base_url).ok_or_else(|| {
            anyhow::anyhow!(
                "Provider profile '{}' has invalid base_url '{}'.",
                profile_name,
                profile.base_url
            )
        })?;
        let key = Self::resolve_named_key(profile);
        let key_label = profile
            .api_key_env
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| "inline api_key".to_string());
        let fallback_auth = match profile.auth {
            NamedProviderAuth::None => NamedAnthropicAuth::None {
                label: "local endpoint (no auth)".to_string(),
            },
            NamedProviderAuth::Bearer => match key {
                Some(token) => NamedAnthropicAuth::Bearer {
                    token,
                    label: key_label.clone(),
                },
                None => NamedAnthropicAuth::Missing { label: key_label },
            },
            NamedProviderAuth::Header => match key {
                Some(value) => NamedAnthropicAuth::Header {
                    header_name: HeaderName::from_bytes(
                        profile
                            .auth_header
                            .as_deref()
                            .unwrap_or("api-key")
                            .as_bytes(),
                    )?,
                    value,
                    label: key_label,
                },
                None => NamedAnthropicAuth::Missing { label: key_label },
            },
        };

        let model = profile.default_model.clone().unwrap_or_default();
        let static_models = profile
            .models
            .iter()
            .map(|m| m.id.trim())
            .filter(|id| !id.is_empty())
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let static_context_limits = profile
            .models
            .iter()
            .filter_map(|model| {
                let id = model.id.trim();
                if id.is_empty() {
                    return None;
                }
                model
                    .context_window
                    .map(|limit| (id.to_ascii_lowercase(), limit))
            })
            .collect::<HashMap<_, _>>();

        let reasoning_effort = jcode_base::config::config()
            .provider
            .anthropic_reasoning_effort
            .as_deref()
            .and_then(AnthropicProvider::normalize_reasoning_effort)
            .map(|effort| AnthropicProvider::store_effort_for_model(&model, &effort));
        let max_tokens_override = std::env::var("JCODE_ANTHROPIC_MAX_TOKENS")
            .ok()
            .and_then(|v| v.trim().parse::<u32>().ok());

        // Per-model auth overrides: each `[[providers.<name>.models]]` entry may
        // carry its own `api_key_env` / `auth` / `auth_header` / `env_file` so a
        // gateway serving several vendors (cch: DeepSeek / MiniMax / GLM / Kimi /
        // Xiaomi) uses the right key after `/model` switching.
        let mut per_model_auth: HashMap<String, NamedAnthropicAuth> = HashMap::new();
        for model_entry in &profile.models {
            let id = model_entry.id.trim();
            if id.is_empty()
                || (model_entry.api_key_env.is_none()
                    && model_entry.auth.is_none()
                    && model_entry.auth_header.is_none()
                    && model_entry.env_file.is_none())
            {
                continue;
            }
            let auth_mode = model_entry.auth.unwrap_or(profile.auth);
            let header = model_entry
                .auth_header
                .as_deref()
                .or(profile.auth_header.as_deref());
            let env_file = model_entry
                .env_file
                .as_deref()
                .or(profile.env_file.as_deref());
            let key = Self::resolve_key(
                model_entry.api_key_env.as_deref(),
                profile.api_key.as_deref(),
                env_file,
            );
            let label = model_entry
                .api_key_env
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(ToString::to_string)
                .unwrap_or_else(|| "inline api_key".to_string());
            let model_auth = match auth_mode {
                NamedProviderAuth::None => NamedAnthropicAuth::None {
                    label: "local endpoint (no auth)".to_string(),
                },
                NamedProviderAuth::Bearer => match key {
                    Some(token) => NamedAnthropicAuth::Bearer { token, label },
                    None => NamedAnthropicAuth::Missing { label },
                },
                NamedProviderAuth::Header => match key {
                    Some(value) => NamedAnthropicAuth::Header {
                        header_name: HeaderName::from_bytes(
                            header.unwrap_or("api-key").as_bytes(),
                        )?,
                        value,
                        label,
                    },
                    None => NamedAnthropicAuth::Missing { label },
                },
            };
            per_model_auth.insert(id.to_ascii_lowercase(), model_auth);
        }

        let initial_model_key = model.to_ascii_lowercase();
        Ok(Self {
            client: jcode_provider_core::http_client_with_proxy(profile.proxy.as_deref())?,
            model: Arc::new(std::sync::RwLock::new(model)),
            reasoning_effort: Arc::new(std::sync::RwLock::new(reasoning_effort)),
            api_base,
            auth: Arc::new(std::sync::RwLock::new(
                per_model_auth
                    .get(&initial_model_key)
                    .cloned()
                    .unwrap_or_else(|| fallback_auth.clone()),
            )),
            fallback_auth,
            per_model_auth,
            profile_name: profile_name.to_string(),
            supports_model_catalog: profile.model_catalog,
            static_models,
            static_context_limits,
            models_cache: Arc::new(RwLock::new(Vec::new())),
            max_tokens_override,
            extra_body: profile.extra_body.as_ref().and_then(|value| {
                value.as_object().cloned().or_else(|| {
                    jcode_base::logging::warn(&format!(
                        "Ignoring non-object extra_body for Anthropic profile '{}'",
                        profile_name
                    ));
                    None
                })
            }),
        })
    }

    fn effort_for_model(&self, model: &str) -> Option<String> {
        if !AnthropicProvider::model_supports_reasoning_effort(model) {
            return None;
        }
        Some(
            self.reasoning_effort
                .read()
                .map(|guard| guard.clone())
                .unwrap_or_else(|poisoned| poisoned.into_inner().clone())
                .or_else(|| AnthropicProvider::default_reasoning_effort_for_model(model))
                .unwrap_or_else(|| "none".to_string()),
        )
    }

    fn build_reasoning_request_parts(
        &self,
        model: &str,
    ) -> (Option<ApiThinking>, Option<ApiOutputConfig>, Option<f32>) {
        let show_thinking = jcode_base::config::config().display.show_thinking;
        let effort = self.effort_for_model(model);
        let effort_is_explicit_none = self
            .reasoning_effort
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_else(|poisoned| poisoned.into_inner().clone())
            .as_deref()
            == Some("none");
        let effort = effort.as_deref().filter(|effort| *effort != "none");
        let show_thinking = show_thinking && !effort_is_explicit_none;

        let output_config = effort
            .filter(|_| AnthropicProvider::model_supports_output_effort(model))
            .map(|effort| ApiOutputConfig {
                effort: AnthropicProvider::actual_effort_for_model(model, effort),
            });

        let thinking = if AnthropicProvider::model_supports_adaptive_thinking(model) {
            (effort.is_some() || show_thinking).then_some(ApiThinking::Adaptive {
                display: Some("summarized"),
            })
        } else if AnthropicProvider::model_supports_manual_thinking(model) {
            effort
                .or(show_thinking.then_some("low"))
                .and_then(|effort| {
                    AnthropicProvider::manual_thinking_budget(effort, self.max_tokens_for(model))
                })
                .map(|budget_tokens| ApiThinking::Enabled { budget_tokens })
        } else {
            None
        };

        // Non-OAuth endpoints never force a temperature.
        (thinking, output_config, None)
    }

    fn max_tokens_for(&self, model: &str) -> u32 {
        self.max_tokens_override
            .unwrap_or_else(|| jcode_provider_core::anthropic::anthropic_max_output_tokens(model))
    }

    /// Split a pre-built request so retries can mutate `request["model"]`.
    fn run_stream_with_retries(
        &self,
        token: String,
        request: Value,
        tx: mpsc::Sender<Result<StreamEvent>>,
        model_name: String,
    ) {
        let client = self.client.clone();
        let api_base = self.api_base.clone();
        let per_model_auth = self.per_model_auth.clone();
        let fallback_auth = self.fallback_auth.clone();
        let model_state = Arc::clone(&self.model);
        tokio::spawn(async move {
            let token = token;
            let mut last_error = None;
            let mut next_retry_delay = None;
            let mut request = request;
            let original_model = model_name.clone();
            let mut model_name = model_name;
            let mut tried_models: Vec<String> = vec![original_model.clone()];
            // Per-model auth: recompute per attempt so a catalog-following
            // fallback to another model (cc-switch auto-follow) uses that
            // model's own key/header.
            let auth_for = |model: &str| {
                per_model_auth
                    .get(&model.to_ascii_lowercase())
                    .cloned()
                    .unwrap_or_else(|| fallback_auth.clone())
            };

            for attempt in 0..MAX_RETRIES {
                if attempt > 0 {
                    let delay = jcode_provider_core::retry_after::retry_delay(
                        attempt,
                        RETRY_BASE_DELAY_MS,
                        next_retry_delay.take(),
                    );
                    let _ = tx
                        .send(Ok(StreamEvent::ConnectionPhase {
                            phase: jcode_message_types::ConnectionPhase::Retrying {
                                attempt: attempt + 1,
                                max: MAX_RETRIES,
                            },
                        }))
                        .await;
                    tokio::time::sleep(delay).await;
                    jcode_base::logging::info(&format!(
                        "Retrying Anthropic-format request (attempt {}/{})",
                        attempt + 1,
                        MAX_RETRIES
                    ));
                }

                let (attempt_tx, attempt_guard) =
                    jcode_provider_core::attempt_tracker::track_attempt_output(tx.clone());

                let attempt_client = if attempt == 0 {
                    client.clone()
                } else {
                    jcode_provider_core::fresh_transport_client()
                };

                let url = Self::messages_url(&api_base);
                let auth = auth_for(&model_name);
                match stream_response_named(
                    attempt_client,
                    &url,
                    &auth,
                    token.clone(),
                    request.clone(),
                    attempt_tx,
                    &model_name,
                )
                .await
                {
                    Ok(()) => {
                        let _ = attempt_guard.finish().await;
                        return;
                    }
                    Err(e) => {
                        let saw_output = attempt_guard.finish().await;
                        let error_str = format!("{e:#}").to_lowercase();

                        if is_model_not_found_error(&error_str) && !saw_output {
                            jcode_base::logging::warn(&format!(
                                "Anthropic-format model '{}' is not available ({}); retrying with best available model",
                                model_name, e
                            ));
                            // 本地网关（cc-switch 等）场景：面板切换 provider 后
                            // 模型名会变化，先刷新模型目录、选择目录内未尝试过的
                            // 模型跟随切换；目录不可用（刷新失败）时再回退到内置
                            // Claude 模型列表。
                            let fallback =
                                match fetch_gateway_catalog(&client, &api_base, &auth).await {
                                    Ok(models) => models.into_iter().find(|model| {
                                        !tried_models.iter().any(|tried| tried == model)
                                    }),
                                    Err(_) => anthropic_fallback_model(&tried_models, &error_str),
                                };
                            if let Some(fallback) = fallback {
                                let _ = tx
                                    .send(Ok(StreamEvent::StatusDetail {
                                        detail: format!(
                                            "鈿?'{}' is unavailable; falling back to '{}'",
                                            strip_1m_suffix(&model_name),
                                            strip_1m_suffix(&fallback)
                                        ),
                                    }))
                                    .await;
                                request["model"] =
                                    Value::String(strip_1m_suffix(&fallback).to_string());
                                *model_state
                                    .write()
                                    .unwrap_or_else(|poisoned| poisoned.into_inner()) =
                                    fallback.clone();
                                tried_models.push(fallback.clone());
                                model_name = fallback;
                                last_error = Some(e);
                                continue;
                            }
                        }

                        let has_reasoning_request = request.get("thinking").is_some()
                            || request.get("output_config").is_some();
                        if has_reasoning_request
                            && !saw_output
                            && is_reasoning_unsupported_error(&error_str)
                        {
                            jcode_base::logging::warn(&format!(
                                "Anthropic-format model '{}' rejected the reasoning request ({}); retrying without thinking/effort",
                                model_name, e
                            ));
                            if let Some(obj) = request.as_object_mut() {
                                obj.remove("thinking");
                                obj.remove("output_config");
                            }
                            last_error = Some(e);
                            continue;
                        }

                        if is_retryable_error(&error_str) && attempt + 1 < MAX_RETRIES {
                            if saw_output {
                                let _ = tx
                                    .send(Ok(StreamEvent::RetryRollback {
                                        attempt: attempt + 2,
                                        max: MAX_RETRIES,
                                    }))
                                    .await;
                            }
                            next_retry_delay =
                                jcode_provider_core::retry_after::retry_after_from_error(&e);
                            last_error = Some(e);
                            continue;
                        }

                        let _ = tx.send(Err(e)).await;
                        return;
                    }
                }
            }

            if let Some(e) = last_error {
                let _ = tx
                    .send(Err(anyhow::anyhow!(
                        "Failed after {} retries: {}",
                        MAX_RETRIES,
                        e
                    )))
                    .await;
            }
        });
    }
}

impl Default for NamedAnthropicProvider {
    fn default() -> Self {
        let model = String::new();
        Self {
            client: jcode_provider_core::shared_http_client(),
            model: Arc::new(std::sync::RwLock::new(model.clone())),
            reasoning_effort: Arc::new(std::sync::RwLock::new(None)),
            api_base: "https://api.anthropic.com".to_string(),
            auth: Arc::new(std::sync::RwLock::new(NamedAnthropicAuth::None {
                label: "unconfigured".to_string(),
            })),
            fallback_auth: NamedAnthropicAuth::None {
                label: "unconfigured".to_string(),
            },
            per_model_auth: HashMap::new(),
            profile_name: "anthropic".to_string(),
            supports_model_catalog: false,
            static_models: Vec::new(),
            static_context_limits: HashMap::new(),
            models_cache: Arc::new(RwLock::new(Vec::new())),
            max_tokens_override: None,
            extra_body: None,
        }
    }
}

/// POST an Anthropic Messages request to `url` with named-provider auth and
/// stream the SSE response through `process_sse_event`.
async fn stream_response_named(
    client: Client,
    url: &str,
    auth: &NamedAnthropicAuth,
    token: String,
    request: Value,
    tx: mpsc::Sender<Result<StreamEvent>>,
    model_name: &str,
) -> Result<()> {
    use jcode_message_types::ConnectionPhase;
    let requested_model_base = request
        .get("model")
        .and_then(Value::as_str)
        .map(strip_1m_suffix)
        .unwrap_or("")
        .to_ascii_lowercase();
    let _ = tx
        .send(Ok(StreamEvent::ConnectionPhase {
            phase: ConnectionPhase::SendingRequest,
        }))
        .await;

    let connect_start = std::time::Instant::now();
    let stream_idle_timeout = jcode_base::provider::stream_idle_timeout();

    let mut req = auth
        .apply(
            client
                .post(url)
                .header("anthropic-version", API_VERSION)
                .header("content-type", "application/json")
                .header("accept", "text/event-stream"),
        )
        .await?;

    // Native API-key endpoints authenticate with `x-api-key`; the named
    // `Bearer`/`Header` modes above already set their own auth header, so only
    // add `x-api-key` when auth is None but a token was resolved (defensive).
    if matches!(auth, NamedAnthropicAuth::None { .. }) && !token.is_empty() {
        req = req.header("x-api-key", &token);
    }

    let response = jcode_provider_core::transport::send_with_initial_response_timeout(
        req.json(&request),
        stream_idle_timeout,
    )
    .await
    .with_context(|| {
        format!(
            "Failed to send Anthropic-format request\n  endpoint: {}\n  model: {}\n  auth: {}",
            url,
            model_name,
            auth.label()
        )
    })?;

    let connect_ms = connect_start.elapsed().as_millis();
    jcode_base::logging::info(&format!(
        "HTTP connection established in {}ms (status={})",
        connect_ms,
        response.status()
    ));

    if !response.status().is_success() {
        let status = response.status();
        let retry_after = jcode_provider_core::retry_after::retry_after(response.headers());
        let error_text = jcode_base::util::http_error_body(response, "HTTP error").await;
        return Err(jcode_provider_core::retry_after::error_with_retry_after(
            format!("Anthropic-format API error ({}): {}", status, error_text),
            retry_after,
        ));
    }

    let _ = tx
        .send(Ok(StreamEvent::ConnectionPhase {
            phase: ConnectionPhase::WaitingForResponse,
        }))
        .await;

    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut sse_state = SseStreamState {
        requested_model_base,
        ..SseStreamState::default()
    };

    loop {
        let chunk = match tokio::time::timeout(stream_idle_timeout, stream.next()).await {
            Ok(Some(chunk_result)) => chunk_result.context("Error reading stream chunk")?,
            Ok(None) => break,
            Err(_) => {
                jcode_base::logging::warn(&format!(
                    "Anthropic-format SSE stream timed out (no data for {}s)",
                    stream_idle_timeout.as_secs()
                ));
                anyhow::bail!(
                    "Stream read timeout: no data received for {} seconds",
                    stream_idle_timeout.as_secs()
                );
            }
        };
        let chunk_str = String::from_utf8_lossy(&chunk);
        buffer.push_str(&chunk_str);

        while let Some(event) = parse_sse_event(&mut buffer) {
            let events = process_sse_event(&event, &mut sse_state, false);
            for stream_event in events {
                if let StreamEvent::Error { ref message, .. } = stream_event
                    && is_retryable_error(&message.to_lowercase())
                {
                    anyhow::bail!("Retryable stream error: {}", message);
                }
                if tx.send(Ok(stream_event)).await.is_err() {
                    return Ok(());
                }
            }
        }
    }

    if sse_state.input_tokens.is_some() || sse_state.output_tokens.is_some() {
        let _ = tx
            .send(Ok(StreamEvent::TokenUsage {
                input_tokens: sse_state.input_tokens,
                output_tokens: sse_state.output_tokens,
                cache_read_input_tokens: sse_state.cache_read_input_tokens,
                cache_creation_input_tokens: sse_state.cache_creation_input_tokens,
            }))
            .await;
    }

    Ok(())
}

#[async_trait]
impl Provider for NamedAnthropicProvider {
    async fn complete(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
        system: &str,
        _resume_session_id: Option<&str>,
    ) -> Result<EventStream> {
        // 未配置默认模型（cc-switch 本地网关等）：首次请求前自动发现并
        // 跟随当前 provider 的模型，避免发送空 `model` 字段。
        if self.model().trim().is_empty() {
            self.auto_select_model().await.map_err(|err| {
                anyhow::anyhow!(
                    "No model is configured for provider '{}' and automatic model discovery failed: {err:#}. Set `default_model` in the provider profile or pass `--model <id>`.",
                    self.profile_name
                )
            })?;
        }
        let model = self
            .model
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let api_model = strip_1m_suffix(&model).to_string();

        let api_messages = jcode_provider_anthropic::format_messages(messages, false);
        let api_tools = jcode_provider_anthropic::format_tools(tools, false, is_cache_ttl_1h());
        let (thinking, output_config, temperature) = self.build_reasoning_request_parts(&model);

        let request = ApiRequest {
            model: api_model,
            max_tokens: self.max_tokens_for(&model),
            system: jcode_provider_anthropic::build_system_param(system, false, is_cache_ttl_1h()),
            messages: jcode_provider_anthropic::format_messages_with_identity(
                api_messages,
                false,
                is_cache_ttl_1h(),
            ),
            tools: if api_tools.is_empty() {
                None
            } else {
                Some(api_tools)
            },
            metadata: None,
            thinking,
            output_config,
            temperature,
            service_tier: None,
            stream: true,
        };

        // Serialize to JSON so user-configured extra request-body fields can be
        // merged last (mirroring the OpenAI-compatible profile path), then stream
        // the raw body. `ApiRequest` only derives Serialize, so the merge happens
        // at the JSON-object level rather than re-parsing into the typed struct.
        let mut request_body = serde_json::to_value(&request)
            .ok()
            .and_then(|value| value.as_object().cloned())
            .unwrap_or_default();
        if let Some(extra) = self.extra_body.as_ref() {
            for (key, value) in extra {
                request_body.insert(key.clone(), value.clone());
            }
        }

        log_anthropic_canonical_input(&model, "anthropic_messages", &request, false, false);

        let (tx, rx) = mpsc::channel::<Result<StreamEvent>>(100);
        let auth = self.auth();
        let token = match &auth {
            NamedAnthropicAuth::Bearer { token, .. } => token.clone(),
            NamedAnthropicAuth::Header { value, .. } => value.clone(),
            NamedAnthropicAuth::None { .. } | NamedAnthropicAuth::Missing { .. } => String::new(),
        };
        self.run_stream_with_retries(token, Value::Object(request_body), tx, model.clone());

        Ok(Box::pin(ReceiverStream::new(rx)))
    }

    fn name(&self) -> &str {
        "anthropic"
    }

    fn display_name(&self) -> String {
        self.runtime_display_name()
    }

    fn runtime_display_name(&self) -> String {
        self.profile_name.clone()
    }

    fn direct_openai_compatible_route_parts(&self) -> Option<(String, String, String)> {
        Some((
            self.profile_name.clone(),
            format!("openai-compatible:{}", self.profile_name),
            self.api_base.clone(),
        ))
    }

    fn model(&self) -> String {
        self.model
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn set_model(&self, model: &str) -> Result<()> {
        let model = model.trim();
        if model.is_empty() {
            anyhow::bail!("Model cannot be empty");
        }
        *self
            .model
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = model.to_string();
        // Per-model auth (api_key_env / auth / auth_header) follows the active
        // model so switching between vendors on one gateway uses the right key.
        let next_auth = self
            .per_model_auth
            .get(&model.to_ascii_lowercase())
            .cloned()
            .unwrap_or_else(|| self.fallback_auth.clone());
        *self
            .auth
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = next_auth;
        Ok(())
    }

    fn available_models(&self) -> Vec<&'static str> {
        AVAILABLE_MODELS.to_vec()
    }

    fn available_models_display(&self) -> Vec<String> {
        // 动态目录缓存优先：cc-switch 本地网关自动发现 / `model_catalog`
        // 刷新后，模型列表跟随当前 provider 变化，`/model` 切换可用。
        let mut models = self
            .models_cache
            .try_read()
            .map(|cache| cache.clone())
            .unwrap_or_default();
        if !models.is_empty() {
            for model in &self.static_models {
                if !models.contains(model) {
                    models.push(model.clone());
                }
            }
            if models.is_empty() {
                models.push(self.model());
            }
            return models;
        }
        if !self.supports_model_catalog {
            if !self.static_models.is_empty() {
                return self.static_models.clone();
            }
            return AVAILABLE_MODELS.iter().map(|m| (*m).to_string()).collect();
        }

        let mut models = self
            .models_cache
            .try_read()
            .map(|cache| cache.clone())
            .unwrap_or_default();
        for model in &self.static_models {
            if !models.contains(model) {
                models.push(model.clone());
            }
        }
        if models.is_empty() {
            models.push(self.model());
        }
        models
    }

    fn available_models_for_switching(&self) -> Vec<String> {
        self.available_models_display()
    }

    fn model_routes(&self) -> Vec<jcode_provider_core::ModelRoute> {
        use jcode_provider_core::ModelRoute;
        let provider_configured = !matches!(self.fallback_auth, NamedAnthropicAuth::Missing { .. });
        let mut seen = std::collections::HashSet::new();
        let mut routes = Vec::new();
        for model in self.available_models_display() {
            if !seen.insert(model.clone()) {
                continue;
            }
            let available = provider_configured
                && !matches!(
                    self.per_model_auth.get(&model.to_ascii_lowercase()),
                    Some(NamedAnthropicAuth::Missing { .. })
                );
            routes.push(ModelRoute {
                model,
                provider: self.profile_name.clone(),
                api_method: format!("openai-compatible:{}", self.profile_name),
                available,
                detail: self.api_base.clone(),
                cheapness: None,
                capability: None,
            });
        }
        routes
    }

    async fn prefetch_models(&self) -> Result<()> {
        // 未指定默认模型（cc-switch 本地网关等）：自动发现并选择当前
        // provider 的模型，跟随面板切换。其余场景仅 `model_catalog` 开启
        // 时刷新目录缓存，失败不阻塞启动。
        let needs_auto_model = self.model().trim().is_empty();
        if !self.supports_model_catalog && !needs_auto_model {
            return Ok(());
        }
        match self.fetch_models().await {
            Ok(models) => {
                if needs_auto_model && let Some(first) = models.first() {
                    let _ = self.set_model(first);
                }
            }
            Err(err) => {
                if needs_auto_model {
                    jcode_base::logging::warn(&format!(
                        "Automatic model discovery for '{}' failed: {err:#}",
                        self.profile_name
                    ));
                }
            }
        }
        Ok(())
    }

    fn context_window(&self) -> usize {
        let model = self.model();
        self.static_context_limits
            .get(&model.to_ascii_lowercase())
            .copied()
            .unwrap_or_else(|| {
                jcode_provider_core::context_limit_for_model_with_provider(
                    &model,
                    Some(self.name()),
                )
                .unwrap_or(jcode_provider_core::DEFAULT_CONTEXT_LIMIT)
            })
    }

    fn supports_image_input(&self) -> bool {
        true
    }

    fn supports_compaction(&self) -> bool {
        true
    }

    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(Self {
            client: self.client.clone(),
            model: Arc::new(std::sync::RwLock::new(self.model())),
            reasoning_effort: Arc::new(std::sync::RwLock::new(
                self.reasoning_effort
                    .read()
                    .map(|guard| guard.clone())
                    .unwrap_or_else(|poisoned| poisoned.into_inner().clone()),
            )),
            api_base: self.api_base.clone(),
            auth: Arc::clone(&self.auth),
            fallback_auth: self.fallback_auth.clone(),
            per_model_auth: self.per_model_auth.clone(),
            profile_name: self.profile_name.clone(),
            supports_model_catalog: self.supports_model_catalog,
            static_models: self.static_models.clone(),
            static_context_limits: self.static_context_limits.clone(),
            models_cache: Arc::clone(&self.models_cache),
            max_tokens_override: self.max_tokens_override,
            extra_body: self.extra_body.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jcode_base::config::{
        NamedProviderAuth, NamedProviderConfig, NamedProviderModelConfig, ProviderApiFormat,
    };

    /// One-shot `/models` mock: serves `body` to the first request.
    fn spawn_models_server(body: &'static str) -> String {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake models server");
        let addr = listener.local_addr().expect("fake models addr");
        std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
            let mut buf = vec![0u8; 8192];
            let _ = stream.read(&mut buf);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
        });
        format!("http://{addr}")
    }

    fn cc_switch_profile(base_url: String) -> NamedProviderConfig {
        NamedProviderConfig {
            base_url,
            api_format: Some(ProviderApiFormat::Anthropic),
            auth: NamedProviderAuth::None,
            default_model: None,
            ..NamedProviderConfig::default()
        }
    }

    /// 统一 `<jcode home>/.env` 是密钥的权威来源：未配置 `env_file` 时，
    /// named Anthropic provider 也必须从统一 .env 解析 `api_key_env` 指向
    /// 的 key，而不是只看进程环境变量（resonix 对齐，回归测试）。
    #[test]
    fn named_profile_key_resolves_from_unified_env_without_env_file() {
        let _guard = jcode_base::storage::lock_test_env();
        let temp = tempfile::tempdir().expect("tempdir");
        // 备份并隔离 JCODE_HOME 与测试 key 的进程环境变量。
        let saved_home = std::env::var_os("JCODE_HOME");
        let saved_key = std::env::var_os("JCODE_NAMED_ANTH_KEY_TEST");
        jcode_base::env::set_var("JCODE_HOME", temp.path());
        std::fs::write(
            temp.path().join(".env"),
            "JCODE_NAMED_ANTH_KEY_TEST=sk-from-unified-env\n",
        )
        .expect("write unified .env");

        let profile = NamedProviderConfig {
            base_url: "https://gateway.example.com".to_string(),
            api_format: Some(ProviderApiFormat::Anthropic),
            auth: NamedProviderAuth::Bearer,
            api_key_env: Some("JCODE_NAMED_ANTH_KEY_TEST".to_string()),
            // env_file 故意留空：必须走统一 .env 而非仅进程环境。
            env_file: None,
            default_model: Some("deepseek-v4-flash".to_string()),
            ..NamedProviderConfig::default()
        };
        let provider = NamedAnthropicProvider::new_named("test-gw", &profile)
            .expect("named anthropic provider should construct");
        match provider.auth() {
            NamedAnthropicAuth::Bearer { token, .. } => {
                assert_eq!(token, "sk-from-unified-env");
            }
            other => panic!("expected Bearer auth resolved from unified .env, got: {other:?}"),
        }

        if let Some(home) = saved_home {
            jcode_base::env::set_var("JCODE_HOME", home);
        } else {
            jcode_base::env::remove_var("JCODE_HOME");
        }
        match saved_key {
            Some(key) => jcode_base::env::set_var("JCODE_NAMED_ANTH_KEY_TEST", key),
            None => jcode_base::env::remove_var("JCODE_NAMED_ANTH_KEY_TEST"),
        }
    }

    #[test]
    fn messages_url_derivation() {
        assert_eq!(
            NamedAnthropicProvider::messages_url("https://host"),
            "https://host/v1/messages"
        );
        assert_eq!(
            NamedAnthropicProvider::messages_url("https://host/v1"),
            "https://host/v1/messages"
        );
        assert_eq!(
            NamedAnthropicProvider::messages_url("https://host/v1/"),
            "https://host/v1/messages"
        );
        assert_eq!(
            NamedAnthropicProvider::messages_url("https://host/v1/messages"),
            "https://host/v1/messages"
        );
        assert_eq!(
            NamedAnthropicProvider::messages_url("https://host/anthropic"),
            "https://host/anthropic/v1/messages"
        );
    }

    #[tokio::test]
    async fn fetch_models_parses_cc_switch_gateway_format() {
        let server = spawn_models_server(
            r#"{"models":[{"slug":"deepseek-v4-flash","display_name":"DeepSeek V4 Flash"},{"slug":"deepseek-v4-pro","display_name":"DeepSeek V4 Pro"}]}"#,
        );
        let provider =
            NamedAnthropicProvider::new_named("cc-switch", &cc_switch_profile(server)).unwrap();
        let models = provider.fetch_models().await.unwrap();
        assert_eq!(models, vec!["deepseek-v4-flash", "deepseek-v4-pro"]);
    }

    #[tokio::test]
    async fn fetch_models_parses_anthropic_standard_format() {
        let server = spawn_models_server(
            r#"{"data":[{"id":"claude-sonnet-4-6"},{"id":"claude-opus-4-6"}]}"#,
        );
        let provider =
            NamedAnthropicProvider::new_named("cc-switch", &cc_switch_profile(server)).unwrap();
        let models = provider.fetch_models().await.unwrap();
        assert_eq!(models, vec!["claude-sonnet-4-6", "claude-opus-4-6"]);
    }

    #[tokio::test]
    async fn auto_select_model_picks_first_discovered_model() {
        let server = spawn_models_server(
            r#"{"models":[{"slug":"deepseek-v4-flash"},{"slug":"deepseek-v4-pro"}]}"#,
        );
        let provider =
            NamedAnthropicProvider::new_named("cc-switch", &cc_switch_profile(server)).unwrap();
        assert!(provider.model().is_empty());
        assert!(provider.auto_select_model().await.unwrap());
        assert_eq!(provider.model(), "deepseek-v4-flash");
    }

    #[tokio::test]
    async fn auto_select_model_never_overrides_explicit_default() {
        let server = spawn_models_server(r#"{"models":[{"slug":"deepseek-v4-flash"}]}"#);
        let mut profile = cc_switch_profile(server);
        profile.default_model = Some("explicit-model".to_string());
        let provider = NamedAnthropicProvider::new_named("cc-switch", &profile).unwrap();
        assert_eq!(provider.model(), "explicit-model");
        assert!(!provider.auto_select_model().await.unwrap());
        assert_eq!(provider.model(), "explicit-model");
    }

    #[tokio::test]
    async fn prefetch_models_auto_selects_model_for_gateway() {
        let server = spawn_models_server(
            r#"{"models":[{"slug":"deepseek-v4-flash"},{"slug":"deepseek-v4-pro"}]}"#,
        );
        // cc-switch 场景即使未开 model_catalog 也会自动选择。
        let provider =
            NamedAnthropicProvider::new_named("cc-switch", &cc_switch_profile(server)).unwrap();
        provider.prefetch_models().await.unwrap();
        assert_eq!(provider.model(), "deepseek-v4-flash");
        let models = provider.available_models_display();
        assert_eq!(models, vec!["deepseek-v4-flash", "deepseek-v4-pro"]);
    }

    #[tokio::test]
    async fn prefetch_models_with_explicit_model_keeps_model_catalog_behavior() {
        let server =
            spawn_models_server(r#"{"models":[{"slug":"gateway-a"},{"slug":"gateway-b"}]}"#);
        let mut profile = cc_switch_profile(server);
        profile.model_catalog = true;
        profile.default_model = Some("gateway-a".to_string());
        let provider = NamedAnthropicProvider::new_named("cc-switch", &profile).unwrap();
        provider.prefetch_models().await.unwrap();
        // 显式 default_model 不被覆盖；目录缓存填充供 /model 切换。
        assert_eq!(provider.model(), "gateway-a");
        let models = provider.available_models_display();
        assert_eq!(models, vec!["gateway-a", "gateway-b"]);
    }

    #[tokio::test]
    async fn available_models_prefers_dynamic_catalog_after_discovery() {
        let server = spawn_models_server(
            r#"{"models":[{"slug":"deepseek-v4-flash"},{"slug":"deepseek-v4-pro"}]}"#,
        );
        let provider =
            NamedAnthropicProvider::new_named("cc-switch", &cc_switch_profile(server)).unwrap();
        // 目录未获取前：无 static models，回退内置 Claude 列表。
        let before = provider.available_models_display();
        assert!(
            before.iter().any(|m| m.starts_with("claude-")),
            "fallback should be built-in Claude models: {before:?}"
        );
        // 自动发现后：目录模型优先。
        provider.prefetch_models().await.unwrap();
        let after = provider.available_models_display();
        assert_eq!(after, vec!["deepseek-v4-flash", "deepseek-v4-pro"]);
    }

    #[test]
    fn per_model_api_key_env_switches_auth_on_set_model() {
        // cch ???provider ? auth=header/x-api-key + DEEPSEEK_API_KEY?
        // ? MiniMax-M3 ??????? api_key_env????????? key?
        let _guard = jcode_base::storage::lock_test_env();
        let temp = tempfile::tempdir().expect("tempdir");
        let saved_home = std::env::var_os("JCODE_HOME");
        let saved_deepseek = std::env::var_os("CCH_KEY_DEEPSEEK");
        let saved_minimax = std::env::var_os("CCH_KEY_MINIMAX");
        jcode_base::env::set_var("JCODE_HOME", temp.path());
        std::fs::write(
            temp.path().join(".env"),
            "CCH_KEY_DEEPSEEK=sk-deepseek
CCH_KEY_MINIMAX=sk-minimax
",
        )
        .expect("write unified .env");

        let profile = NamedProviderConfig {
            base_url: "http://cch.skytech.io".to_string(),
            api_format: Some(ProviderApiFormat::Anthropic),
            auth: NamedProviderAuth::Header,
            auth_header: Some("x-api-key".to_string()),
            api_key_env: Some("CCH_KEY_DEEPSEEK".to_string()),
            default_model: Some("deepseek-v4-flash".to_string()),
            models: vec![
                NamedProviderModelConfig {
                    id: "deepseek-v4-flash".to_string(),
                    ..NamedProviderModelConfig::default()
                },
                NamedProviderModelConfig {
                    id: "MiniMax-M3".to_string(),
                    api_key_env: Some("CCH_KEY_MINIMAX".to_string()),
                    ..NamedProviderModelConfig::default()
                },
            ],
            ..NamedProviderConfig::default()
        };
        let provider = NamedAnthropicProvider::new_named("cch", &profile).expect("construct");

        match provider.auth() {
            NamedAnthropicAuth::Header {
                value,
                header_name,
                label,
            } => {
                assert_eq!(value, "sk-deepseek");
                assert_eq!(header_name.as_str(), "x-api-key");
                assert_eq!(label, "CCH_KEY_DEEPSEEK");
            }
            other => panic!("expected provider-level Header auth, got: {other:?}"),
        }

        provider.set_model("MiniMax-M3").expect("switch to MiniMax");
        match provider.auth() {
            NamedAnthropicAuth::Header { value, label, .. } => {
                assert_eq!(value, "sk-minimax", "per-model key must follow the switch");
                assert_eq!(label, "CCH_KEY_MINIMAX");
            }
            other => panic!("expected per-model Header auth, got: {other:?}"),
        }

        // ????????????? provider ? key?
        provider
            .set_model("deepseek-v4-flash")
            .expect("switch back");
        match provider.auth() {
            NamedAnthropicAuth::Header { value, .. } => assert_eq!(value, "sk-deepseek"),
            other => panic!("expected fallback Header auth, got: {other:?}"),
        }

        if let Some(home) = saved_home {
            jcode_base::env::set_var("JCODE_HOME", home);
        } else {
            jcode_base::env::remove_var("JCODE_HOME");
        }
        for (key, value) in [
            ("CCH_KEY_DEEPSEEK", saved_deepseek),
            ("CCH_KEY_MINIMAX", saved_minimax),
        ] {
            match value {
                Some(value) => jcode_base::env::set_var(key, value),
                None => jcode_base::env::remove_var(key),
            }
        }
    }
}
