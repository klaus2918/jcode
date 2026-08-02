use super::{
    LoginProviderAuthKind, LoginProviderAuthStateKey, LoginProviderDescriptor,
    LoginProviderSurfaceOrder, LoginProviderTarget, OpenAiCompatibleProfile,
};

// OpenRouter also has a dedicated provider implementation elsewhere, but it
// speaks the standard OpenAI-compatible /api/v1 endpoint, so it can be driven
// by `provider-doctor` / `provider-test-coverage` like any other
// OpenAI-compatible provider. `default_model` is None so the doctor selects the
// live catalog's first model unless `--model` is passed.
pub const OPENROUTER_OPENAI_COMPAT_PROFILE: OpenAiCompatibleProfile = OpenAiCompatibleProfile {
    id: "openrouter",
    display_name: "OpenRouter",
    api_base: "https://openrouter.ai/api/v1",
    api_key_env: "OPENROUTER_API_KEY",
    env_file: "openrouter.env",
    setup_url: "https://openrouter.ai/keys",
    default_model: None,
    requires_api_key: true,
};

// Anthropic and OpenAI also expose OpenAI-compatible `/v1/chat/completions`
// endpoints, so they can be driven by `provider-doctor` /
// `provider-test-coverage` as OpenAI-compatible profiles. These profile ids
// alias the native login-provider ids (`anthropic-api`, `openai-api`); auth
// activation deliberately routes them through the native runtime, while the
// live HTTP probes hit these hosts (Anthropic needs `x-api-key` +
// `anthropic-version`, handled in the probe layer). `default_model` is None so
// the doctor selects from the live catalog unless `--model` is passed.
pub const ANTHROPIC_OPENAI_COMPAT_PROFILE: OpenAiCompatibleProfile = OpenAiCompatibleProfile {
    id: "anthropic-api",
    display_name: "Anthropic API",
    api_base: "https://api.anthropic.com/v1",
    api_key_env: "ANTHROPIC_API_KEY",
    env_file: "anthropic.env",
    setup_url: "https://docs.anthropic.com/en/api/openai-sdk",
    default_model: None,
    requires_api_key: true,
};

pub const OPENAI_NATIVE_OPENAI_COMPAT_PROFILE: OpenAiCompatibleProfile = OpenAiCompatibleProfile {
    id: "openai-api",
    display_name: "OpenAI API",
    api_base: "https://api.openai.com/v1",
    api_key_env: "OPENAI_API_KEY",
    env_file: "openai.env",
    setup_url: "https://platform.openai.com/api-keys",
    default_model: None,
    requires_api_key: true,
};

pub const GEMINI_OPENAI_COMPAT_PROFILE: OpenAiCompatibleProfile = OpenAiCompatibleProfile {
    id: "gemini-api",
    display_name: "Gemini API",
    // Google's official OpenAI-compatible surface for the Gemini Developer API.
    // The `/models` endpoint here returns `models/`-prefixed ids, which the live
    // probe layer normalizes back to bare model names.
    api_base: "https://generativelanguage.googleapis.com/v1beta/openai",
    api_key_env: "GEMINI_API_KEY",
    env_file: "gemini.env",
    setup_url: "https://ai.google.dev/gemini-api/docs/openai",
    default_model: Some("gemini-2.5-flash"),
    requires_api_key: true,
};

pub const LMSTUDIO_PROFILE: OpenAiCompatibleProfile = OpenAiCompatibleProfile {
    id: "lmstudio",
    display_name: "LM Studio",
    api_base: "http://localhost:1234/v1",
    api_key_env: "LMSTUDIO_API_KEY",
    env_file: "lmstudio.env",
    setup_url: "https://lmstudio.ai/docs/app/api/endpoints/openai",
    default_model: None,
    requires_api_key: false,
};

pub const OLLAMA_PROFILE: OpenAiCompatibleProfile = OpenAiCompatibleProfile {
    id: "ollama",
    display_name: "Ollama",
    api_base: "http://localhost:11434/v1",
    api_key_env: "OLLAMA_API_KEY",
    env_file: "ollama.env",
    setup_url: "https://docs.ollama.com/api/openai-compatibility",
    default_model: None,
    requires_api_key: false,
};

pub const OPENAI_COMPAT_PROFILE: OpenAiCompatibleProfile = OpenAiCompatibleProfile {
    id: "openai-compatible",
    display_name: "OpenAI-compatible",
    api_base: "https://api.openai.com/v1",
    api_key_env: "OPENAI_COMPAT_API_KEY",
    env_file: "openai-compatible.env",
    setup_url: "https://github.com/1jehuang/jcode#openai-compatible-providers",
    default_model: None,
    requires_api_key: true,
};

pub(crate) const OPENAI_COMPAT_PROFILES: [OpenAiCompatibleProfile; 7] = [
    OPENROUTER_OPENAI_COMPAT_PROFILE,
    ANTHROPIC_OPENAI_COMPAT_PROFILE,
    OPENAI_NATIVE_OPENAI_COMPAT_PROFILE,
    GEMINI_OPENAI_COMPAT_PROFILE,
    LMSTUDIO_PROFILE,
    OLLAMA_PROFILE,
    OPENAI_COMPAT_PROFILE,
];

pub const CLAUDE_LOGIN_PROVIDER: LoginProviderDescriptor = LoginProviderDescriptor {
    id: "claude",
    display_name: "Anthropic/Claude",
    auth_kind: LoginProviderAuthKind::OAuth,
    auth_state_key: LoginProviderAuthStateKey::Anthropic,
    auth_status_method: "OAuth",
    aliases: &["anthropic"],
    menu_detail: "requires Claude Pro or Max subscription",
    recommended: true,
    target: LoginProviderTarget::Claude,
    order: LoginProviderSurfaceOrder::new(Some(1), Some(1), Some(1), Some(1), Some(1)),
};

pub const ANTHROPIC_API_LOGIN_PROVIDER: LoginProviderDescriptor = LoginProviderDescriptor {
    id: "anthropic-api",
    display_name: "Anthropic API",
    auth_kind: LoginProviderAuthKind::ApiKey,
    auth_state_key: LoginProviderAuthStateKey::Anthropic,
    auth_status_method: "API key",
    aliases: &["claude-api", "anthropic-key", "claude-key"],
    menu_detail: "direct Anthropic Messages API",
    recommended: false,
    target: LoginProviderTarget::ClaudeApiKey,
    order: LoginProviderSurfaceOrder::new(Some(2), Some(2), Some(2), Some(2), Some(2)),
};

pub const AUTO_IMPORT_LOGIN_PROVIDER: LoginProviderDescriptor = LoginProviderDescriptor {
    id: "auto-import",
    display_name: "Auto Import",
    auth_kind: LoginProviderAuthKind::Local,
    auth_state_key: LoginProviderAuthStateKey::ExternalImport,
    auth_status_method: "Reuse detected logins",
    aliases: &["import", "reuse", "autoimport"],
    menu_detail: "review and reuse logins from other tools",
    recommended: false,
    target: LoginProviderTarget::AutoImport,
    order: LoginProviderSurfaceOrder::new(Some(1), Some(1), None, None, None),
};

pub const JCODE_LOGIN_PROVIDER: LoginProviderDescriptor = LoginProviderDescriptor {
    id: "jcode",
    display_name: "Jcode Subscription",
    auth_kind: LoginProviderAuthKind::ApiKey,
    auth_state_key: LoginProviderAuthStateKey::Jcode,
    auth_status_method: "API key",
    aliases: &["subscription", "jcode-subscription"],
    menu_detail: "curated jcode subscription models",
    recommended: false,
    target: LoginProviderTarget::Jcode,
    order: LoginProviderSurfaceOrder::new(Some(3), Some(3), Some(3), Some(3), Some(3)),
};

pub const OPENAI_LOGIN_PROVIDER: LoginProviderDescriptor = LoginProviderDescriptor {
    id: "openai",
    display_name: "OpenAI",
    auth_kind: LoginProviderAuthKind::OAuth,
    auth_state_key: LoginProviderAuthStateKey::OpenAi,
    auth_status_method: "OAuth",
    aliases: &[],
    menu_detail: "requires ChatGPT Plus or Pro subscription",
    recommended: true,
    target: LoginProviderTarget::OpenAi,
    order: LoginProviderSurfaceOrder::new(Some(2), Some(2), Some(2), Some(2), Some(2)),
};

pub const OPENAI_API_LOGIN_PROVIDER: LoginProviderDescriptor = LoginProviderDescriptor {
    id: "openai-api",
    display_name: "OpenAI API",
    auth_kind: LoginProviderAuthKind::ApiKey,
    auth_state_key: LoginProviderAuthStateKey::OpenAi,
    auth_status_method: "API key",
    aliases: &[
        "openai-key",
        "openai-apikey",
        "openai-platform",
        "platform-openai",
    ],
    menu_detail: "native OpenAI API key, pay-per-token",
    recommended: false,
    target: LoginProviderTarget::OpenAiApiKey,
    order: LoginProviderSurfaceOrder::new(Some(99), Some(99), Some(99), Some(99), Some(99)),
};

pub const OPENROUTER_LOGIN_PROVIDER: LoginProviderDescriptor = LoginProviderDescriptor {
    id: "openrouter",
    display_name: "OpenRouter",
    auth_kind: LoginProviderAuthKind::ApiKey,
    auth_state_key: LoginProviderAuthStateKey::OpenRouterLike,
    auth_status_method: "API key",
    aliases: &[],
    menu_detail: "API key, pay-per-token, 200+ models",
    recommended: false,
    target: LoginProviderTarget::OpenRouter,
    order: LoginProviderSurfaceOrder::new(Some(4), Some(3), Some(4), Some(3), Some(3)),
};

pub const BEDROCK_LOGIN_PROVIDER: LoginProviderDescriptor = LoginProviderDescriptor {
    id: "bedrock",
    display_name: "AWS Bedrock",
    auth_kind: LoginProviderAuthKind::ApiKey,
    auth_state_key: LoginProviderAuthStateKey::Bedrock,
    auth_status_method: "API key / AWS credentials",
    aliases: &["aws-bedrock", "aws_bedrock"],
    menu_detail: "Bedrock API key or AWS credentials, pay-per-token",
    recommended: false,
    target: LoginProviderTarget::Bedrock,
    order: LoginProviderSurfaceOrder::new(Some(5), Some(4), None, None, Some(4)),
};

pub const AZURE_LOGIN_PROVIDER: LoginProviderDescriptor = LoginProviderDescriptor {
    id: "azure",
    display_name: "Azure OpenAI",
    auth_kind: LoginProviderAuthKind::Hybrid,
    auth_state_key: LoginProviderAuthStateKey::Azure,
    auth_status_method: "Entra ID / API key",
    aliases: &["azure-openai", "azure_openai", "aoai"],
    menu_detail: "Microsoft Entra ID or Azure OpenAI API key",
    recommended: false,
    target: LoginProviderTarget::Azure,
    order: LoginProviderSurfaceOrder::new(Some(5), Some(5), None, None, Some(4)),
};

pub const LMSTUDIO_LOGIN_PROVIDER: LoginProviderDescriptor = LoginProviderDescriptor {
    id: "lmstudio",
    display_name: "LM Studio",
    auth_kind: LoginProviderAuthKind::Local,
    auth_state_key: LoginProviderAuthStateKey::OpenRouterLike,
    auth_status_method: "local endpoint",
    aliases: &["lm-studio"],
    menu_detail: "local OpenAI-compatible endpoint",
    recommended: false,
    target: LoginProviderTarget::OpenAiCompatible(LMSTUDIO_PROFILE),
    order: LoginProviderSurfaceOrder::new(Some(34), Some(34), Some(34), Some(34), Some(34)),
};

pub const OLLAMA_LOGIN_PROVIDER: LoginProviderDescriptor = LoginProviderDescriptor {
    id: "ollama",
    display_name: "Ollama",
    auth_kind: LoginProviderAuthKind::Local,
    auth_state_key: LoginProviderAuthStateKey::OpenRouterLike,
    auth_status_method: "local endpoint",
    aliases: &[],
    menu_detail: "local OpenAI-compatible endpoint",
    recommended: false,
    target: LoginProviderTarget::OpenAiCompatible(OLLAMA_PROFILE),
    order: LoginProviderSurfaceOrder::new(Some(35), Some(35), Some(35), Some(35), Some(35)),
};

pub const OPENAI_COMPAT_LOGIN_PROVIDER: LoginProviderDescriptor = LoginProviderDescriptor {
    id: "openai-compatible",
    display_name: "OpenAI-compatible",
    auth_kind: LoginProviderAuthKind::Hybrid,
    auth_state_key: LoginProviderAuthStateKey::OpenRouterLike,
    auth_status_method: "API key / local endpoint",
    aliases: &["openai_compatible", "compat", "custom"],
    menu_detail: "custom endpoint setup: base URL first, then API key",
    recommended: false,
    target: LoginProviderTarget::OpenAiCompatible(OPENAI_COMPAT_PROFILE),
    order: LoginProviderSurfaceOrder::new(Some(10), Some(9), None, None, Some(9)),
};

pub const CURSOR_LOGIN_PROVIDER: LoginProviderDescriptor = LoginProviderDescriptor {
    id: "cursor",
    display_name: "Cursor",
    auth_kind: LoginProviderAuthKind::Hybrid,
    auth_state_key: LoginProviderAuthStateKey::Cursor,
    auth_status_method: "API key / CLI",
    aliases: &[],
    menu_detail: "browser login or API key",
    recommended: false,
    target: LoginProviderTarget::Cursor,
    order: LoginProviderSurfaceOrder::new(Some(11), Some(12), None, Some(9), Some(12)),
};

pub const COPILOT_LOGIN_PROVIDER: LoginProviderDescriptor = LoginProviderDescriptor {
    id: "copilot",
    display_name: "GitHub Copilot",
    auth_kind: LoginProviderAuthKind::DeviceCode,
    auth_state_key: LoginProviderAuthStateKey::Copilot,
    auth_status_method: "device code",
    aliases: &[],
    menu_detail: "GitHub device flow",
    recommended: false,
    target: LoginProviderTarget::Copilot,
    order: LoginProviderSurfaceOrder::new(Some(3), Some(10), Some(3), Some(10), Some(10)),
};

pub const GEMINI_LOGIN_PROVIDER: LoginProviderDescriptor = LoginProviderDescriptor {
    id: "gemini",
    display_name: "Google Gemini",
    auth_kind: LoginProviderAuthKind::OAuth,
    auth_state_key: LoginProviderAuthStateKey::Gemini,
    auth_status_method: "OAuth",
    aliases: &[],
    menu_detail: "Google Gemini Code Assist OAuth login",
    recommended: false,
    target: LoginProviderTarget::Gemini,
    order: LoginProviderSurfaceOrder::new(Some(13), Some(11), Some(4), Some(11), Some(13)),
};

pub const GEMINI_API_LOGIN_PROVIDER: LoginProviderDescriptor = LoginProviderDescriptor {
    id: "gemini-api",
    display_name: "Gemini API",
    auth_kind: LoginProviderAuthKind::ApiKey,
    auth_state_key: LoginProviderAuthStateKey::OpenRouterLike,
    auth_status_method: "API key",
    aliases: &[
        "gemini-key",
        "gemini-apikey",
        "google-ai-studio",
        "ai-studio",
    ],
    menu_detail: "Google AI Studio Developer API key (OpenAI-compatible)",
    recommended: false,
    target: LoginProviderTarget::OpenAiCompatible(GEMINI_OPENAI_COMPAT_PROFILE),
    order: LoginProviderSurfaceOrder::new(Some(38), Some(38), Some(38), Some(38), Some(38)),
};

pub const ANTIGRAVITY_LOGIN_PROVIDER: LoginProviderDescriptor = LoginProviderDescriptor {
    id: "antigravity",
    display_name: "Antigravity",
    auth_kind: LoginProviderAuthKind::OAuth,
    auth_state_key: LoginProviderAuthStateKey::Antigravity,
    auth_status_method: "OAuth",
    aliases: &[],
    menu_detail: "Google Antigravity OAuth login",
    recommended: false,
    target: LoginProviderTarget::Antigravity,
    order: LoginProviderSurfaceOrder::new(Some(12), Some(12), None, Some(12), Some(12)),
};

pub const GOOGLE_LOGIN_PROVIDER: LoginProviderDescriptor = LoginProviderDescriptor {
    id: "google",
    display_name: "Google/Gmail",
    auth_kind: LoginProviderAuthKind::OAuth,
    auth_state_key: LoginProviderAuthStateKey::Google,
    auth_status_method: "OAuth",
    aliases: &["gmail"],
    menu_detail: "read, draft, and send emails",
    recommended: false,
    target: LoginProviderTarget::Google,
    order: LoginProviderSurfaceOrder::new(Some(13), None, None, None, None),
};

pub(crate) const LOGIN_PROVIDERS: [LoginProviderDescriptor; 18] = [
    AUTO_IMPORT_LOGIN_PROVIDER,
    CLAUDE_LOGIN_PROVIDER,
    ANTHROPIC_API_LOGIN_PROVIDER,
    OPENAI_LOGIN_PROVIDER,
    OPENAI_API_LOGIN_PROVIDER,
    JCODE_LOGIN_PROVIDER,
    OPENROUTER_LOGIN_PROVIDER,
    BEDROCK_LOGIN_PROVIDER,
    AZURE_LOGIN_PROVIDER,
    LMSTUDIO_LOGIN_PROVIDER,
    OLLAMA_LOGIN_PROVIDER,
    OPENAI_COMPAT_LOGIN_PROVIDER,
    CURSOR_LOGIN_PROVIDER,
    COPILOT_LOGIN_PROVIDER,
    GEMINI_LOGIN_PROVIDER,
    GEMINI_API_LOGIN_PROVIDER,
    ANTIGRAVITY_LOGIN_PROVIDER,
    GOOGLE_LOGIN_PROVIDER,
];
