//! Anthropic provider shared helpers (compatibility shim).
//!
//! The direct Anthropic Messages API *runtime* (`AnthropicProvider`) now lives
//! in the downstream `jcode-provider-anthropic-runtime` crate so provider
//! edits do not rebuild the base -> app-core -> tui spine. The binary's
//! composition root registers it via [`crate::provider::external`].
//!
//! Base keeps the pieces its own auth/usage/sidecar code (and the runtime
//! crate) share:
//! - the OAuth attribution headers + Claude CLI user agent used for
//!   subscription API calls,
//! - API-key resolution (`load_anthropic_api_key`, `has_anthropic_api_key`),
//! - the process-wide cache-TTL toggle, and
//! - the static model list.

use anyhow::{Context, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use uuid::Uuid;

pub use jcode_provider_core::CredentialMode as AnthropicCredentialMode;
use jcode_provider_core::{
    ANTHROPIC_OAUTH_BETA_HEADERS, anthropic_effectively_1m,
    anthropic_stainless_arch as stainless_arch, anthropic_stainless_os as stainless_os,
};

const ANTHROPIC_API_BASE: &str = "https://api.anthropic.com";

static CACHE_TTL_1H: AtomicBool = AtomicBool::new(true);

/// Resolve the Anthropic Messages API base URL for **API-key** mode.
///
/// Defaults to `https://api.anthropic.com`, but honors a user override so the
/// direct Anthropic provider can target a local/proxied Anthropic-compatible
/// gateway over either `http://` or `https://`. Checked in order:
/// `JCODE_ANTHROPIC_API_BASE`, `ANTHROPIC_BASE_URL`, `ANTHROPIC_API_BASE`.
///
/// The override must be an absolute `http(s)://` URL; anything else is logged
/// and ignored. Unlike the stricter built-in profile validator, named
/// user-configured gateways (and this explicit environment override) may use
/// public HTTP hosts.
pub fn resolve_api_base() -> String {
    const OVERRIDE_VARS: [&str; 3] = [
        "JCODE_ANTHROPIC_API_BASE",
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_API_BASE",
    ];
    for var in OVERRIDE_VARS {
        let Ok(raw) = std::env::var(var) else {
            continue;
        };
        if let Some(normalized) = crate::provider_catalog::normalize_api_base_relaxed(&raw) {
            crate::logging::info(&format!(
                "Anthropic API base overridden to '{}' via {}",
                normalized, var
            ));
            return normalized;
        }
        crate::logging::warn(&format!(
            "Ignoring invalid {} '{}'; expected an absolute http(s):// URL",
            var,
            raw.trim()
        ));
    }
    ANTHROPIC_API_BASE.to_string()
}

/// Whether the direct Anthropic API-key path is pointing at a custom gateway.
pub fn is_custom_api_base_configured() -> bool {
    resolve_api_base().trim_end_matches('/') != ANTHROPIC_API_BASE
}

/// Derive the Messages endpoint from an Anthropic base URL.
///
/// Accepts any of:
/// - `https://host`                 -> `https://host/v1/messages`
/// - `https://host/v1`              -> `https://host/v1/messages`
/// - `https://host/v1/messages`     -> verbatim
/// - `https://host/anthropic`       -> `https://host/anthropic/v1/messages`
pub fn messages_url_from_api_base(base: &str) -> String {
    let trimmed = base.trim_end_matches('/');
    if trimmed.ends_with("/messages") {
        trimmed.to_string()
    } else if trimmed.ends_with("/v1") {
        format!("{trimmed}/messages")
    } else {
        format!("{trimmed}/v1/messages")
    }
}

/// Derive the `GET /models` catalog endpoint from an Anthropic base URL.
pub fn models_url_from_api_base(base: &str) -> String {
    let trimmed = base.trim_end_matches('/');
    let trimmed = trimmed
        .strip_suffix("/messages")
        .map(|value| value.trim_end_matches('/'))
        .unwrap_or(trimmed);
    if trimmed.ends_with("/v1") {
        format!("{trimmed}/models")
    } else {
        format!("{trimmed}/v1/models")
    }
}

/// Enable or disable the 1-hour cache TTL (default: 1-hour)
pub fn set_cache_ttl_1h(enabled: bool) {
    CACHE_TTL_1H.store(enabled, Ordering::Relaxed);
}

/// Check if 1-hour cache TTL is enabled
pub fn is_cache_ttl_1h() -> bool {
    CACHE_TTL_1H.load(Ordering::Relaxed)
}

/// User-Agent for OAuth requests, matching the official Claude Code CLI.
pub const CLAUDE_CLI_USER_AGENT: &str = "claude-cli/2.1.123 (external, sdk-cli)";

pub const OAUTH_BETA_HEADERS: &str = ANTHROPIC_OAUTH_BETA_HEADERS;

/// Whether a model id effectively runs with the 1M-token context beta.
pub fn effectively_1m(model: &str) -> bool {
    anthropic_effectively_1m(model)
}

pub fn new_oauth_request_id() -> String {
    Uuid::new_v4().to_string()
}

/// Attach the OAuth attribution headers the official Claude CLI sends.
/// Shared by the runtime crate's request path and base's usage probes.
pub fn apply_oauth_attribution_headers(
    req: reqwest::RequestBuilder,
    session_id: &str,
) -> reqwest::RequestBuilder {
    req.header("x-client-request-id", new_oauth_request_id())
        .header("x-app", "cli")
        .header("X-Claude-Code-Session-Id", session_id)
        .header("X-Stainless-Arch", stainless_arch())
        .header("X-Stainless-Lang", "js")
        .header("X-Stainless-OS", stainless_os())
        .header("X-Stainless-Package-Version", "0.81.0")
        .header("X-Stainless-Retry-Count", "0")
        .header("X-Stainless-Runtime", "node")
        .header("X-Stainless-Runtime-Version", "v24.3.0")
        .header("X-Stainless-Timeout", "600")
        .header("anthropic-dangerous-direct-browser-access", "true")
}

/// Available models
pub const AVAILABLE_MODELS: &[&str] = &[
    "claude-opus-5",
    "claude-fable-5",
    "claude-opus-4-8",
    "claude-opus-4-6",
    "claude-opus-4-6[1m]",
    "claude-sonnet-5",
    "claude-sonnet-4-6",
    "claude-sonnet-4-6[1m]",
    "claude-haiku-4-5",
    "claude-opus-4-5",
    "claude-sonnet-4-5",
    "claude-sonnet-4-20250514",
];

pub fn load_anthropic_api_key() -> Result<String> {
    let key = crate::provider_catalog::load_api_key_from_env_or_config(
        "ANTHROPIC_API_KEY",
        "anthropic.env",
    )
    .context("No Anthropic API key found")?;
    if std::env::var("JCODE_LOG_SERVICE_TIER").is_ok() {
        let prefix: String = key.chars().take(14).collect();
        eprintln!(
            "[anthropic] resolved API key prefix={prefix}... (len={})",
            key.len()
        );
    }
    Ok(key)
}

pub fn has_anthropic_api_key() -> bool {
    load_anthropic_api_key().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EnvGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn remove(key: &'static str) -> Self {
            let previous = std::env::var_os(key);
            crate::env::remove_var(key);
            Self { key, previous }
        }

        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var_os(key);
            crate::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(previous) = self.previous.take() {
                crate::env::set_var(self.key, previous);
            } else {
                crate::env::remove_var(self.key);
            }
        }
    }

    #[test]
    fn resolve_api_base_precedence_and_validation() {
        let _a = EnvGuard::remove("JCODE_ANTHROPIC_API_BASE");
        let _b = EnvGuard::remove("ANTHROPIC_BASE_URL");
        let _c = EnvGuard::remove("ANTHROPIC_API_BASE");
        assert_eq!(resolve_api_base(), ANTHROPIC_API_BASE);

        let _d = EnvGuard::set("ANTHROPIC_API_BASE", "https://b.example/v1");
        let _e = EnvGuard::set("ANTHROPIC_BASE_URL", "https://a.example/v1");
        assert_eq!(resolve_api_base(), "https://a.example/v1");

        let _f = EnvGuard::set("JCODE_ANTHROPIC_API_BASE", "http://gateway.example/v1");
        assert_eq!(resolve_api_base(), "http://gateway.example/v1");
        assert!(is_custom_api_base_configured());

        let _g = EnvGuard::set("JCODE_ANTHROPIC_API_BASE", "not-a-url");
        assert_eq!(resolve_api_base(), "https://a.example/v1");
    }

    #[test]
    fn messages_and_models_urls_handle_common_base_shapes() {
        assert_eq!(
            messages_url_from_api_base("https://host"),
            "https://host/v1/messages"
        );
        assert_eq!(
            messages_url_from_api_base("http://host/v1"),
            "http://host/v1/messages"
        );
        assert_eq!(
            messages_url_from_api_base("https://host/v1/messages"),
            "https://host/v1/messages"
        );
        assert_eq!(
            models_url_from_api_base("https://host"),
            "https://host/v1/models"
        );
        assert_eq!(
            models_url_from_api_base("http://host/v1"),
            "http://host/v1/models"
        );
        assert_eq!(
            models_url_from_api_base("https://host/v1/messages"),
            "https://host/v1/models"
        );
        assert_eq!(
            models_url_from_api_base("https://host/anthropic"),
            "https://host/anthropic/v1/models"
        );
    }
}
