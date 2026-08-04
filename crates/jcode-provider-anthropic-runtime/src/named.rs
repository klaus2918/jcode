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

/// Anthropic Messages API runtime for a user-defined named provider profile.
pub struct NamedAnthropicProvider {
    client: Client,
    model: Arc<std::sync::RwLock<String>>,
    reasoning_effort: Arc<std::sync::RwLock<Option<String>>>,
    /// Normalized base URL (e.g. `https://gateway.example.com/v1`). The
    /// Messages endpoint is derived as `{base}/v1/messages` (or used verbatim
    /// when `base` already ends in `/messages`).
    api_base: String,
    auth: NamedAnthropicAuth,
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

    async fn fetch_models(&self) -> Result<Vec<String>> {
        let url = jcode_base::provider::anthropic::models_url_from_api_base(&self.api_base);
        let response = self
            .auth
            .apply(
                self.client
                    .get(&url)
                    .header("anthropic-version", "2023-06-01"),
            )
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
                self.auth.label(),
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
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        if models.is_empty() {
            anyhow::bail!(
                "Anthropic-compatible model catalog response did not contain any model ids"
            );
        }
        if let Ok(mut cache) = self.models_cache.try_write() {
            *cache = models.clone();
        }
        Ok(models)
    }

    fn resolve_named_key(profile: &NamedProviderConfig) -> Option<String> {
        let env_key = profile
            .api_key_env
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty());
        if let Some(env_key) = env_key {
            return match profile
                .env_file
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
            {
                Some(env_file) => {
                    jcode_base::provider_catalog::load_api_key_from_env_or_config(env_key, env_file)
                }
                // No env file configured: read the environment variable
                // directly. Passing an empty file name to
                // `load_api_key_from_env_or_config` fails its safe-name
                // validation and would lose the key entirely.
                None => std::env::var(env_key)
                    .ok()
                    .map(|key| key.trim().to_string())
                    .filter(|key| !key.is_empty()),
            };
        }
        profile.api_key.clone()
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
        let auth = match profile.auth {
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

        let model = profile
            .default_model
            .clone()
            .unwrap_or_default();
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

        Ok(Self {
            client: jcode_provider_core::http_client_with_proxy(profile.proxy.as_deref())?,
            model: Arc::new(std::sync::RwLock::new(model)),
            reasoning_effort: Arc::new(std::sync::RwLock::new(reasoning_effort)),
            api_base,
            auth,
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
        let auth = self.auth.clone();
        let model_state = Arc::clone(&self.model);
        tokio::spawn(async move {
            let token = token;
            let mut last_error = None;
            let mut next_retry_delay = None;
            let mut request = request;
            let original_model = model_name.clone();
            let mut model_name = model_name;
            let mut tried_models: Vec<String> = vec![original_model.clone()];

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
                            let fallback = anthropic_fallback_model(&tried_models, &error_str);
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
            auth: NamedAnthropicAuth::None {
                label: "unconfigured".to_string(),
            },
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
        let token = match &self.auth {
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
        Ok(())
    }

    fn available_models(&self) -> Vec<&'static str> {
        AVAILABLE_MODELS.to_vec()
    }

    fn available_models_display(&self) -> Vec<String> {
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

    async fn prefetch_models(&self) -> Result<()> {
        if !self.supports_model_catalog {
            return Ok(());
        }
        let _ = self.fetch_models().await;
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
            auth: self.auth.clone(),
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
}
