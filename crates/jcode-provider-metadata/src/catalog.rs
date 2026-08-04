use super::{
    LoginProviderAuthKind, LoginProviderAuthStateKey, LoginProviderDescriptor,
    LoginProviderSurfaceOrder, LoginProviderTarget, OpenAiCompatibleProfile,
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

pub(crate) const OPENAI_COMPAT_PROFILES: [OpenAiCompatibleProfile; 3] = [
    LMSTUDIO_PROFILE,
    OLLAMA_PROFILE,
    OPENAI_COMPAT_PROFILE,
];

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

// Config-driven (Reasonix-aligned) login surface. Built-in third-party /
// first-party vendors (Claude/OpenAI/OpenRouter/Bedrock/Azure/Google/Gemini)
// are intentionally absent: models are connected via `[[providers]]` config
// entries and an `openai-compatible` endpoint, not interactive login. The
// generic openai-compatible entry, local endpoints (LM Studio / Ollama),
// external-auth import, and the jcode subscription remain.
pub(crate) const LOGIN_PROVIDERS: [LoginProviderDescriptor; 5] = [
    AUTO_IMPORT_LOGIN_PROVIDER,
    JCODE_LOGIN_PROVIDER,
    LMSTUDIO_LOGIN_PROVIDER,
    OLLAMA_LOGIN_PROVIDER,
    OPENAI_COMPAT_LOGIN_PROVIDER,
];
