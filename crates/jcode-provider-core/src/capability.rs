//! Unified model capability model, embedded registry, and resolution pipeline.
//!
//! This module is the single source of truth for *declarative* model
//! capabilities (chat, modalities, reasoning, tools, context window, sampling)
//! in jcode. It mirrors the model-access architecture described in the
//! Reasonix design docs (000-task 01/03): every capability decision flows
//! through one resolution entry point with a fixed priority:
//!
//! ```text
//! explicit config > embedded registry > heuristics > conservative default
//! ```
//!
//! Explicit configuration always wins; the registry only supplies defaults for
//! models that ship no explicit configuration, and heuristics are the final
//! fallback for unknown models. Nothing in this module ever touches
//! credentials, endpoints, or proxy settings — `ModelCapability` values are
//! safe to project into remote descriptors.

use serde::{Deserialize, Serialize};
use std::sync::RwLock;

/// Input modality a model can consume.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Modality {
    Text,
    Image,
    Audio,
}

impl Modality {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Image => "image",
            Self::Audio => "audio",
        }
    }

    pub fn parse(input: &str) -> Option<Self> {
        match input.trim().to_ascii_lowercase().as_str() {
            "text" | "chat" => Some(Self::Text),
            "image" | "images" | "vision" | "visual" => Some(Self::Image),
            "audio" => Some(Self::Audio),
            _ => None,
        }
    }
}

/// Wire-level reasoning shape used by the runtime that serves this model.
///
/// The enum is a *declaration*, not the wire translator: runtimes still decide
/// the exact request fields, but the capability layer records which family of
/// request shapes the model speaks so pickers, effort gating, and diagnostics
/// can reason about it without re-deriving host heuristics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningProtocol {
    /// No explicit declaration; fall back to heuristics/runtime detection.
    Auto,
    /// DeepSeek-style `thinking.type` + `reasoning_effort`.
    DeepSeek,
    /// Standard OpenAI-compatible `reasoning_effort`.
    OpenAi,
    /// Binary `thinking.type` knob (Zhipu GLM, LongCat, MiniMax).
    ThinkingType,
    /// Anthropic Messages API extended thinking (`thinking` + `output_config.effort`).
    Anthropic,
    /// Reasoning disabled / not controllable.
    None,
}

impl ReasoningProtocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::DeepSeek => "deepseek",
            Self::OpenAi => "openai",
            Self::ThinkingType => "thinking_type",
            Self::Anthropic => "anthropic",
            Self::None => "none",
        }
    }

    pub fn parse(input: &str) -> Option<Self> {
        match input.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "deepseek" => Some(Self::DeepSeek),
            "openai" | "openai-compatible" | "responses" => Some(Self::OpenAi),
            "thinking_type" | "thinking-type" | "binary" => Some(Self::ThinkingType),
            "anthropic" | "messages" => Some(Self::Anthropic),
            "none" | "off" | "disabled" => Some(Self::None),
            _ => None,
        }
    }
}

/// Reasoning behavior for one model.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ReasoningCapability {
    pub protocol: Option<ReasoningProtocol>,
    /// Selectable `/effort` values in canonical jcode vocabulary.
    pub efforts: Vec<String>,
    /// Effort used by `/effort auto` and new sessions.
    pub default_effort: Option<String>,
    /// Full assistant-message reasoning round-trip (Kimi K3 style).
    pub round_trip: bool,
    /// Tool-call turns must replay `reasoning_content` (DeepSeek style).
    pub tool_call_replay: bool,
    /// `thinking` knob shape: `enabled`/`disabled`/`adaptive`/`binary`.
    pub thinking_kind: Option<String>,
}

/// Sampling constraints for one model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SamplingCapability {
    /// Whether the endpoint honors a temperature field. `true` by default;
    /// fixed-sampling models (e.g. Kimi K3) set `false`.
    pub temperature_supported: bool,
    /// The endpoint requires omitting user sampling knobs entirely.
    pub fixed_sampling: bool,
    /// Output-limit request field: `max_tokens` or `max_completion_tokens`.
    pub output_limit_field: Option<String>,
}

impl Default for SamplingCapability {
    fn default() -> Self {
        Self {
            temperature_supported: true,
            fixed_sampling: false,
            output_limit_field: None,
        }
    }
}

/// Unified capability record for one model id.
///
/// `modalities` always contains `Text` for chat models; extra modalities
/// (image/audio) are appended when declared or inferred. `tools` is `None`
/// when unknown, which callers must treat as "tools available" (conservative,
/// preserves current behavior).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelCapability {
    /// Canonical model id the record applies to.
    pub id: String,
    pub chat: bool,
    pub modalities: Vec<Modality>,
    pub reasoning: ReasoningCapability,
    /// `None` = unknown (treat as available); `Some(false)` disables tools.
    pub tools: Option<bool>,
    pub context_window: Option<usize>,
    pub output_window: Option<usize>,
    pub sampling: SamplingCapability,
}

impl Default for ModelCapability {
    fn default() -> Self {
        Self {
            id: String::new(),
            chat: true,
            modalities: vec![Modality::Text],
            reasoning: ReasoningCapability::default(),
            tools: None,
            context_window: None,
            output_window: None,
            sampling: SamplingCapability::default(),
        }
    }
}

impl ModelCapability {
    pub fn supports_image(&self) -> bool {
        self.modalities.contains(&Modality::Image)
    }

    /// Whether tools may be sent. Unknown (`None`) counts as available.
    pub fn supports_tools(&self) -> bool {
        self.tools.unwrap_or(true)
    }

    /// Serde-friendly projection for remote descriptors (no internal types).
    pub fn route_view(&self) -> RouteCapabilityView {
        RouteCapabilityView {
            modalities: self
                .modalities
                .iter()
                .map(|modality| modality.as_str().to_string())
                .collect(),
            tools: self.tools,
            reasoning_protocol: self
                .reasoning
                .protocol
                .map(|protocol| protocol.as_str().to_string()),
            context_window: self.context_window,
            output_window: self.output_window,
            sampling: Some(SamplingView::from(self.sampling.clone())),
        }
    }

    /// True when the projection carries no information beyond the conservative
    /// default (`text` chat model with no other declared capabilities). Such
    /// routes serialize without a capability field to keep wire bytes stable.
    pub fn route_view_is_default(&self) -> bool {
        self.route_view().is_default()
    }
}

/// Sampling constraints in a serde-friendly, all-`Option` form.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SamplingView {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature_supported: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed_sampling: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_limit_field: Option<String>,
}

impl SamplingView {
    pub fn is_empty(&self) -> bool {
        self.temperature_supported.is_none()
            && self.fixed_sampling.is_none()
            && self.output_limit_field.is_none()
    }
}

impl From<SamplingCapability> for SamplingView {
    fn from(sampling: SamplingCapability) -> Self {
        Self {
            temperature_supported: (!sampling.temperature_supported).then_some(false),
            fixed_sampling: sampling.fixed_sampling.then_some(true),
            output_limit_field: sampling.output_limit_field,
        }
    }
}

/// Capability fields attached to a route/protocol descriptor.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouteCapabilityView {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modalities: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_protocol: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_window: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sampling: Option<SamplingView>,
}

impl RouteCapabilityView {
    /// True when this is exactly the conservative default (`text` + no other
    /// fields). Serializers should omit such views to keep wire bytes stable.
    pub fn is_default(&self) -> bool {
        (self.modalities.is_empty() || self.modalities == ["text".to_string()])
            && self.tools.is_none()
            && self.reasoning_protocol.is_none()
            && self.context_window.is_none()
            && self.output_window.is_none()
            && self
                .sampling
                .as_ref()
                .is_none_or(|sampling| sampling.is_empty())
    }

    pub fn non_default(&self) -> Option<&Self> {
        (!self.is_default()).then_some(self)
    }
}

/// Explicit per-model capability overrides coming from configuration.
///
/// This is the config-side projection of `NamedProviderModelConfig` capability
/// fields (see `jcode-config-types`). `None` fields mean "no explicit
/// declaration" and defer to registry/heuristic resolution.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExplicitModelCapability {
    pub vision: Option<bool>,
    pub tools: Option<bool>,
    pub reasoning_protocol: Option<ReasoningProtocol>,
    pub supported_efforts: Option<Vec<String>>,
    pub default_effort: Option<String>,
    pub context_window: Option<usize>,
    pub output_window: Option<usize>,
    pub temperature_supported: Option<bool>,
    pub fixed_sampling: Option<bool>,
    pub output_limit_field: Option<String>,
    /// Extra modalities declared via `input = ["image", ...]` (text is implicit).
    pub input_modalities: Option<Vec<Modality>>,
}

/// Where a resolved capability field came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilitySource {
    ExplicitConfig,
    Registry,
    Heuristic,
    Default,
}

impl CapabilitySource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExplicitConfig => "config",
            Self::Registry => "registry",
            Self::Heuristic => "heuristic",
            Self::Default => "default",
        }
    }
}

/// Per-field provenance for a resolved capability, used by diagnostics
/// (`provider-doctor` / `model list --verbose`) and the model picker.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityTrace {
    pub context_window: Option<CapabilitySource>,
    pub output_window: Option<CapabilitySource>,
    pub vision: Option<CapabilitySource>,
    pub tools: Option<CapabilitySource>,
    pub reasoning_protocol: Option<CapabilitySource>,
    pub efforts: Option<CapabilitySource>,
    pub sampling: Option<CapabilitySource>,
}

impl CapabilityTrace {
    pub fn is_empty(&self) -> bool {
        self.context_window.is_none()
            && self.output_window.is_none()
            && self.vision.is_none()
            && self.tools.is_none()
            && self.reasoning_protocol.is_none()
            && self.efforts.is_none()
            && self.sampling.is_none()
    }
}

/// A capability merged from explicit config, registry, heuristics, and
/// conservative defaults, plus the per-field provenance trace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedModelCapability {
    pub capability: ModelCapability,
    pub trace: CapabilityTrace,
}

impl ResolvedModelCapability {
    /// The route-safe projection; `None` when the view is exactly the
    /// conservative default (serializers omit the field entirely).
    pub fn route_view(&self) -> Option<RouteCapabilityView> {
        self.capability.route_view().non_default().cloned()
    }
}

/// A compact, const-constructible registry entry.
///
/// All fields are `&'static` so the embedded table costs no startup time.
/// `model` may end with `*` for a prefix match; `provider` is an optional
/// provider-key filter (`claude`, `openai`, `deepseek`, ...). The first
/// matching entry wins, so specific entries must be listed before generic
/// ones.
pub struct RegistryEntry {
    pub model: &'static str,
    pub provider: Option<&'static str>,
    pub chat: bool,
    pub modalities: &'static [Modality],
    pub reasoning_protocol: Option<ReasoningProtocol>,
    pub efforts: &'static [&'static str],
    pub default_effort: Option<&'static str>,
    pub round_trip: bool,
    pub tool_call_replay: bool,
    pub thinking_kind: Option<&'static str>,
    pub tools: Option<bool>,
    pub context_window: Option<usize>,
    pub output_window: Option<usize>,
    pub temperature_supported: Option<bool>,
    pub fixed_sampling: Option<bool>,
    pub output_limit_field: Option<&'static str>,
}

/// A user-supplied registry entry (from `modelcap.json`).
///
/// Same field semantics as the embedded [`RegistryEntry`], but owned so it can
/// be deserialized from disk. Entries are validated leniently: unknown JSON
/// fields are ignored, unknown protocol/modality spellings fall back to the
/// embedded/heuristic resolution, and only entries with a non-empty `model`
/// participate. User entries take precedence over the embedded table for the
/// same model key (exact before prefix), matching the design rule that the
/// registry only supplies defaults and explicit config always wins.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UserRegistryEntry {
    /// Exact model id, or a `prefix*` wildcard.
    pub model: String,
    /// Optional provider-key filter (same semantics as `RegistryEntry.provider`).
    pub provider: Option<String>,
    /// Explicit visual-capability declaration. `Some(false)` prevents the
    /// vision heuristic from re-enabling image input for this model.
    pub vision: Option<bool>,
    pub chat: Option<bool>,
    pub modalities: Vec<String>,
    pub reasoning_protocol: Option<String>,
    #[serde(default, alias = "supported_efforts", alias = "supported-efforts")]
    pub efforts: Vec<String>,
    #[serde(default, alias = "default-effort")]
    pub default_effort: Option<String>,
    pub round_trip: bool,
    pub tool_call_replay: bool,
    pub thinking_kind: Option<String>,
    pub tools: Option<bool>,
    pub context_window: Option<usize>,
    pub output_window: Option<usize>,
    pub temperature_supported: Option<bool>,
    pub fixed_sampling: Option<bool>,
    pub output_limit_field: Option<String>,
}

impl UserRegistryEntry {
    fn to_model_capability(&self, model: &str) -> ModelCapability {
        let mut capability = ModelCapability {
            id: model.trim().to_string(),
            chat: self.chat.unwrap_or(true),
            tools: self.tools,
            context_window: self.context_window,
            output_window: self.output_window,
            sampling: SamplingCapability {
                temperature_supported: self.temperature_supported.unwrap_or(true),
                fixed_sampling: self.fixed_sampling.unwrap_or(false),
                output_limit_field: self.output_limit_field.clone(),
            },
            ..ModelCapability::default()
        };
        if let Some(vision) = self.vision {
            set_image_modality(&mut capability, vision);
        }
        if self.vision.is_none() {
            for modality in &self.modalities {
                if let Some(modality) = Modality::parse(modality) {
                    capability.modalities.push(modality);
                }
            }
        }
        capability.reasoning.protocol = self
            .reasoning_protocol
            .as_deref()
            .and_then(ReasoningProtocol::parse);
        capability.reasoning.efforts = self.efforts.clone();
        capability.reasoning.default_effort = self.default_effort.clone();
        capability.reasoning.round_trip = self.round_trip;
        capability.reasoning.tool_call_replay = self.tool_call_replay;
        capability.reasoning.thinking_kind = self.thinking_kind.clone();
        capability
    }
}

/// JSON envelope for user registry files (`modelcap.json`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UserRegistryFile {
    pub entries: Vec<UserRegistryEntry>,
}

/// Process-wide user registry entries, installed by `jcode-base` when the
/// user `modelcap.json` is loaded. Resolution consults this table before the
/// embedded const registry, so users can extend or correct defaults without
/// recompiling.
static USER_REGISTRY: RwLock<Vec<UserRegistryEntry>> = RwLock::new(Vec::new());

/// Install the user registry entries (called once at config load).
pub fn set_user_registry_entries(entries: Vec<UserRegistryEntry>) {
    if let Ok(mut registry) = USER_REGISTRY.write() {
        *registry = entries;
    }
}

/// Test hook: clear any installed user registry.
pub fn clear_user_registry_entries() {
    set_user_registry_entries(Vec::new());
}

#[allow(clippy::too_many_arguments)]
const fn entry(
    model: &'static str,
    provider: Option<&'static str>,
    modalities: &'static [Modality],
    reasoning_protocol: Option<ReasoningProtocol>,
    efforts: &'static [&'static str],
    default_effort: Option<&'static str>,
    round_trip: bool,
    tool_call_replay: bool,
    thinking_kind: Option<&'static str>,
    tools: Option<bool>,
    context_window: Option<usize>,
    output_window: Option<usize>,
    temperature_supported: Option<bool>,
    fixed_sampling: Option<bool>,
    output_limit_field: Option<&'static str>,
) -> RegistryEntry {
    RegistryEntry {
        model,
        provider,
        chat: true,
        modalities,
        reasoning_protocol,
        efforts,
        default_effort,
        round_trip,
        tool_call_replay,
        thinking_kind,
        tools,
        context_window,
        output_window,
        temperature_supported,
        fixed_sampling,
        output_limit_field,
    }
}

/// Embedded default capability registry.
///
/// Entries are seeded to mirror the existing heuristic outputs for the same
/// models (context windows, effort ladders) so net behavior does not change;
/// they add declarative knowledge for fields heuristics cannot express
/// (vision, tools, sampling, reasoning round-trip flags).
pub const EMBEDDED_REGISTRY: &[RegistryEntry] = &[
    // --- DeepSeek official family ---
    entry(
        "deepseek-v4-flash",
        None,
        &[],
        Some(ReasoningProtocol::DeepSeek),
        &["none", "low", "medium", "high", "max"],
        Some("high"),
        false,
        true,
        None,
        None,
        Some(1_000_000),
        None,
        None,
        None,
        None,
    ),
    entry(
        "deepseek-v4-pro",
        None,
        &[],
        Some(ReasoningProtocol::DeepSeek),
        &["none", "low", "medium", "high", "max"],
        Some("high"),
        false,
        true,
        None,
        None,
        Some(1_000_000),
        None,
        None,
        None,
        None,
    ),
    // --- Moonshot Kimi K3 family (bare `k3` ids served by Kimi Code) ---
    entry(
        "kimi-k3",
        None,
        &[Modality::Image],
        Some(ReasoningProtocol::OpenAi),
        &["low", "high", "max"],
        Some("max"),
        true,
        false,
        None,
        Some(true),
        Some(1_048_576),
        None,
        Some(false),
        Some(true),
        Some("max_completion_tokens"),
    ),
    entry(
        "kimi-k3-turbo",
        None,
        &[Modality::Image],
        Some(ReasoningProtocol::OpenAi),
        &["low", "high", "max"],
        Some("max"),
        true,
        false,
        None,
        Some(true),
        Some(1_048_576),
        None,
        Some(false),
        Some(true),
        Some("max_completion_tokens"),
    ),
    entry(
        "kimi-k2.7-code",
        None,
        &[Modality::Image],
        None,
        &[],
        None,
        false,
        false,
        None,
        Some(true),
        Some(262_144),
        None,
        None,
        None,
        None,
    ),
    // --- Zhipu GLM / Z.AI ---
    entry(
        "glm-5.2",
        None,
        &[],
        Some(ReasoningProtocol::ThinkingType),
        &["enabled", "disabled"],
        Some("enabled"),
        false,
        false,
        Some("enabled"),
        Some(true),
        Some(1_000_000),
        None,
        None,
        None,
        None,
    ),
    entry(
        "glm-5v-turbo",
        None,
        &[Modality::Image],
        Some(ReasoningProtocol::ThinkingType),
        &["enabled", "disabled"],
        Some("enabled"),
        false,
        false,
        Some("enabled"),
        Some(true),
        None,
        None,
        None,
        None,
        None,
    ),
    // --- MiniMax M3 ---
    entry(
        "minimax-m3",
        None,
        &[Modality::Image],
        Some(ReasoningProtocol::ThinkingType),
        &[],
        None,
        false,
        false,
        Some("adaptive"),
        Some(true),
        Some(204_800),
        None,
        None,
        None,
        None,
    ),
    // --- Qwen / DashScope vision families ---
    entry(
        "qwen3.7-plus",
        None,
        &[Modality::Image],
        None,
        &[],
        None,
        false,
        false,
        None,
        Some(true),
        Some(262_144),
        None,
        None,
        None,
        None,
    ),
    entry(
        "qwen3.6-plus",
        None,
        &[Modality::Image],
        None,
        &[],
        None,
        false,
        false,
        None,
        Some(true),
        Some(262_144),
        None,
        None,
        None,
        None,
    ),
    entry(
        "qwen3.7-max",
        None,
        &[Modality::Image],
        None,
        &[],
        None,
        false,
        false,
        None,
        Some(true),
        Some(262_144),
        None,
        None,
        None,
        None,
    ),
    // --- Anthropic Claude family: vision + tools always, reasoning per caps ---
    entry(
        "claude-*",
        Some("claude"),
        &[Modality::Image],
        Some(ReasoningProtocol::Anthropic),
        &[],
        None,
        false,
        false,
        None,
        Some(true),
        None,
        None,
        None,
        None,
        None,
    ),
    // --- OpenAI GPT-5 family (reasoning + tools; context via heuristics) ---
    entry(
        "gpt-5*",
        Some("openai"),
        &[],
        Some(ReasoningProtocol::OpenAi),
        crate::reasoning::OPENAI_SELECTABLE_EFFORTS,
        None,
        false,
        false,
        None,
        Some(true),
        None,
        None,
        None,
        None,
        None,
    ),
    // --- OpenAI o-series reasoning models (tools + selectable efforts) ---
    entry(
        "o1*",
        Some("openai"),
        &[],
        Some(ReasoningProtocol::OpenAi),
        crate::reasoning::OPENAI_SELECTABLE_EFFORTS,
        None,
        false,
        false,
        None,
        Some(true),
        None,
        None,
        None,
        None,
        None,
    ),
    entry(
        "o3*",
        Some("openai"),
        &[],
        Some(ReasoningProtocol::OpenAi),
        crate::reasoning::OPENAI_SELECTABLE_EFFORTS,
        None,
        false,
        false,
        None,
        Some(true),
        None,
        None,
        None,
        None,
        None,
    ),
    entry(
        "o4*",
        Some("openai"),
        &[],
        Some(ReasoningProtocol::OpenAi),
        crate::reasoning::OPENAI_SELECTABLE_EFFORTS,
        None,
        false,
        false,
        None,
        Some(true),
        None,
        None,
        None,
        None,
        None,
    ),
    entry(
        "o5*",
        Some("openai"),
        &[],
        Some(ReasoningProtocol::OpenAi),
        crate::reasoning::OPENAI_SELECTABLE_EFFORTS,
        None,
        false,
        false,
        None,
        Some(true),
        None,
        None,
        None,
        None,
        None,
    ),
    // --- Google Gemini family: multimodal + tool calling ---
    entry(
        "gemini-*",
        Some("gemini"),
        &[Modality::Image],
        Some(ReasoningProtocol::OpenAi),
        &[],
        None,
        false,
        false,
        None,
        Some(true),
        None,
        None,
        None,
        None,
        None,
    ),
];

fn registry_entry_for(model: &str, provider_hint: Option<&str>) -> Option<&'static RegistryEntry> {
    let normalized = model.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }
    let provider_key = provider_hint
        .and_then(|hint| crate::models::provider_key_from_hint(Some(hint)))
        .map(|key| key.to_string());
    let provider_matches =
        |entry_provider: Option<&str>| match (entry_provider, provider_key.as_deref()) {
            (None, _) => true,
            (Some(expected), Some(actual)) => expected == actual,
            (Some(_), None) => false,
        };
    // Exact model match first (specific beats prefix/wildcard).
    if let Some(found) = EMBEDDED_REGISTRY.iter().find(|entry| {
        !entry.model.ends_with('*') && entry.model == normalized && provider_matches(entry.provider)
    }) {
        return Some(found);
    }
    // Prefix match (`claude-*`, `gpt-5*`).
    EMBEDDED_REGISTRY.iter().find(|entry| {
        entry
            .model
            .strip_suffix('*')
            .is_some_and(|prefix| !prefix.is_empty() && normalized.starts_with(prefix))
            && provider_matches(entry.provider)
    })
}

fn user_registry_entry_for(model: &str, provider_hint: Option<&str>) -> Option<UserRegistryEntry> {
    let normalized = model.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }
    // User registry entries scope by *raw* provider names (arbitrary gateways),
    // unlike the embedded table which uses canonical keys. Match the hint
    // case-insensitively, allowing `openai-compatible:acme-gateway` style
    // prefixed hints to hit an `acme-gateway` entry.
    let raw_provider = provider_hint.map(|hint| hint.to_ascii_lowercase());
    let provider_matches =
        |entry_provider: Option<&String>| match (entry_provider, raw_provider.as_deref()) {
            (None, _) => true,
            (Some(expected), Some(actual)) => {
                let expected = expected.to_ascii_lowercase();
                actual == &expected || actual.ends_with(&format!(":{expected}"))
            }
            (Some(_), None) => false,
        };
    let entries = USER_REGISTRY.read().ok()?;
    if let Some(found) = entries.iter().find(|entry| {
        !entry.model.ends_with('*')
            && entry.model.eq_ignore_ascii_case(&normalized)
            && provider_matches(entry.provider.as_ref())
    }) {
        return Some(found.clone());
    }
    entries
        .iter()
        .find(|entry| {
            entry
                .model
                .strip_suffix('*')
                .is_some_and(|prefix| !prefix.is_empty() && normalized.starts_with(prefix))
                && provider_matches(entry.provider.as_ref())
        })
        .cloned()
}

/// Registry capability for a model: user entries first (exact > prefix), then
/// the embedded table (exact > prefix).
///
/// The returned `Option<bool>` is the *declared* vision state: `Some(true)`
/// when the entry explicitly lists image as a modality, `Some(false)` when the
/// entry explicitly excludes it, and `None` when the entry carries no modality
/// declaration (heuristics stay in charge).
fn registry_capability_for(
    model: &str,
    provider_hint: Option<&str>,
) -> Option<(ModelCapability, Option<bool>)> {
    if let Some(entry) = user_registry_entry_for(model, provider_hint) {
        let declared_vision = entry.vision.or_else(|| {
            (!entry.modalities.is_empty()).then(|| {
                entry
                    .modalities
                    .iter()
                    .any(|m| Modality::parse(m) == Some(Modality::Image))
            })
        });
        return Some((entry.to_model_capability(model), declared_vision));
    }
    registry_entry_for(model, provider_hint).map(|entry| {
        let capability = registry_capability(entry, model);
        let declared_vision =
            (!entry.modalities.is_empty()).then(|| entry.modalities.contains(&Modality::Image));
        (capability, declared_vision)
    })
}

fn registry_capability(entry: &RegistryEntry, model: &str) -> ModelCapability {
    let mut capability = ModelCapability {
        id: model.trim().to_string(),
        chat: entry.chat,
        modalities: vec![Modality::Text],
        tools: entry.tools,
        context_window: entry.context_window,
        output_window: entry.output_window,
        sampling: SamplingCapability {
            temperature_supported: entry.temperature_supported.unwrap_or(true),
            fixed_sampling: entry.fixed_sampling.unwrap_or(false),
            output_limit_field: entry.output_limit_field.map(str::to_string),
        },
        ..ModelCapability::default()
    };
    capability
        .modalities
        .extend(entry.modalities.iter().copied());
    capability.reasoning.protocol = entry.reasoning_protocol;
    capability.reasoning.efforts = entry
        .efforts
        .iter()
        .map(|effort| (*effort).to_string())
        .collect();
    capability.reasoning.default_effort = entry.default_effort.map(str::to_string);
    capability.reasoning.round_trip = entry.round_trip;
    capability.reasoning.tool_call_replay = entry.tool_call_replay;
    capability.reasoning.thinking_kind = entry.thinking_kind.map(str::to_string);
    capability
}

fn heuristic_reasoning_protocol(
    provider_hint: Option<&str>,
    model: &str,
) -> Option<ReasoningProtocol> {
    let provider = provider_hint.unwrap_or_default().to_ascii_lowercase();
    let m = model.to_ascii_lowercase();
    if provider.contains("deepseek") || m.contains("deepseek") {
        Some(ReasoningProtocol::DeepSeek)
    } else if provider.contains("anthropic")
        || provider.contains("claude")
        || m.starts_with("claude-")
    {
        Some(ReasoningProtocol::Anthropic)
    } else if provider.contains("minimax")
        || provider.contains("zhipu")
        || provider.contains("longcat")
        || provider.contains("bigmodel")
    {
        Some(ReasoningProtocol::ThinkingType)
    } else if provider.contains("openai")
        || m.starts_with("gpt-")
        || m.starts_with("o1")
        || m.starts_with("o3")
        || m.starts_with("o4")
        || m.starts_with("o5")
    {
        Some(ReasoningProtocol::OpenAi)
    } else {
        None
    }
}

fn heuristic_efforts(provider_hint: Option<&str>, model: &str) -> Vec<String> {
    crate::reasoning::inferred_reasoning_efforts(provider_hint, Some(model))
        .into_iter()
        .map(str::to_string)
        .collect()
}

fn heuristic_vision(model: &str, provider_hint: Option<&str>) -> bool {
    let m = model.to_ascii_lowercase();
    let provider = provider_hint.unwrap_or_default().to_ascii_lowercase();
    if provider.contains("anthropic") || provider.contains("claude") || m.starts_with("claude-") {
        return true;
    }
    m.starts_with("gpt-4o")
        || m.starts_with("gpt-5.6")
        || m.starts_with("k3")
        || m.contains("kimi-k3")
        || m.contains("kimi-k2.7")
        || m.contains("glm-5v")
        || m.contains("minimax-m3")
        || m.contains("qwen3.7-plus")
        || m.contains("qwen3.6-plus")
        || ["vl", "vision", "visual", "multimodal", "omni"]
            .iter()
            .any(|token| m.contains(token))
}

fn default_capability(model: &str) -> ModelCapability {
    ModelCapability {
        id: model.trim().to_string(),
        ..ModelCapability::default()
    }
}

/// Resolve the capability for a model with the standard heuristic set
/// (context via `models::context_limit_for_model_with_provider`).
pub fn resolve_capability(
    model: &str,
    provider_hint: Option<&str>,
    explicit: Option<&ExplicitModelCapability>,
) -> ResolvedModelCapability {
    resolve_capability_with_context_fn(model, provider_hint, explicit, |model, hint| {
        crate::models::context_limit_for_model_with_provider(model, hint)
    })
}

/// Resolve the capability for a model using a caller-supplied context
/// heuristic (used by `jcode-base` to feed the live catalog cache).
pub fn resolve_capability_with_context_fn<F>(
    model: &str,
    provider_hint: Option<&str>,
    explicit: Option<&ExplicitModelCapability>,
    context_fn: F,
) -> ResolvedModelCapability
where
    F: Fn(&str, Option<&str>) -> Option<usize>,
{
    let registry = registry_capability_for(model, provider_hint);
    let registry_capability = registry.as_ref().map(|(capability, _)| capability);
    let registry_vision = registry.as_ref().and_then(|(_, vision)| *vision);
    let mut capability = default_capability(model);
    let mut trace = CapabilityTrace::default();

    // --- context window: explicit > registry > heuristic > none ---
    if let Some(window) = explicit.and_then(|e| e.context_window) {
        capability.context_window = Some(window);
        trace.context_window = Some(CapabilitySource::ExplicitConfig);
    } else if let Some(window) = registry_capability.and_then(|r| r.context_window) {
        capability.context_window = Some(window);
        trace.context_window = Some(CapabilitySource::Registry);
    } else if let Some(window) = context_fn(model, provider_hint) {
        capability.context_window = Some(window);
        trace.context_window = Some(CapabilitySource::Heuristic);
    } else {
        trace.context_window = Some(CapabilitySource::Default);
    }

    // --- output window: explicit > registry ---
    if let Some(window) = explicit.and_then(|e| e.output_window) {
        capability.output_window = Some(window);
        trace.output_window = Some(CapabilitySource::ExplicitConfig);
    } else if let Some(window) = registry_capability.and_then(|r| r.output_window) {
        capability.output_window = Some(window);
        trace.output_window = Some(CapabilitySource::Registry);
    } else {
        trace.output_window = Some(CapabilitySource::Default);
    }

    // --- vision: explicit > explicit input modalities > registry > heuristic > default(false) ---
    let explicit_vision = explicit.and_then(|e| e.vision);
    let explicit_input_image = explicit
        .and_then(|e| e.input_modalities.as_ref())
        .is_some_and(|modalities| modalities.contains(&Modality::Image));
    if let Some(vision) = explicit_vision {
        set_image_modality(&mut capability, vision);
        trace.vision = Some(CapabilitySource::ExplicitConfig);
    } else if explicit_input_image {
        set_image_modality(&mut capability, true);
        trace.vision = Some(CapabilitySource::ExplicitConfig);
    } else if let Some(vision) = registry_vision {
        set_image_modality(&mut capability, vision);
        trace.vision = Some(CapabilitySource::Registry);
    } else if heuristic_vision(model, provider_hint) {
        set_image_modality(&mut capability, true);
        trace.vision = Some(CapabilitySource::Heuristic);
    } else {
        trace.vision = Some(CapabilitySource::Default);
    }

    // --- tools: explicit > registry > None (available by default) ---
    if let Some(tools) = explicit.and_then(|e| e.tools) {
        capability.tools = Some(tools);
        trace.tools = Some(CapabilitySource::ExplicitConfig);
    } else if let Some(tools) = registry_capability.and_then(|r| r.tools) {
        capability.tools = Some(tools);
        trace.tools = Some(CapabilitySource::Registry);
    } else {
        trace.tools = Some(CapabilitySource::Default);
    }

    // --- reasoning protocol: explicit > registry > heuristic ---
    if let Some(protocol) = explicit.and_then(|e| e.reasoning_protocol) {
        capability.reasoning.protocol = Some(protocol);
        trace.reasoning_protocol = Some(CapabilitySource::ExplicitConfig);
    } else if let Some(protocol) = registry_capability.and_then(|r| r.reasoning.protocol) {
        capability.reasoning.protocol = Some(protocol);
        trace.reasoning_protocol = Some(CapabilitySource::Registry);
    } else if let Some(protocol) = heuristic_reasoning_protocol(provider_hint, model) {
        capability.reasoning.protocol = Some(protocol);
        trace.reasoning_protocol = Some(CapabilitySource::Heuristic);
    }

    // --- efforts: explicit > registry > heuristic ---
    let explicit_efforts = explicit
        .and_then(|e| e.supported_efforts.as_ref())
        .filter(|efforts| !efforts.is_empty())
        .cloned();
    if let Some(efforts) = explicit_efforts {
        capability.reasoning.efforts = efforts;
        trace.efforts = Some(CapabilitySource::ExplicitConfig);
    } else if let Some(registry) = registry_capability
        && !registry.reasoning.efforts.is_empty()
    {
        capability.reasoning.efforts = registry.reasoning.efforts.clone();
        trace.efforts = Some(CapabilitySource::Registry);
    } else {
        let efforts = heuristic_efforts(provider_hint, model);
        if !efforts.is_empty() {
            capability.reasoning.efforts = efforts;
            trace.efforts = Some(CapabilitySource::Heuristic);
        }
    }

    // --- default effort: explicit > registry ---
    if let Some(effort) = explicit.and_then(|e| e.default_effort.as_ref()) {
        capability.reasoning.default_effort = Some(effort.clone());
    } else if let Some(effort) =
        registry_capability.and_then(|r| r.reasoning.default_effort.clone())
    {
        capability.reasoning.default_effort = Some(effort);
    }

    // --- round-trip flags from registry (not user-configurable) ---
    if let Some(registry) = registry_capability {
        capability.reasoning.round_trip = registry.reasoning.round_trip;
        capability.reasoning.tool_call_replay = registry.reasoning.tool_call_replay;
        capability.reasoning.thinking_kind = registry
            .reasoning
            .thinking_kind
            .clone()
            .or(capability.reasoning.thinking_kind);
    }

    // --- sampling: explicit > registry > defaults ---
    if let Some((temperature, fixed, output_limit)) = explicit.map(|e| {
        (
            e.temperature_supported,
            e.fixed_sampling,
            e.output_limit_field.clone(),
        )
    }) {
        if let Some(supported) = temperature {
            capability.sampling.temperature_supported = supported;
            trace.sampling = Some(CapabilitySource::ExplicitConfig);
        }
        if let Some(fixed) = fixed {
            capability.sampling.fixed_sampling = fixed;
            trace.sampling = Some(CapabilitySource::ExplicitConfig);
        }
        if let Some(field) = output_limit
            && !field.is_empty()
        {
            capability.sampling.output_limit_field = Some(field);
            trace.sampling = Some(CapabilitySource::ExplicitConfig);
        }
    }
    if trace.sampling.is_none()
        && let Some(registry) = registry_capability
        && (registry.sampling.fixed_sampling
            || !registry.sampling.temperature_supported
            || registry.sampling.output_limit_field.is_some())
    {
        capability.sampling = registry.sampling.clone();
        trace.sampling = Some(CapabilitySource::Registry);
    }

    ResolvedModelCapability { capability, trace }
}

fn set_image_modality(capability: &mut ModelCapability, enabled: bool) {
    capability
        .modalities
        .retain(|modality| *modality != Modality::Image);
    if enabled {
        capability.modalities.push(Modality::Image);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serializes tests that install/clear the process-wide user registry.
    static REGISTRY_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn modality_and_protocol_parse_round_trip() {
        assert_eq!(Modality::parse("image"), Some(Modality::Image));
        assert_eq!(Modality::parse("VISION"), Some(Modality::Image));
        assert_eq!(Modality::parse("audio"), Some(Modality::Audio));
        assert_eq!(Modality::parse("video"), None);
        assert_eq!(
            ReasoningProtocol::parse("deepseek"),
            Some(ReasoningProtocol::DeepSeek)
        );
        assert_eq!(
            ReasoningProtocol::parse("thinking_type"),
            Some(ReasoningProtocol::ThinkingType)
        );
        assert_eq!(
            ReasoningProtocol::parse("anthropic"),
            Some(ReasoningProtocol::Anthropic)
        );
        assert_eq!(ReasoningProtocol::parse("bogus"), None);
    }

    #[test]
    fn explicit_config_beats_registry_and_heuristic() {
        let resolved = resolve_capability(
            "deepseek-v4-pro",
            Some("deepseek"),
            Some(&ExplicitModelCapability {
                context_window: Some(777_000),
                tools: Some(false),
                ..ExplicitModelCapability::default()
            }),
        );
        assert_eq!(resolved.capability.context_window, Some(777_000));
        assert_eq!(resolved.capability.tools, Some(false));
        assert!(!resolved.capability.supports_tools());
        assert_eq!(
            resolved.trace.context_window,
            Some(CapabilitySource::ExplicitConfig)
        );
        assert_eq!(resolved.trace.tools, Some(CapabilitySource::ExplicitConfig));
        assert_eq!(
            resolved.capability.reasoning.protocol,
            Some(ReasoningProtocol::DeepSeek)
        );
    }

    #[test]
    fn registry_fills_known_model_capabilities() {
        let resolved = resolve_capability("kimi-k3", Some("openai-compatible:kimi"), None);
        assert_eq!(resolved.capability.context_window, Some(1_048_576));
        assert!(resolved.capability.supports_image());
        assert_eq!(resolved.capability.tools, Some(true));
        assert_eq!(
            resolved.capability.reasoning.default_effort.as_deref(),
            Some("max")
        );
        assert!(resolved.capability.reasoning.round_trip);
        assert!(resolved.capability.sampling.fixed_sampling);
        assert_eq!(
            resolved.capability.sampling.output_limit_field.as_deref(),
            Some("max_completion_tokens")
        );
        assert_eq!(
            resolved.trace.context_window,
            Some(CapabilitySource::Registry)
        );
        assert_eq!(resolved.trace.vision, Some(CapabilitySource::Registry));
    }

    #[test]
    fn registry_prefix_matches_claude_family() {
        let resolved = resolve_capability("claude-opus-4-8", Some("claude"), None);
        assert_eq!(resolved.capability.tools, Some(true));
        assert!(resolved.capability.supports_image());
        assert_eq!(
            resolved.capability.reasoning.protocol,
            Some(ReasoningProtocol::Anthropic)
        );
    }

    #[test]
    fn registry_prefix_matches_o_series_and_gemini() {
        let o3 = resolve_capability("o3-mini", Some("openai"), None);
        assert_eq!(o3.capability.tools, Some(true));
        assert_eq!(
            o3.capability.reasoning.protocol,
            Some(ReasoningProtocol::OpenAi)
        );
        assert!(!o3.capability.reasoning.efforts.is_empty());

        let gemini = resolve_capability("gemini-2.5-pro", Some("gemini"), None);
        assert_eq!(gemini.capability.tools, Some(true));
        assert!(gemini.capability.supports_image());
        assert_eq!(gemini.trace.vision, Some(CapabilitySource::Registry));
    }

    #[test]
    fn heuristic_provides_context_for_unknown_open_weight_models() {
        let resolved = resolve_capability("glm-4.7", Some("zai"), None);
        assert_eq!(resolved.capability.context_window, Some(200_000));
        assert_eq!(
            resolved.trace.context_window,
            Some(CapabilitySource::Heuristic)
        );
    }

    #[test]
    fn explicit_input_image_enables_vision() {
        let resolved = resolve_capability(
            "custom-vlm",
            Some("openai-compatible:custom"),
            Some(&ExplicitModelCapability {
                input_modalities: Some(vec![Modality::Image]),
                ..ExplicitModelCapability::default()
            }),
        );
        assert!(resolved.capability.supports_image());
        assert_eq!(
            resolved.trace.vision,
            Some(CapabilitySource::ExplicitConfig)
        );
    }

    #[test]
    fn conservative_defaults_for_unknown_model() {
        let resolved = resolve_capability("totally-unknown-model", Some("my-gateway"), None);
        assert!(resolved.capability.chat);
        assert!(!resolved.capability.supports_image());
        assert!(resolved.capability.supports_tools());
        assert_eq!(resolved.capability.reasoning.protocol, None);
        assert_eq!(resolved.trace.vision, Some(CapabilitySource::Default));
        assert!(
            resolved.route_view().is_none(),
            "default view must be omitted from routes"
        );
    }

    #[test]
    fn route_view_is_non_default_when_any_field_is_known() {
        let resolved = resolve_capability("kimi-k3", Some("openai-compatible:kimi"), None);
        let view = resolved.route_view().expect("k3 has declared capabilities");
        assert!(view.modalities.contains(&"image".to_string()));
        assert_eq!(view.tools, Some(true));
        assert_eq!(view.context_window, Some(1_048_576));
        assert_eq!(view.reasoning_protocol.as_deref(), Some("openai"));
    }

    #[test]
    fn sampling_view_omits_defaults() {
        let view = SamplingView::from(SamplingCapability::default());
        assert!(view.is_empty());
        let view = SamplingView::from(SamplingCapability {
            temperature_supported: false,
            fixed_sampling: true,
            output_limit_field: Some("max_completion_tokens".to_string()),
        });
        assert!(!view.is_empty());
    }

    #[test]
    fn user_registry_entries_win_over_embedded_and_heuristics() {
        let _guard = REGISTRY_TEST_LOCK.lock().unwrap();
        set_user_registry_entries(vec![UserRegistryEntry {
            model: "deepseek-v4-pro".to_string(),
            tools: Some(false),
            context_window: Some(777_000),
            ..UserRegistryEntry::default()
        }]);

        let resolved = resolve_capability("deepseek-v4-pro", Some("deepseek"), None);
        assert_eq!(resolved.capability.tools, Some(false));
        assert!(!resolved.capability.supports_tools());
        assert_eq!(resolved.capability.context_window, Some(777_000));
        assert_eq!(resolved.trace.tools, Some(CapabilitySource::Registry));
        assert_eq!(
            resolved.trace.context_window,
            Some(CapabilitySource::Registry)
        );

        clear_user_registry_entries();
    }

    #[test]
    fn user_registry_wildcard_and_provider_scope() {
        let _guard = REGISTRY_TEST_LOCK.lock().unwrap();
        set_user_registry_entries(vec![
            UserRegistryEntry {
                model: "acme-*".to_string(),
                provider: Some("acme-gateway".to_string()),
                vision: Some(true),
                tools: Some(false),
                context_window: Some(64_000),
                ..UserRegistryEntry::default()
            },
            UserRegistryEntry {
                model: "acme-*".to_string(),
                provider: Some("other-gateway".to_string()),
                vision: Some(false),
                ..UserRegistryEntry::default()
            },
        ]);

        let scoped = resolve_capability("acme-vlm", Some("acme-gateway"), None);
        assert!(scoped.capability.supports_image());
        assert!(!scoped.capability.supports_tools());
        assert_eq!(scoped.capability.context_window, Some(64_000));

        let other = resolve_capability("acme-vlm", Some("other-gateway"), None);
        assert!(!other.capability.supports_image());

        let unscoped = resolve_capability("acme-vlm", Some("unrelated"), None);
        assert_eq!(
            unscoped.trace.vision,
            Some(CapabilitySource::Heuristic),
            "provider-scoped user entries must not leak to other providers"
        );
        assert!(
            unscoped.capability.supports_image(),
            "without a matching user entry the vision heuristic still applies"
        );

        clear_user_registry_entries();
    }

    #[test]
    fn user_registry_does_not_shadow_explicit_config() {
        let _guard = REGISTRY_TEST_LOCK.lock().unwrap();
        set_user_registry_entries(vec![UserRegistryEntry {
            model: "custom-vlm".to_string(),
            vision: Some(false),
            tools: Some(false),
            ..UserRegistryEntry::default()
        }]);

        let resolved = resolve_capability(
            "custom-vlm",
            Some("gateway"),
            Some(&ExplicitModelCapability {
                vision: Some(true),
                tools: Some(true),
                ..ExplicitModelCapability::default()
            }),
        );
        assert!(resolved.capability.supports_image());
        assert!(resolved.capability.supports_tools());
        assert_eq!(
            resolved.trace.vision,
            Some(CapabilitySource::ExplicitConfig)
        );
        assert_eq!(resolved.trace.tools, Some(CapabilitySource::ExplicitConfig));

        clear_user_registry_entries();
    }
}
