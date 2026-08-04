//! Configuration file support for jcode
//!
//! Config is loaded from `~/.jcode/config.toml` (or `$JCODE_HOME/config.toml`)
//! Environment variables override config file settings.

pub use jcode_config_types::{
    AgentsConfig, AmbientConfig, AuthConfig, AutoJudgeConfig, AutoReviewConfig, CompactionConfig,
    CompactionMode, CrossProviderFailoverMode, DiagramDisplayMode, DiagramPanePosition,
    DiffDisplayMode, DisplayConfig, FeatureConfig, GatewayConfig, HooksConfig, KeybindingsConfig,
    LatexRenderingMode, MarkdownSpacingMode, NamedProviderAuth, NamedProviderConfig,
    NamedProviderModelConfig, NamedProviderType, NativeScrollbarConfig, NetworkConfig,
    NotificationsConfig, OverscrollStatusMode, PowerConfig, ProviderApiFormat, ProviderConfig,
    ReasoningDisplayMode, SafetyConfig, SessionPickerResumeAction, SponsorsConfig, SwarmSpawnMode,
    SwarmStripLayout, TerminalConfig, UpdateChannel, WebSearchConfig, WebSearchEngine,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::{LazyLock, RwLock};

/// Deserialize the `providers` config section as either the jcode-style
/// `[providers.<name>]` mapping table or a resonix-style top-level
/// `[[providers]]` array table (`name`/`kind`/`base_url`/`model`/`models`/
/// `api_key_env`). The array form is converted into the equivalent mapping so
/// every downstream consumer sees the same `BTreeMap`.
fn deserialize_providers<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<String, NamedProviderConfig>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum ProvidersField {
        Map(BTreeMap<String, NamedProviderConfig>),
        Array(Vec<NamedProviderArrayEntry>),
    }
    match ProvidersField::deserialize(deserializer)? {
        ProvidersField::Map(map) => Ok(map),
        ProvidersField::Array(entries) => {
            let mut map = BTreeMap::new();
            for entry in entries {
                map.insert(entry.name.clone(), entry.into_config());
            }
            Ok(map)
        }
    }
}

/// Serialize the `providers` config section back out as a resonix-style
/// top-level `[[providers]]` array of tables, preserving the array style on
/// every `Config::save()` round-trip (matching DeepSeek-Reasonix).
///
/// The `toml` crate cannot derive `[[providers]]` array-of-tables output from a
/// plain `Vec<Struct>` (it errors `UnsupportedType`), so the section is built
/// explicitly as `toml::Value` trees and emitted through the value serializer.
/// Ordering follows the `BTreeMap` key order (deterministic, alphabetical).
///
/// Round-trip rules:
/// - Every `NamedProviderConfig` field is preserved (see `provider_to_value`).
/// - `context_window` is hoisted provider-wide when all declared models share
///   one value, then dropped per-model; otherwise it stays per-model.
/// - Models that carry only an `id` render as a clean `models = ["a", "b"]`
///   string array; models with extra fields render as `[[providers.models]]`
///   tables. Both forms are accepted by `deserialize_array_models`.
fn serialize_providers<S>(
    providers: &BTreeMap<String, NamedProviderConfig>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let entries = providers
        .iter()
        .map(|(name, config)| provider_to_value(name, config))
        .collect::<Vec<_>>();
    toml::Value::Array(entries).serialize(serializer)
}

/// Convert one named provider into a `[[providers]]` array-entry table.
fn provider_to_value(name: &str, config: &NamedProviderConfig) -> toml::Value {
    let mut table = toml::map::Map::new();

    table.insert("name".to_string(), toml::Value::String(name.to_string()));

    let provider_type = match config.provider_type {
        NamedProviderType::OpenAiCompatible => "openai-compatible",
        NamedProviderType::OpenRouter => "open-router",
    };
    table.insert("type".to_string(), toml::Value::String(provider_type.to_string()));

    // `api_format` renders as the resonix `kind` selector ("openai"/"anthropic").
    // `OpenAiCompatible` is the default so it is omitted unless set differently.
    if let Some(format) = config.api_format {
        let kind = match format {
            ProviderApiFormat::OpenAiCompatible => "openai",
            ProviderApiFormat::Anthropic => "anthropic",
        };
        table.insert("kind".to_string(), toml::Value::String(kind.to_string()));
    }

    table.insert(
        "base_url".to_string(),
        toml::Value::String(config.base_url.clone()),
    );

    if let Some(model) = config.default_model.as_deref().filter(|m| !m.is_empty()) {
        table.insert("model".to_string(), toml::Value::String(model.to_string()));
    }

    if let Some(key_env) = config.api_key_env.as_deref().filter(|k| !k.is_empty()) {
        table.insert("api_key_env".to_string(), toml::Value::String(key_env.to_string()));
    }

    if let Some(proxy) = config.proxy.as_deref().filter(|p| !p.is_empty()) {
        table.insert("proxy".to_string(), toml::Value::String(proxy.to_string()));
    }

    let auth = match config.auth {
        NamedProviderAuth::Bearer => "bearer",
        NamedProviderAuth::Header => "header",
        NamedProviderAuth::None => "none",
    };
    table.insert("auth".to_string(), toml::Value::String(auth.to_string()));

    if let Some(header) = config.auth_header.as_deref().filter(|h| !h.is_empty()) {
        table.insert("auth_header".to_string(), toml::Value::String(header.to_string()));
    }

    if let Some(env_file) = config.env_file.as_deref().filter(|f| !f.is_empty()) {
        table.insert("env_file".to_string(), toml::Value::String(env_file.to_string()));
    }

    // Inline API keys are deprecated but must not be lost on round-trip.
    if let Some(key) = config.api_key.as_deref().filter(|k| !k.is_empty()) {
        table.insert("api_key".to_string(), toml::Value::String(key.to_string()));
    }

    if let Some(requires) = config.requires_api_key {
        table.insert("requires_api_key".to_string(), toml::Value::Boolean(requires));
    }

    if config.provider_routing {
        table.insert("provider_routing".to_string(), toml::Value::Boolean(true));
    }
    if config.model_catalog {
        table.insert("model_catalog".to_string(), toml::Value::Boolean(true));
    }
    if config.allow_provider_pinning {
        table.insert("allow_provider_pinning".to_string(), toml::Value::Boolean(true));
    }

    if let Some(extra_body) = config.extra_body.as_ref() {
        match toml::Value::try_from(extra_body.clone()) {
            Ok(value) => {
                table.insert("extra_body".to_string(), value);
            }
            Err(_) => {
                // Non-TOML-representable JSON value; serialize as a JSON string
                // so the data survives. `extra_body` re-parses either way.
                table.insert(
                    "extra_body".to_string(),
                    toml::Value::String(extra_body.to_string()),
                );
            }
        }
    }

    if let Some(supports) = config.supports_reasoning_effort {
        table.insert(
            "supports_reasoning_effort".to_string(),
            toml::Value::Boolean(supports),
        );
    }

    // Provider-wide context window: hoist when every model shares the same
    // non-`None` value; otherwise keep windows on the model tables.
    let shared_context = models_shared_context(&config.models);
    if let Some(window) = shared_context {
        table.insert(
            "context_window".to_string(),
            toml::Value::Integer(window as i64),
        );
    }

    table.insert("models".to_string(), models_to_value(&config.models, shared_context));

    toml::Value::Table(table)
}

/// The single `context_window` all models share, if any.
fn models_shared_context(models: &[NamedProviderModelConfig]) -> Option<usize> {
    let mut shared: Option<usize> = None;
    for model in models {
        match model.context_window {
            Some(window) => match shared {
                None => shared = Some(window),
                Some(existing) if existing != window => return None,
                _ => {}
            },
            None => return None,
        }
    }
    shared
}

/// Render the `models` list: a clean `["a", "b"]` string array when every model
/// carries only its `id`, otherwise `[[providers.models]]` tables.
fn models_to_value(
    models: &[NamedProviderModelConfig],
    shared_context: Option<usize>,
) -> toml::Value {
    let all_simple = models
        .iter()
        .all(|model| model_is_simple(model, shared_context));

    if all_simple {
        let ids = models
            .iter()
            .map(|model| toml::Value::String(model.id.clone()))
            .collect::<Vec<_>>();
        return toml::Value::Array(ids);
    }

    let tables = models
        .iter()
        .map(|model| {
            let mut table = toml::map::Map::new();
            table.insert("id".to_string(), toml::Value::String(model.id.clone()));
            if let Some(window) = model.context_window {
                if shared_context != Some(window) {
                    table.insert(
                        "context_window".to_string(),
                        toml::Value::Integer(window as i64),
                    );
                }
            }
            if !model.input.is_empty() {
                let input = model
                    .input
                    .iter()
                    .map(|v| toml::Value::String(v.clone()))
                    .collect::<Vec<_>>();
                table.insert("input".to_string(), toml::Value::Array(input));
            }
            insert_opt_bool(&mut table, "vision", model.vision);
            insert_opt_bool(&mut table, "tools", model.tools);
            insert_opt_str(&mut table, "reasoning_protocol", model.reasoning_protocol.as_deref());
            if let Some(efforts) = model.supported_efforts.as_ref() {
                let values = efforts
                    .iter()
                    .map(|v| toml::Value::String(v.clone()))
                    .collect::<Vec<_>>();
                table.insert("supported_efforts".to_string(), toml::Value::Array(values));
            }
            insert_opt_str(&mut table, "default_effort", model.default_effort.as_deref());
            insert_opt_usize(&mut table, "output_window", model.output_window);
            insert_opt_bool(&mut table, "temperature_supported", model.temperature_supported);
            insert_opt_bool(&mut table, "fixed_sampling", model.fixed_sampling);
            insert_opt_str(
                &mut table,
                "output_limit_field",
                model.output_limit_field.as_deref(),
            );
            toml::Value::Table(table)
        })
        .collect::<Vec<_>>();

    toml::Value::Array(tables)
}

/// A model is "simple" (renders as a bare id string) when it carries nothing
/// beyond `id` and, at most, the context window that was hoisted provider-wide.
fn model_is_simple(model: &NamedProviderModelConfig, shared_context: Option<usize>) -> bool {
    if !model.input.is_empty()
        || model.vision.is_some()
        || model.tools.is_some()
        || model.reasoning_protocol.is_some()
        || model.supported_efforts.is_some()
        || model.default_effort.is_some()
        || model.output_window.is_some()
        || model.temperature_supported.is_some()
        || model.fixed_sampling.is_some()
        || model.output_limit_field.is_some()
    {
        return false;
    }
    match model.context_window {
        // A model window identical to the hoisted provider-wide window is
        // implied and safe to drop.
        Some(window) => shared_context == Some(window),
        None => true,
    }
}

fn insert_opt_bool(table: &mut toml::map::Map<String, toml::Value>, key: &str, value: Option<bool>) {
    if let Some(value) = value {
        table.insert(key.to_string(), toml::Value::Boolean(value));
    }
}

fn insert_opt_str(table: &mut toml::map::Map<String, toml::Value>, key: &str, value: Option<&str>) {
    if let Some(value) = value.filter(|v| !v.is_empty()) {
        table.insert(key.to_string(), toml::Value::String(value.to_string()));
    }
}

fn insert_opt_usize(table: &mut toml::map::Map<String, toml::Value>, key: &str, value: Option<usize>) {
    if let Some(value) = value {
        table.insert(key.to_string(), toml::Value::Integer(value as i64));
    }
}

/// One entry of a resonix-style top-level `[[providers]]` array table.
#[derive(Deserialize)]
struct NamedProviderArrayEntry {
    name: String,
    #[serde(rename = "type", default)]
    provider_type: NamedProviderType,
    base_url: String,
    /// `kind` is the resonix protocol selector; it maps onto `api_format`
    /// ("openai" / "anthropic" values are already aliased there).
    #[serde(
        default,
        alias = "kind",
        alias = "api",
        alias = "api-format",
        alias = "api_format",
        alias = "format"
    )]
    api_format: Option<ProviderApiFormat>,
    #[serde(default, alias = "model")]
    default_model: Option<String>,
    #[serde(default, deserialize_with = "deserialize_array_models")]
    models: Vec<NamedProviderModelConfig>,
    #[serde(default)]
    api_key_env: Option<String>,
    /// Provider-wide fallback context window applied to every declared model
    /// that does not set its own.
    #[serde(default, alias = "context-window", alias = "context_window")]
    context_window: Option<usize>,
}

impl NamedProviderArrayEntry {
    fn into_config(self) -> NamedProviderConfig {
        let mut models = self.models;
        if models.is_empty()
            && let Some(model) = &self.default_model
        {
            models.push(NamedProviderModelConfig {
                id: model.clone(),
                ..NamedProviderModelConfig::default()
            });
        }
        if let Some(context_window) = self.context_window {
            for model in &mut models {
                if model.context_window.is_none() {
                    model.context_window = Some(context_window);
                }
            }
        }
        NamedProviderConfig {
            provider_type: self.provider_type,
            base_url: self.base_url,
            api_format: self.api_format,
            default_model: self.default_model,
            api_key_env: self.api_key_env,
            models,
            ..NamedProviderConfig::default()
        }
    }
}

/// String-array form of `models = ["a", "b"]` for `[[providers]]` entries,
/// alongside the normal array-of-table form.
fn deserialize_array_models<'de, D>(
    deserializer: D,
) -> Result<Vec<NamedProviderModelConfig>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Models {
        Table(Vec<NamedProviderModelConfig>),
        Strings(Vec<String>),
    }
    match Models::deserialize(deserializer)? {
        Models::Table(models) => Ok(models),
        Models::Strings(ids) => Ok(ids
            .into_iter()
            .map(|id| NamedProviderModelConfig {
                id,
                ..NamedProviderModelConfig::default()
            })
            .collect()),
    }
}
use std::time::{Duration, Instant, SystemTime};

const CONFIG_CACHE_CHECK_INTERVAL: Duration = if cfg!(test) {
    Duration::ZERO
} else {
    Duration::from_millis(500)
};

const CONFIG_ENV_KEYS: &[&str] = &[
    "HOME",
    "JCODE_ACP_PROFILE",
    "JCODE_ACP_TOOL_PROFILE",
    "JCODE_ACTIVE_SESSIONS_MANAGER",
    "JCODE_AMBIENT_ENABLED",
    "JCODE_AMBIENT_MAX_INTERVAL",
    "JCODE_AMBIENT_MIN_INTERVAL",
    "JCODE_AMBIENT_MODEL",
    "JCODE_AMBIENT_PROACTIVE",
    "JCODE_AMBIENT_PROVIDER",
    "JCODE_AMBIENT_VISIBLE",
    "JCODE_ANIMATION_FPS",
    "JCODE_AUTO_POKE",
    "JCODE_AUTOJUDGE_ENABLED",
    "JCODE_AUTOJUDGE_MODEL",
    "JCODE_AUTOREVIEW_ENABLED",
    "JCODE_AUTOREVIEW_MODEL",
    "JCODE_AUTO_POKE",
    "JCODE_AUTO_SERVER_RELOAD",
    "JCODE_BING_API_KEY",
    "JCODE_BING_API_KEY_ENV",
    "JCODE_BING_MARKET",
    "JCODE_CENTERED_TOGGLE_KEY",
    "JCODE_CHAT_NATIVE_SCROLLBAR",
    "JCODE_COMPACT_NOTIFICATIONS",
    "JCODE_COPY_BADGE_ALT_LABEL",
    "JCODE_COPY_SELECTION_TOGGLE_KEY",
    "JCODE_COPILOT_PREMIUM",
    "JCODE_CROSS_PROVIDER_FAILOVER",
    "JCODE_DEBUG_SOCKET",
    "JCODE_DEFAULT_REASONING_DISPLAY",
    "JCODE_DICTATION_COMMAND",
    "JCODE_DICTATION_KEY",
    "JCODE_DICTATION_MODE",
    "JCODE_DICTATION_TIMEOUT_SECS",
    "JCODE_DIFF_LINE_WRAP",
    "JCODE_DIFF_MODE",
    "JCODE_DIFF_MODE_CYCLE_KEY",
    "JCODE_DIAGRAM_PANE_TOGGLE_KEY",
    "JCODE_DISABLE_BASE_TOOLS",
    "JCODE_DISABLED_ANIMATIONS",
    "JCODE_DISABLED_TOOLS",
    "JCODE_DISCORD_BOT_TOKEN",
    "JCODE_DISCORD_BOT_USER_ID",
    "JCODE_DISCORD_CHANNEL_ID",
    "JCODE_DISCORD_REPLY_ENABLED",
    "JCODE_DISPLAY_CENTERED",
    "JCODE_EFFORT_DECREASE_KEY",
    "JCODE_EFFORT_INCREASE_KEY",
    "JCODE_EMAIL_REPLY_ENABLED",
    "JCODE_EMAIL_TO",
    "JCODE_FOCUS_HOOK",
    "JCODE_GATEWAY_BIND_ADDR",
    "JCODE_GATEWAY_ENABLED",
    "JCODE_GATEWAY_PORT",
    "JCODE_HOME",
    "JCODE_HOOK_PRE_TOOL",
    "JCODE_HOOK_PRE_TOOL_TIMEOUT_MS",
    "JCODE_HOOK_POST_TOOL",
    "JCODE_HOOK_SESSION_END",
    "JCODE_HOOK_SESSION_START",
    "JCODE_HOOK_TURN_END",
    "JCODE_HOOK_TURN_START",
    "JCODE_IDLE_ANIMATION",
    "JCODE_IMAP_HOST",
    "JCODE_INFO_WIDGET_TOGGLE_KEY",
    "JCODE_JADE_RELAY_API_BASE",
    "JCODE_JADE_RELAY_ENABLED",
    "JCODE_JADE_RELAY_LAUNCH_ENABLED",
    "JCODE_JADE_RELAY_LAUNCH_WORKING_DIR",
    "JCODE_JADE_RELAY_REPLY_ENABLED",
    "JCODE_JADE_RELAY_SESSION_ID",
    "JCODE_JADE_RELAY_TOKEN",
    "JCODE_JADE_RELAY_TOKEN_ID",
    "JCODE_JADE_RELAY_USER_ID",
    "JCODE_KV_CACHE_MISS_NOTICES",
    "JCODE_LATEX_RENDERING",
    "JCODE_MARKDOWN_SPACING",
    "JCODE_MEMORY_EMBEDDING_BACKEND",
    "JCODE_MEMORY_EMBEDDING_BASE_URL",
    "JCODE_MEMORY_EMBEDDING_DIM",
    "JCODE_MEMORY_EMBEDDING_MODEL",
    "JCODE_MEMORY_ENABLED",
    "JCODE_ENABLE_MERMAID",
    "JCODE_MEMORY_MODEL",
    "JCODE_MEMORY_SIDECAR_ENABLED",
    "JCODE_PERSIST_MEMORY_INJECTIONS",
    "JCODE_MESSAGE_TIMESTAMPS",
    "JCODE_MODEL",
    "JCODE_MODEL_SWITCH_KEY",
    "JCODE_MODEL_SWITCH_PREV_KEY",
    "JCODE_MOUSE_CAPTURE",
    "JCODE_NEW_TERMINAL_KEY",
    "JCODE_NO_EMOJI",
    "JCODE_NTFY_SERVER",
    "JCODE_NTFY_TOPIC",
    "JCODE_OPENAI_NATIVE_COMPACTION_MODE",
    "JCODE_OPENAI_NATIVE_COMPACTION_THRESHOLD_TOKENS",
    "JCODE_OPENAI_REASONING_EFFORT",
    "JCODE_OPENAI_SERVICE_TIER",
    "JCODE_OPENAI_TRANSPORT",
    "JCODE_ANTHROPIC_REASONING_EFFORT",
    "JCODE_PRESERVE_REASONING_CONTEXT",
    "JCODE_PERFORMANCE",
    "JCODE_PIN_IMAGES",
    "JCODE_PIN_TODOS",
    "JCODE_PREVENT_SLEEP_WHILE_STREAMING",
    "JCODE_PROVIDER",
    "JCODE_PROXY",
    "JCODE_NO_PROXY",
    "JCODE_PROMPT_ENTRY_ANIMATION",
    "JCODE_QUEUE_MODE",
    "JCODE_REASONING_DISPLAY",
    "JCODE_REDRAW_FPS",
    "JCODE_SAME_PROVIDER_ACCOUNT_FAILOVER",
    "JCODE_SCROLL_BOOKMARK_KEY",
    "JCODE_SCROLL_DOWN_FALLBACK_KEY",
    "JCODE_SCROLL_DOWN_KEY",
    "JCODE_SCROLL_PAGE_DOWN_KEY",
    "JCODE_SCROLL_PAGE_UP_KEY",
    "JCODE_SCROLL_PROMPT_DOWN_KEY",
    "JCODE_SCROLL_PROMPT_UP_KEY",
    "JCODE_SCROLL_UP_FALLBACK_KEY",
    "JCODE_SCROLL_UP_KEY",
    "JCODE_SEARXNG_URL",
    "JCODE_SHOW_AGENTGREP_OUTPUT",
    "JCODE_SHOW_DIFFS",
    "JCODE_SHOW_THINKING",
    "JCODE_SIDE_PANEL_TOGGLE_KEY",
    "JCODE_SIDE_PANEL_NATIVE_SCROLLBAR",
    "JCODE_SMTP_PASSWORD",
    "JCODE_SPAWN_HOOK",
    "JCODE_STREAM_IDLE_TIMEOUT_SECS",
    "JCODE_SWARM_ENABLED",
    "JCODE_SWARM_MODEL",
    "JCODE_SWARM_MAX_CONCURRENT_AGENTS",
    "JCODE_SWARM_SPAWN_MODE",
    "JCODE_SWARM_STRIP_LAYOUT",
    "JCODE_TELEGRAM_BOT_TOKEN",
    "JCODE_TELEGRAM_CHAT_ID",
    "JCODE_TELEGRAM_REPLY_ENABLED",
    "JCODE_TOOL_CALL_DETAILS",
    "JCODE_TOOL_PROFILE",
    "JCODE_TOOLS",
    "JCODE_TRUSTED_EXTERNAL_AUTH_SOURCES",
    "JCODE_TYPING_SCROLL_LOCK_TOGGLE_KEY",
    "JCODE_UPDATE_CHANNEL",
    "JCODE_WEBSEARCH_ENGINE",
    "JCODE_WEBSEARCH_FALLBACK_ENGINES",
    "JCODE_WORKSPACE_DOWN_KEY",
    "JCODE_WORKSPACE_LEFT_KEY",
    "JCODE_WORKSPACE_RIGHT_KEY",
    "JCODE_WORKSPACE_UP_KEY",
    "XDG_CONFIG_HOME",
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConfigCacheFingerprint {
    path: Option<PathBuf>,
    modified: Option<SystemTime>,
    len: Option<u64>,
    env: Vec<(String, String)>,
}

impl ConfigCacheFingerprint {
    fn current() -> Self {
        let path = Config::path();
        let metadata = path.as_ref().and_then(|path| std::fs::metadata(path).ok());
        Self {
            path,
            modified: metadata
                .as_ref()
                .and_then(|metadata| metadata.modified().ok()),
            len: metadata.as_ref().map(std::fs::Metadata::len),
            env: config_env_fingerprint(),
        }
    }
}

struct ConfigCache {
    config: &'static Config,
    fingerprint: ConfigCacheFingerprint,
    last_checked: Instant,
    force_reload: bool,
}

static CONFIG_CACHE: LazyLock<RwLock<ConfigCache>> = LazyLock::new(|| {
    let config = leak_config(Config::load());
    // Fingerprint after the load: applying env overrides may set env vars
    // (e.g. copilot_premium -> JCODE_COPILOT_PREMIUM), and fingerprinting
    // first would guarantee a spurious full reload on the next check.
    let fingerprint = ConfigCacheFingerprint::current();
    // Seed the global context-limit cache from named provider configs on first
    // load so every codepath (TUI info widget, compaction budget, model
    // switching) sees user-configured `context_window` values from the start.
    // Read from the loaded config directly to avoid recursing into config(),
    // which would deadlock on the still-initializing CONFIG_CACHE.
    populate_context_limits_from_config_ref(config);
    RwLock::new(ConfigCache {
        config,
        fingerprint,
        last_checked: Instant::now(),
        force_reload: false,
    })
});

fn leak_config(config: Config) -> &'static Config {
    Box::leak(Box::new(config))
}

/// Seed the global context-limit cache from a config reference directly.
///
/// Used during CONFIG_CACHE initialization (where calling config() would
/// deadlock) and shares its logic with
/// `crate::provider::populate_context_limits_from_config`.
fn populate_context_limits_from_config_ref(cfg: &Config) {
    crate::provider::populate_context_limits_from_config_value(cfg);
}

/// Get the global config instance.
///
/// The returned reference is backed by a reloadable process cache. Calls check
/// the config file path/metadata and relevant environment overrides on a short
/// throttle, not every frame. When those inputs change, the next checked call
/// reloads config.toml and invalidates dependent auth/model caches. Older
/// references remain valid for the duration of any in-flight operation.
pub fn config() -> &'static Config {
    let now = Instant::now();
    if let Ok(cache) = CONFIG_CACHE.read()
        && !cache.force_reload
        && now.duration_since(cache.last_checked) < CONFIG_CACHE_CHECK_INTERVAL
    {
        return cache.config;
    }

    let mut reload_reason = None;
    let config = {
        let mut cache = CONFIG_CACHE
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let now = Instant::now();
        if !cache.force_reload
            && now.duration_since(cache.last_checked) < CONFIG_CACHE_CHECK_INTERVAL
        {
            return cache.config;
        }

        let fingerprint = ConfigCacheFingerprint::current();
        cache.last_checked = now;
        if cache.force_reload || cache.fingerprint != fingerprint {
            reload_reason = Some(describe_config_reload(
                cache.force_reload,
                &cache.fingerprint,
                &fingerprint,
            ));
            cache.config = leak_config(Config::load());
            // Loading applies env overrides that can themselves set env vars
            // (e.g. copilot_premium propagates config -> JCODE_COPILOT_PREMIUM).
            // Re-fingerprint after the load so those self-inflicted env changes
            // don't trigger a guaranteed second reload on the next check.
            cache.fingerprint = ConfigCacheFingerprint::current();
            cache.force_reload = false;
        }
        cache.config
    };

    if let Some(reason) = reload_reason {
        crate::logging::info(&format!("CONFIG_RELOAD {}", reason));
        // A config reload can change config-derived system prompt sections
        // (feature toggles, sponsors, ...), which legitimately invalidates the
        // KV cache prefix of warm sessions. Document it so a subsequent
        // harness-attributed cache miss is surfaced with this cause instead of
        // as an unexplained prompt mutation.
        crate::cache_invalidation::record("config reload", &reason);
        notify_config_reloaded();
        // Re-seed the global context-limit cache so user edits to named
        // provider `context_window` values take effect without a restart.
        crate::provider::populate_context_limits_from_config();
    }

    config
}

fn describe_config_reload(
    forced: bool,
    previous: &ConfigCacheFingerprint,
    next: &ConfigCacheFingerprint,
) -> String {
    let mut parts = Vec::new();
    if forced {
        parts.push("forced=true".to_string());
    }
    if previous.path != next.path {
        parts.push(format!(
            "path={:?}->{:?}",
            previous.path.as_ref().map(|p| p.display().to_string()),
            next.path.as_ref().map(|p| p.display().to_string())
        ));
    }
    if previous.modified != next.modified {
        parts.push("modified_changed=true".to_string());
    }
    if previous.len != next.len {
        parts.push(format!("len={:?}->{:?}", previous.len, next.len));
    }
    let env_changes = describe_env_changes(&previous.env, &next.env);
    if !env_changes.is_empty() {
        parts.push(format!("env=[{}]", env_changes.join(", ")));
    }
    if parts.is_empty() {
        "unchanged".to_string()
    } else {
        parts.join(" ")
    }
}

fn describe_env_changes(previous: &[(String, String)], next: &[(String, String)]) -> Vec<String> {
    let previous_map: BTreeMap<&str, &str> = previous
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    let next_map: BTreeMap<&str, &str> = next
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    let keys: BTreeSet<&str> = previous_map
        .keys()
        .chain(next_map.keys())
        .copied()
        .collect();

    keys.into_iter()
        .filter_map(|key| match (previous_map.get(key), next_map.get(key)) {
            (Some(previous), Some(next)) if previous != next => Some(format!(
                "{}:changed({}->{})",
                key,
                env_value_fingerprint(previous),
                env_value_fingerprint(next)
            )),
            (None, Some(next)) => Some(format!("{}:added({})", key, env_value_fingerprint(next))),
            (Some(previous), None) => Some(format!(
                "{}:removed({})",
                key,
                env_value_fingerprint(previous)
            )),
            _ => None,
        })
        .collect()
}

fn env_value_fingerprint(value: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    format!("len:{} hash:{:016x}", value.len(), hasher.finish())
}

fn config_env_fingerprint() -> Vec<(String, String)> {
    let mut values = std::env::vars_os()
        .filter_map(|(key, value)| {
            let key = key.to_string_lossy().to_string();
            if CONFIG_ENV_KEYS.contains(&key.as_str()) {
                Some((key, value.to_string_lossy().to_string()))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    values.sort_by(|left, right| left.0.cmp(&right.0));
    values
}

pub fn invalidate_config_cache() {
    let mut cache = CONFIG_CACHE
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    cache.force_reload = true;
    drop(cache);
    notify_config_reloaded();
}

fn notify_config_reloaded() {
    CONFIG_RELOAD_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    for listener in CONFIG_RELOAD_LISTENERS
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .iter()
    {
        listener();
    }
}

/// Monotonic counter bumped every time the config cache reloads.
///
/// Callers that snapshot config-derived state (e.g. the TUI's parsed
/// keybindings) can poll this cheaply and re-derive their snapshot when the
/// generation changes, giving instant hot-reload of config edits without a
/// restart.
static CONFIG_RELOAD_GENERATION: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// Current config reload generation. Increments after every cache reload.
pub fn config_reload_generation() -> u64 {
    CONFIG_RELOAD_GENERATION.load(std::sync::atomic::Ordering::Relaxed)
}

/// Listeners invoked after the config cache reloads.
///
/// Config is a foundational module, so instead of reaching up into higher-level
/// subsystems (auth cache, event bus) on reload, those subsystems register a
/// reaction here at startup. This keeps config free of upward dependencies and
/// breaks the config -> auth / config -> bus cycle edges.
/// Type of a config reload listener callback.
type ConfigReloadListener = fn();

static CONFIG_RELOAD_LISTENERS: LazyLock<RwLock<Vec<ConfigReloadListener>>> =
    LazyLock::new(|| RwLock::new(Vec::new()));

/// Register a callback to run after the config cache reloads.
///
/// Callbacks must be cheap and non-blocking; they run on whichever thread
/// triggers the reload. Intended to be called once per subsystem during
/// process startup.
pub fn on_config_reloaded(listener: fn()) {
    CONFIG_RELOAD_LISTENERS
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(listener);
}

/// Main configuration struct
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    /// Keybinding configuration
    pub keybindings: KeybindingsConfig,

    /// Display/UI configuration
    pub display: DisplayConfig,

    /// Feature toggles
    pub features: FeatureConfig,

    /// Web search tool configuration
    pub websearch: WebSearchConfig,

    /// Built-in tool exposure configuration
    pub tools: ToolConfig,

    /// Agent Client Protocol adapter configuration
    pub acp: AcpConfig,

    /// Auth trust / consent configuration
    pub auth: AuthConfig,

    /// Provider configuration
    pub provider: ProviderConfig,

    /// Named provider profiles, keyed by profile name.
    ///
    /// Example:
    /// [providers.my-gateway]
    /// type = "openai-compatible"
    /// base_url = "https://llm.example.com/v1"
    /// api_key_env = "MY_GATEWAY_API_KEY"
    #[serde(
        deserialize_with = "deserialize_providers",
        serialize_with = "serialize_providers"
    )]
    pub providers: BTreeMap<String, NamedProviderConfig>,

    /// Agent-specific model defaults
    pub agents: AgentsConfig,

    /// Terminal window/pane spawning configuration
    pub terminal: TerminalConfig,

    /// Lifecycle hooks (external commands at turn/session/tool boundaries)
    pub hooks: HooksConfig,

    /// Ambient mode configuration
    pub ambient: AmbientConfig,

    /// Safety / notification configuration
    pub safety: SafetyConfig,

    /// Desktop notifications for interactive sessions (e.g. turn completion)
    pub notifications: NotificationsConfig,

    /// WebSocket gateway configuration (for iOS/web clients)
    pub gateway: GatewayConfig,

    /// Compaction configuration
    pub compaction: CompactionConfig,

    /// Power-management configuration (prevent sleep while streaming)
    pub power: PowerConfig,

    /// Auto-review configuration
    pub autoreview: AutoReviewConfig,

    /// Auto-judge configuration
    pub autojudge: AutoJudgeConfig,

    /// Partner discovery configuration. Skipped when it matches the shipped
    /// default so saving config never bakes today's default into the file (see
    /// [`sponsors_is_default`]).
    #[serde(skip_serializing_if = "sponsors_is_default")]
    pub sponsors: SponsorsConfig,

    /// Network / proxy configuration for outbound provider requests.
    pub network: NetworkConfig,
}

/// Agent Client Protocol adapter configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AcpConfig {
    /// Client compatibility profile: "standard" (default), "extended", or "full".
    pub profile: String,
    /// Tool profile to request when `jcode acp` starts a daemon itself.
    pub tool_profile: String,
}

impl Default for AcpConfig {
    fn default() -> Self {
        Self {
            profile: "standard".to_string(),
            tool_profile: "acp".to_string(),
        }
    }
}

/// Controls which tools are sent to the model.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ToolConfig {
    /// Tool profile: "full" (default), "acp", "minimal"/"lite", or "none".
    pub profile: String,
    /// Explicit allow-list. When set, only these tools are exposed.
    /// Use "*" or "all" to expose all tools without an allow-list.
    pub enabled: Vec<String>,
    /// Tools to remove after applying profile/enabled.
    pub disabled: Vec<String>,
    /// Disable all built-in tools unless `enabled` is provided.
    pub disable_base_tools: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolSelection {
    pub allowed_tools: Option<HashSet<String>>,
    pub disabled_tools: HashSet<String>,
}

impl ToolConfig {
    pub fn selection(&self) -> ToolSelection {
        let mut allowed_tools = self.base_allowed_tools();
        let disabled_tools: HashSet<String> = self
            .disabled
            .iter()
            .map(|name| normalize_tool_name(name))
            .filter(|name| !name.is_empty())
            .collect();

        if let Some(allowed) = allowed_tools.as_mut() {
            for name in &disabled_tools {
                allowed.remove(name);
            }
        }

        ToolSelection {
            allowed_tools,
            disabled_tools,
        }
    }

    pub fn allowed_tools(&self) -> Option<HashSet<String>> {
        self.selection().allowed_tools
    }

    pub fn apply_to_allowed_set(&self, allowed: &mut HashSet<String>) {
        let selection = self.selection();
        if let Some(global_allowed) = selection.allowed_tools {
            allowed.retain(|name| global_allowed.contains(name));
        }
        for disabled in selection.disabled_tools {
            allowed.remove(&disabled);
        }
    }

    fn base_allowed_tools(&self) -> Option<HashSet<String>> {
        let (explicit, enables_all_tools) = self.normalized_enabled_tools();

        let profile = self.profile.trim().to_ascii_lowercase();
        if enables_all_tools {
            None
        } else if !explicit.is_empty() {
            Some(explicit)
        } else if self.disable_base_tools || matches!(profile.as_str(), "none" | "off" | "disabled")
        {
            Some(HashSet::new())
        } else if matches!(profile.as_str(), "acp") {
            Some(
                [
                    "bash",
                    "read",
                    "write",
                    "edit",
                    "multiedit",
                    "apply_patch",
                    "patch",
                    "agentgrep",
                    "ls",
                    "batch",
                ]
                .into_iter()
                .map(|name| name.to_string())
                .collect(),
            )
        } else if matches!(profile.as_str(), "minimal" | "lite" | "small") {
            Some(
                [
                    "bash",
                    "read",
                    "write",
                    "edit",
                    "multiedit",
                    "apply_patch",
                    "patch",
                    "agentgrep",
                    "ls",
                ]
                .into_iter()
                .map(|name| name.to_string())
                .collect(),
            )
        } else {
            None
        }
    }

    fn normalized_enabled_tools(&self) -> (HashSet<String>, bool) {
        let mut enabled = HashSet::new();
        let mut enables_all_tools = false;

        for name in &self.enabled {
            let normalized = normalize_tool_name(name);
            if normalized.is_empty() {
                continue;
            }
            if normalized == "*" || normalized.eq_ignore_ascii_case("all") {
                enables_all_tools = true;
            } else {
                enabled.insert(normalized);
            }
        }

        (enabled, enables_all_tools)
    }
}

fn normalize_tool_name(name: &str) -> String {
    let trimmed = name.trim().trim_matches('"');
    jcode_tool_types::resolve_tool_name(trimmed).to_string()
}

mod config_file;
mod default_file;
mod display_summary;
mod env_overrides;
mod modelcap;

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "config_color_tests.rs"]
mod color_tests;

/// Whether integration discovery settings carry no information beyond the shipped
/// default, so `[sponsors]` can be left out of written config files.
///
/// Discovery originally shipped opt-in with `enabled = false`, and because
/// config saves serialize the whole struct, any save during that window froze
/// the old default into the user's file and permanently disabled discovery even
/// after the default flipped. Omitting default sections prevents a repeat.
fn sponsors_is_default(sponsors: &SponsorsConfig) -> bool {
    sponsors.enabled && is_default_discovery_endpoint(&sponsors.endpoint)
}

/// Endpoints that only ever came from a shipped default, never a user choice.
fn is_default_discovery_endpoint(endpoint: &str) -> bool {
    matches!(
        endpoint.trim_end_matches('/'),
        "https://api.jcode.sh/v1/discovery" | "https://api.solosystems.dev/v1/discovery"
    )
}
