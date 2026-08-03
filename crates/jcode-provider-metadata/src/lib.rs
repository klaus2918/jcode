#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginProviderAuthKind {
    OAuth,
    ApiKey,
    DeviceCode,
    Cli,
    Hybrid,
    Local,
}

impl LoginProviderAuthKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::OAuth => "OAuth",
            Self::ApiKey => "API key",
            Self::DeviceCode => "device code",
            Self::Cli => "CLI",
            Self::Hybrid => "API key / CLI",
            Self::Local => "local endpoint",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginProviderTarget {
    AutoImport,
    Jcode,
    Claude,
    ClaudeApiKey,
    OpenAi,
    OpenAiApiKey,
    OpenRouter,
    Bedrock,
    Azure,
    OpenAiCompatible(OpenAiCompatibleProfile),
    Google,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginProviderAuthStateKey {
    ExternalImport,
    Jcode,
    Anthropic,
    OpenAi,
    Azure,
    Bedrock,
    OpenRouterLike,
    Google,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginProviderSurface {
    CliLogin,
    TuiLogin,
    ServerBootstrap,
    AutoInit,
    AuthStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoginProviderSurfaceOrder {
    pub cli_login: Option<u8>,
    pub tui_login: Option<u8>,
    pub server_bootstrap: Option<u8>,
    pub auto_init: Option<u8>,
    pub auth_status: Option<u8>,
}

impl LoginProviderSurfaceOrder {
    pub const fn new(
        cli_login: Option<u8>,
        tui_login: Option<u8>,
        server_bootstrap: Option<u8>,
        auto_init: Option<u8>,
        auth_status: Option<u8>,
    ) -> Self {
        Self {
            cli_login,
            tui_login,
            server_bootstrap,
            auto_init,
            auth_status,
        }
    }

    pub const fn for_surface(self, surface: LoginProviderSurface) -> Option<u8> {
        match surface {
            LoginProviderSurface::CliLogin => self.cli_login,
            LoginProviderSurface::TuiLogin => self.tui_login,
            LoginProviderSurface::ServerBootstrap => self.server_bootstrap,
            LoginProviderSurface::AutoInit => self.auto_init,
            LoginProviderSurface::AuthStatus => self.auth_status,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoginProviderDescriptor {
    pub id: &'static str,
    pub display_name: &'static str,
    pub auth_kind: LoginProviderAuthKind,
    pub auth_state_key: LoginProviderAuthStateKey,
    pub auth_status_method: &'static str,
    pub aliases: &'static [&'static str],
    pub menu_detail: &'static str,
    pub recommended: bool,
    pub target: LoginProviderTarget,
    pub order: LoginProviderSurfaceOrder,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpenAiCompatibleProfile {
    pub id: &'static str,
    pub display_name: &'static str,
    pub api_base: &'static str,
    pub api_key_env: &'static str,
    pub env_file: &'static str,
    pub setup_url: &'static str,
    pub default_model: Option<&'static str>,
    pub requires_api_key: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedOpenAiCompatibleProfile {
    pub id: String,
    pub display_name: String,
    pub api_base: String,
    pub api_key_env: String,
    pub env_file: String,
    pub setup_url: String,
    pub default_model: Option<String>,
    pub requires_api_key: bool,
}

mod catalog;

pub use catalog::*;
use catalog::{LOGIN_PROVIDERS, OPENAI_COMPAT_PROFILES};

pub fn openai_compatible_profiles() -> &'static [OpenAiCompatibleProfile] {
    &OPENAI_COMPAT_PROFILES
}

pub fn login_providers() -> &'static [LoginProviderDescriptor] {
    &LOGIN_PROVIDERS
}

fn login_providers_for_surface(surface: LoginProviderSurface) -> Vec<LoginProviderDescriptor> {
    let mut providers = login_providers()
        .iter()
        .copied()
        .filter(|provider| provider.order.for_surface(surface).is_some())
        .collect::<Vec<_>>();
    providers.sort_by_key(|provider| provider.order.for_surface(surface).unwrap_or(u8::MAX));
    providers
}

pub fn cli_login_providers() -> Vec<LoginProviderDescriptor> {
    login_providers_for_surface(LoginProviderSurface::CliLogin)
}

pub fn tui_login_providers() -> Vec<LoginProviderDescriptor> {
    login_providers_for_surface(LoginProviderSurface::TuiLogin)
}

pub fn server_bootstrap_login_providers() -> Vec<LoginProviderDescriptor> {
    login_providers_for_surface(LoginProviderSurface::ServerBootstrap)
}

pub fn auto_init_login_providers() -> Vec<LoginProviderDescriptor> {
    login_providers_for_surface(LoginProviderSurface::AutoInit)
}

pub fn auth_status_login_providers() -> Vec<LoginProviderDescriptor> {
    login_providers_for_surface(LoginProviderSurface::AuthStatus)
}

pub fn resolve_login_provider(input: &str) -> Option<LoginProviderDescriptor> {
    let normalized = normalize_provider_input(input)?;
    login_providers().iter().copied().find(|provider| {
        provider.id == normalized || provider.aliases.iter().any(|alias| *alias == normalized)
    })
}

/// Resolve a login provider by id, alias, or display name.
///
/// Login completion events carry the human-readable provider label (e.g.
/// "Anthropic API") rather than the canonical id/alias, so the stricter
/// [`resolve_login_provider`] (id/alias only) misses them. Auth-change routing
/// needs to map those labels back to a provider id; matching the display name
/// here keeps the post-login model refresh attributed to the correct provider.
pub fn resolve_login_provider_loose(input: &str) -> Option<LoginProviderDescriptor> {
    if let Some(provider) = resolve_login_provider(input) {
        return Some(provider);
    }
    let normalized = normalize_provider_input(input)?;
    login_providers()
        .iter()
        .copied()
        .find(|provider| provider.display_name.to_ascii_lowercase() == normalized)
}

pub fn resolve_login_selection(
    input: &str,
    providers: &[LoginProviderDescriptor],
) -> Option<LoginProviderDescriptor> {
    let trimmed = input.trim();
    if let Ok(index) = trimmed.parse::<usize>() {
        return index
            .checked_sub(1)
            .and_then(|idx| providers.get(idx))
            .copied();
    }

    let provider = resolve_login_provider(trimmed)?;
    providers
        .iter()
        .copied()
        .find(|candidate| candidate.id == provider.id)
}

pub fn is_safe_env_key_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

pub fn is_safe_env_file_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('/')
        && !name.contains('\\')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
}

pub fn normalize_api_base(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let parsed = url::Url::parse(trimmed).ok()?;
    let scheme = parsed.scheme();
    if scheme != "https" && scheme != "http" {
        return None;
    }

    if scheme == "http" {
        let host = parsed.host_str()?;
        if !allows_insecure_http_host(host) {
            return None;
        }
    }

    Some(trimmed.trim_end_matches('/').to_string())
}

/// Like [`normalize_api_base`], but accepts any `http://` host (not just
/// localhost/private-LAN). Used for user-configured named provider profiles,
/// which are explicit endpoint choices (mirroring how tools like Reasonix
/// accept arbitrary gateway URLs). HTTPS validation is unchanged.
pub fn normalize_api_base_relaxed(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let parsed = url::Url::parse(trimmed).ok()?;
    let scheme = parsed.scheme();
    if scheme != "https" && scheme != "http" {
        return None;
    }

    Some(trimmed.trim_end_matches('/').to_string())
}

fn allows_insecure_http_host(host: &str) -> bool {
    let host = host.trim();
    let host = host
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(host);
    let host_lower = host.to_ascii_lowercase();
    if host_lower == "localhost" || host_lower.ends_with(".local") {
        return true;
    }

    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return match ip {
            std::net::IpAddr::V4(v4) => {
                let raw = u32::from(v4);
                let is_carrier_grade_nat = (raw & 0xffc0_0000) == 0x6440_0000;
                v4.is_loopback()
                    || v4.is_private()
                    || v4.is_link_local()
                    || v4.is_unspecified()
                    || is_carrier_grade_nat
            }
            std::net::IpAddr::V6(v6) => {
                v6.is_loopback()
                    || v6.is_unique_local()
                    || v6.is_unicast_link_local()
                    || v6.is_unspecified()
            }
        };
    }

    false
}

fn normalize_provider_input(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn matrix_profiles_have_unique_ids_and_safe_metadata() {
        let mut ids = HashSet::new();
        for profile in openai_compatible_profiles() {
            assert!(
                ids.insert(profile.id),
                "duplicate provider profile id: {}",
                profile.id
            );
            assert!(is_safe_env_key_name(profile.api_key_env));
            assert!(is_safe_env_file_name(profile.env_file));
            assert_eq!(
                normalize_api_base(profile.api_base).as_deref(),
                Some(profile.api_base)
            );
        }
    }

    #[test]
    fn normalize_api_base_accepts_private_http_hosts() {
        assert_eq!(
            normalize_api_base("http://192.168.1.25:8000/v1/").as_deref(),
            Some("http://192.168.1.25:8000/v1")
        );
        assert_eq!(
            normalize_api_base("http://10.0.0.8:11434/v1").as_deref(),
            Some("http://10.0.0.8:11434/v1")
        );
        assert_eq!(
            normalize_api_base("http://100.103.78.84:11434/v1").as_deref(),
            Some("http://100.103.78.84:11434/v1")
        );
        assert_eq!(
            normalize_api_base("http://hsv.local:11434/v1").as_deref(),
            Some("http://hsv.local:11434/v1")
        );
        assert_eq!(
            normalize_api_base("http://[fd00::1]:8080/v1").as_deref(),
            Some("http://[fd00::1]:8080/v1")
        );
    }

    #[test]
    fn normalize_api_base_rejects_public_http_hosts() {
        assert_eq!(normalize_api_base("http://example.com/v1"), None);
        assert_eq!(normalize_api_base("http://8.8.8.8/v1"), None);
    }

    #[test]
    fn normalize_api_base_relaxed_accepts_public_http_gateways() {
        // Named provider profiles are explicit user endpoint choices, so
        // arbitrary http:// gateways are allowed (https unchanged).
        assert_eq!(
            normalize_api_base_relaxed("http://gateway.example.com").as_deref(),
            Some("http://gateway.example.com")
        );
        assert_eq!(
            normalize_api_base_relaxed("http://gateway.example.com/v1/").as_deref(),
            Some("http://gateway.example.com/v1")
        );
        assert_eq!(
            normalize_api_base_relaxed("https://gateway.example.com/v1").as_deref(),
            Some("https://gateway.example.com/v1")
        );
        // Non-http(s) schemes are still rejected.
        assert_eq!(normalize_api_base_relaxed("ftp://host/v1"), None);
        assert_eq!(normalize_api_base_relaxed(""), None);
    }

    #[test]
    fn resolve_login_provider_loose_matches_id_alias_and_display_name() {
        // id
        assert_eq!(
            resolve_login_provider_loose("anthropic-api").map(|d| d.id),
            Some("anthropic-api")
        );
        // alias
        assert_eq!(
            resolve_login_provider_loose("claude-api").map(|d| d.id),
            Some("anthropic-api")
        );
        // display name (the form LoginCompleted carries for API-key paste logins)
        assert_eq!(
            resolve_login_provider_loose("Anthropic API").map(|d| d.id),
            Some("anthropic-api")
        );
        // display name is matched case-insensitively
        assert_eq!(
            resolve_login_provider_loose("anthropic api").map(|d| d.id),
            Some("anthropic-api")
        );
        // unknown input stays unresolved
        assert_eq!(resolve_login_provider_loose("not-a-provider"), None);
    }

    #[test]
    fn resolve_login_provider_loose_resolves_every_descriptor_by_id_and_display_name() {
        // Guards the LoginCompleted attribution path: the TUI publishes either a
        // descriptor id (OAuth logins) or a display label (API-key paste logins),
        // and both must resolve so the post-login auth-change refresh is
        // attributed to the right provider instead of falling back to the
        // session active provider.
        for descriptor in login_providers() {
            assert_eq!(
                resolve_login_provider_loose(descriptor.id).map(|d| d.id),
                Some(descriptor.id),
                "descriptor id {:?} should resolve",
                descriptor.id
            );
            assert_eq!(
                resolve_login_provider_loose(descriptor.display_name).map(|d| d.id),
                Some(descriptor.id),
                "display name {:?} (id {:?}) should resolve",
                descriptor.display_name,
                descriptor.id
            );
        }
    }

    #[test]
    fn ollama_profile_is_local_openai_compatible_without_required_api_key() {
        assert_eq!(OLLAMA_PROFILE.id, "ollama");
        assert_eq!(OLLAMA_PROFILE.api_base, "http://localhost:11434/v1");
        assert_eq!(OLLAMA_PROFILE.api_key_env, "OLLAMA_API_KEY");
        assert_eq!(OLLAMA_PROFILE.env_file, "ollama.env");
        assert_eq!(
            OLLAMA_PROFILE.setup_url,
            "https://docs.ollama.com/api/openai-compatibility"
        );
        assert_eq!(OLLAMA_PROFILE.default_model, None);
        const {
            assert!(!OLLAMA_PROFILE.requires_api_key);
        }
        assert_eq!(
            OLLAMA_LOGIN_PROVIDER.auth_kind,
            LoginProviderAuthKind::Local
        );
        assert_eq!(OLLAMA_LOGIN_PROVIDER.auth_status_method, "local endpoint");
        assert!(matches!(
            OLLAMA_LOGIN_PROVIDER.target,
            LoginProviderTarget::OpenAiCompatible(profile) if profile.id == "ollama"
        ));
    }

    #[test]
    fn lmstudio_profile_is_local_openai_compatible_without_required_api_key() {
        assert_eq!(LMSTUDIO_PROFILE.id, "lmstudio");
        assert_eq!(LMSTUDIO_PROFILE.api_base, "http://localhost:1234/v1");
        assert_eq!(LMSTUDIO_PROFILE.default_model, None);
        const {
            assert!(!LMSTUDIO_PROFILE.requires_api_key);
        }
        assert_eq!(
            LMSTUDIO_LOGIN_PROVIDER.auth_kind,
            LoginProviderAuthKind::Local
        );
        assert_eq!(LMSTUDIO_LOGIN_PROVIDER.auth_status_method, "local endpoint");
        assert!(matches!(
            LMSTUDIO_LOGIN_PROVIDER.target,
            LoginProviderTarget::OpenAiCompatible(profile) if profile.id == "lmstudio"
        ));
    }

    #[test]
    fn matrix_login_provider_aliases_resolve_to_canonical_ids() {
        assert_eq!(
            resolve_login_provider("subscription").map(|provider| provider.id),
            Some("jcode")
        );
        assert_eq!(
            resolve_login_provider("anthropic").map(|provider| provider.id),
            Some("claude")
        );
        assert_eq!(
            resolve_login_provider("claude-api").map(|provider| provider.id),
            Some("anthropic-api")
        );
        assert_eq!(
            resolve_login_provider("openai-key").map(|provider| provider.id),
            Some("openai-api")
        );
        assert_eq!(
            resolve_login_provider("compat").map(|provider| provider.id),
            Some("openai-compatible")
        );
        assert_eq!(
            resolve_login_provider("aoai").map(|provider| provider.id),
            Some("azure")
        );
        assert_eq!(
            resolve_login_provider("lm-studio").map(|provider| provider.id),
            Some("lmstudio")
        );
        assert_eq!(
            resolve_login_provider("gmail").map(|provider| provider.id),
            Some("google")
        );
        assert_eq!(
            resolve_login_provider("aws-bedrock").map(|provider| provider.id),
            Some("bedrock")
        );
    }

    #[test]
    fn matrix_login_provider_ids_and_aliases_are_unique() {
        let mut seen = HashSet::new();
        for provider in login_providers() {
            assert!(
                seen.insert(provider.id),
                "duplicate login provider identifier: {}",
                provider.id
            );
            for alias in provider.aliases {
                assert!(
                    seen.insert(*alias),
                    "duplicate login provider alias: {}",
                    alias
                );
            }
        }
    }

    #[test]
    fn matrix_tui_login_selection_supports_numbers_and_names() {
        let providers = tui_login_providers();
        assert_eq!(
            resolve_login_selection("1", &providers).map(|provider| provider.id),
            Some("auto-import")
        );
        assert_eq!(
            resolve_login_selection("2", &providers).map(|provider| provider.id),
            Some("claude")
        );
        // `anthropic-api` sits at 3 (between claude and openai).
        assert_eq!(
            resolve_login_selection("3", &providers).map(|provider| provider.id),
            Some("anthropic-api")
        );
        assert_eq!(
            resolve_login_selection("4", &providers).map(|provider| provider.id),
            Some("openai")
        );
        assert_eq!(
            resolve_login_selection("5", &providers).map(|provider| provider.id),
            Some("jcode")
        );
        assert_eq!(
            resolve_login_selection("6", &providers).map(|provider| provider.id),
            Some("openrouter")
        );
        assert_eq!(
            resolve_login_selection("7", &providers).map(|provider| provider.id),
            Some("bedrock")
        );
        assert_eq!(
            resolve_login_selection("8", &providers).map(|provider| provider.id),
            Some("azure")
        );
        assert_eq!(
            resolve_login_selection("9", &providers).map(|provider| provider.id),
            Some("openai-compatible")
        );
        assert_eq!(
            resolve_login_selection("compat", &providers).map(|provider| provider.id),
            Some("openai-compatible")
        );
        assert!(resolve_login_selection("google", &providers).is_none());
    }

    #[test]
    fn matrix_cli_login_selection_preserves_existing_order() {
        let providers = cli_login_providers();
        assert_eq!(
            resolve_login_selection("1", &providers).map(|provider| provider.id),
            Some("auto-import")
        );
        // `anthropic-api` at 3 shifted everything after it down one slot.
        assert_eq!(
            resolve_login_selection("3", &providers).map(|provider| provider.id),
            Some("anthropic-api")
        );
        assert_eq!(
            resolve_login_selection("5", &providers).map(|provider| provider.id),
            Some("jcode")
        );
        assert_eq!(
            resolve_login_selection("6", &providers).map(|provider| provider.id),
            Some("copilot")
        );
        assert_eq!(
            resolve_login_selection("7", &providers).map(|provider| provider.id),
            Some("openrouter")
        );
        assert_eq!(
            resolve_login_selection("8", &providers).map(|provider| provider.id),
            Some("bedrock")
        );
        assert_eq!(
            resolve_login_selection("9", &providers).map(|provider| provider.id),
            Some("azure")
        );
        assert_eq!(
            resolve_login_selection("bedrock", &providers).map(|provider| provider.id),
            Some("bedrock")
        );
    }
}
