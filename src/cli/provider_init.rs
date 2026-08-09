use anyhow::Result;
use std::sync::Arc;

use crate::auth;
use crate::provider;
use crate::provider::Provider;
use crate::provider_catalog::{
    LoginProviderDescriptor, LoginProviderTarget, OpenAiCompatibleProfile,
    apply_openai_compatible_profile_env, force_apply_openai_compatible_profile_env,
    is_safe_env_file_name, is_safe_env_key_name, resolve_openai_compatible_profile,
};
use crate::tool;

use super::output;

use crate::external_auth::{
    can_prompt_for_external_auth, external_auth_blocked_message, prompt_to_trust_external_auth,
};

/// 已废弃的 Claude Code CLI 子进程传输的 CLI id（兼容入口）。
///
/// 散落在 provider_init/login/auth_test/commands 的该魔法字符串统一引用
/// 此常量，避免改名或删除时遗漏。
pub const CLAUDE_SUBPROCESS_ID: &str = "claude-subprocess";

/// 运行时解析的 provider 选择（resonix 思路：二进制不硬编码厂商名，
/// `--provider` 接受任意字符串，按注册表 + 用户配置解析）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedProviderInput {
    /// 自动探测已配置 provider（`auto` 或空）。
    Auto,
    /// 登录 provider 注册表命中的条目（id/alias/display_name 解析）。
    Login(LoginProviderDescriptor),
    /// 用户配置 `[providers.<name>]` 命中的命名 profile。
    NamedProfile(String),
}

/// 按字符串解析 `--provider` 输入。
///
/// 解析链（与 resonix 一致：核心只认识注册表与配置，不认识厂商名）：
/// 1. `auto`/空 → 自动探测
/// 2. `claude-subprocess` → 废弃兼容入口
/// 3. 登录 provider 注册表（`LOGIN_PROVIDERS`，按 id/alias）→ 原生登录 provider
/// 4. 用户配置 `[providers.<name>]` → 命名配置 profile
/// 5. 未命中 → 报错并提示 `jcode provider add`
pub fn resolve_provider_input(input: &str) -> Result<ResolvedProviderInput> {
    let trimmed = input.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("auto") {
        return Ok(ResolvedProviderInput::Auto);
    }
    if let Some(provider) = crate::provider_catalog::resolve_login_provider(trimmed) {
        return Ok(ResolvedProviderInput::Login(provider));
    }
    if crate::config::config().providers.contains_key(trimmed) {
        return Ok(ResolvedProviderInput::NamedProfile(trimmed.to_string()));
    }
    anyhow::bail!(
        "Unknown provider '{}'. Use a registered provider id (claude, openai, openrouter, ...), a [providers.{}] config profile, or `jcode provider add {} --base-url ...` to define one.",
        trimmed,
        trimmed,
        trimmed
    )
}

/// 判断字符串是否是 `auto`（未显式指定 provider）。
pub fn is_auto_provider_input(input: &str) -> bool {
    let trimmed = input.trim();
    trimmed.is_empty() || trimmed.eq_ignore_ascii_case("auto")
}

struct AutoProviderAvailability {
    auth_status: auth::AuthStatus,
    has_claude: bool,
    has_openai: bool,
    has_openrouter: bool,
}

impl AutoProviderAvailability {
    fn has_any_provider(&self) -> bool {
        self.has_claude || self.has_openai || self.has_openrouter
    }
}

async fn detect_auto_provider_flags() -> AutoProviderAvailability {
    let auth_status = auth::AuthStatus::check_fast();
    AutoProviderAvailability {
        has_claude: auth_status.anthropic.has_oauth || auth_status.anthropic.has_api_key,
        has_openai: auth_status.openai_has_oauth || auth_status.openai_has_api_key,
        has_openrouter: auth_status.openrouter == auth::AuthState::Available,
        auth_status,
    }
}

fn provider_label_for_api_key_env(env_key: &str) -> String {
    if env_key == "OPENROUTER_API_KEY" {
        return "OpenRouter".to_string();
    }

    crate::provider_catalog::openai_compatible_profiles()
        .iter()
        .find_map(|profile| {
            let resolved = resolve_openai_compatible_profile(*profile);
            (resolved.api_key_env == env_key).then_some(resolved.display_name)
        })
        .unwrap_or_else(|| env_key.to_string())
}

fn provider_login_hint_for_api_key_env(_env_key: &str) -> String {
    "jcode provider add <name> --base-url <url> --api-key-env <ENV_VAR>".to_string()
}

fn ensure_external_api_key_auth_allowed_for_explicit_choice(env_key: &str) -> Result<()> {
    if direct_api_key_configured_for_env(env_key) {
        return Ok(());
    }
    let Some(source) = auth::external::preferred_unconsented_api_key_source_for_env(env_key) else {
        return Ok(());
    };
    let path = source.path()?;
    let provider_name = provider_label_for_api_key_env(env_key);
    let login_hint = provider_login_hint_for_api_key_env(env_key);
    if !can_prompt_for_external_auth() {
        anyhow::bail!(external_auth_blocked_message(
            &provider_name,
            source.display_name(),
            &path,
            &login_hint,
        ));
    }
    if prompt_to_trust_external_auth(&provider_name, source.display_name(), &path)? {
        auth::external::trust_external_auth_source(source)?;
        return Ok(());
    }
    anyhow::bail!(
        "Skipped trusting external {} credentials. Run `{}` to authenticate jcode directly.",
        provider_name,
        login_hint
    )
}

fn direct_api_key_configured_for_env(env_key: &str) -> bool {
    let env_key = env_key.trim();
    if env_key.is_empty() {
        return false;
    }
    if std::env::var(env_key)
        .ok()
        .map(|key| !key.trim().is_empty())
        .unwrap_or(false)
    {
        return true;
    }

    crate::provider_catalog::openai_compatible_profiles()
        .iter()
        .filter_map(|profile| {
            let resolved = resolve_openai_compatible_profile(*profile);
            (resolved.api_key_env == env_key).then_some(resolved.env_file)
        })
        .any(|env_file| direct_env_file_contains_key(env_key, &env_file))
}

fn direct_env_file_contains_key(env_key: &str, env_file: &str) -> bool {
    if !crate::provider_catalog::is_safe_env_file_name(env_file) {
        return false;
    }
    let Some(config_dir) = crate::storage::app_config_dir().ok() else {
        return false;
    };
    let path = config_dir.join(env_file);
    let Ok(content) = std::fs::read_to_string(path) else {
        return false;
    };
    let prefix = format!("{}=", env_key);
    content.lines().any(|line| {
        line.strip_prefix(&prefix)
            .map(|key| !key.trim().trim_matches('"').trim_matches('\'').is_empty())
            .unwrap_or(false)
    })
}

pub fn select_initial_model_provider(provider_key: &str) {
    crate::provider::activation::select_initial_runtime_provider_key(provider_key);
}

pub fn clear_initial_model_provider() {
    crate::provider::activation::clear_initial_runtime_provider();
}

/// A CLI provider choice for a dual-auth backend is also a credential choice.
/// Pin it through the provider's credential-mode API
/// so `--provider anthropic-api` cannot remain in Auto mode and prefer a stored
/// Claude OAuth credential over `ANTHROPIC_API_KEY` (and likewise for OpenAI).
fn explicit_credential_mode(resolved: &ResolvedProviderInput) -> Option<provider::CredentialMode> {
    match resolved {
        ResolvedProviderInput::Login(desc) => match desc.target {
            LoginProviderTarget::ClaudeApiKey | LoginProviderTarget::OpenAiApiKey => {
                Some(provider::CredentialMode::ApiKey)
            }
            _ => None,
        },
        _ => None,
    }
}

fn disable_subscription_runtime_mode() {
    crate::subscription_catalog::clear_runtime_env();
}

fn disable_subscription_runtime_mode_preserving_active_provider_profile() {
    if std::env::var_os("JCODE_PROVIDER_PROFILE_ACTIVE").is_some()
        || std::env::var_os("JCODE_NAMED_PROVIDER_PROFILE").is_some()
    {
        crate::env::remove_var(crate::subscription_catalog::JCODE_SUBSCRIPTION_ACTIVE_ENV);
    } else {
        disable_subscription_runtime_mode();
    }
}

pub fn apply_login_provider_profile_env(provider: LoginProviderDescriptor) {
    match provider.target {
        LoginProviderTarget::OpenAiCompatible(profile) => {
            force_apply_openai_compatible_profile_env(Some(profile));
            // Bootstrap login still spawns the daemon with `--provider auto`. Mark the
            // just-selected compatible provider as active so the child process does
            // not clear these inherited runtime vars before credential detection.
            crate::env::set_var("JCODE_PROVIDER_PROFILE_ACTIVE", "1");
        }
        LoginProviderTarget::AutoImport => {}
        _ => {
            // A later non-compatible login selection must not inherit a stale
            // compatible-provider profile from an earlier bootstrap/login path.
            force_apply_openai_compatible_profile_env(None);
        }
    }
}

fn resolved_profile_default_model(profile: OpenAiCompatibleProfile) -> Option<String> {
    resolve_openai_compatible_profile(profile).default_model
}

pub fn save_named_api_key(env_file: &str, key_name: &str, key: &str) -> Result<()> {
    if !is_safe_env_key_name(key_name) {
        anyhow::bail!("Invalid API key variable name: {}", key_name);
    }
    if !is_safe_env_file_name(env_file) {
        anyhow::bail!("Invalid env file name: {}", env_file);
    }

    let config_dir = crate::storage::app_config_dir()?;
    let file_path = config_dir.join(env_file);
    crate::storage::upsert_env_file_value(&file_path, key_name, Some(key))?;

    crate::env::set_var(key_name, key);
    Ok(())
}

pub async fn init_provider(
    choice: &str,
    model: Option<&str>,
) -> Result<Arc<dyn provider::Provider>> {
    init_provider_with_options(choice, model, true).await
}

pub async fn init_provider_quiet(
    choice: &str,
    model: Option<&str>,
) -> Result<Arc<dyn provider::Provider>> {
    init_provider_with_options(choice, model, false).await
}

pub async fn init_provider_for_validation(
    choice: &str,
    model: Option<&str>,
) -> Result<Arc<dyn provider::Provider>> {
    init_provider_with_options(choice, model, false).await
}

async fn init_provider_with_options(
    choice: &str,
    model: Option<&str>,
    show_init_messages: bool,
) -> Result<Arc<dyn provider::Provider>> {
    // Provider construction resolves concrete runtimes through the base
    // crate's external-runtime registry (composition-root pattern). The
    // binary's normal path registers them in `startup::run()`, but this
    // function is also entered directly by validation/login/test flows that
    // never run startup. Registration is idempotent, so do it here too;
    // otherwise Auto-init silently loses registry-backed runtimes (e.g. the
    // OpenRouter/OpenAI-compatible factory) and their model-picker routes.
    super::startup::register_external_provider_runtimes();

    if let Ok(profile_name) = std::env::var("JCODE_PROVIDER_PROFILE_NAME")
        && !profile_name.trim().is_empty()
    {
        crate::provider_catalog::apply_named_provider_profile_env(profile_name.trim())?;
        crate::env::set_var("JCODE_PROVIDER_PROFILE_ACTIVE", "1");
    }

    let resolved = resolve_provider_input(choice)?;

    if std::env::var_os("JCODE_PROVIDER_PROFILE_ACTIVE").is_none()
        && std::env::var_os("JCODE_NAMED_PROVIDER_PROFILE").is_none()
    {
        if let Some(profile) = compatible_profile_for_input(&resolved) {
            apply_openai_compatible_profile_env(Some(profile));
        } else {
            apply_openai_compatible_profile_env(None);
        }
    }

    let init_notice = |message: &str| {
        if show_init_messages {
            output::stderr_info(message);
        }
    };

    let provider: Arc<dyn provider::Provider> = match &resolved {
        ResolvedProviderInput::Login(desc) => match desc.target {
            LoginProviderTarget::OpenAiCompatible(profile) => {
                init_openai_compatible_runtime(Some(profile), None, &init_notice)?
            }
            _ => anyhow::bail!(
                "login provider `{}` is no longer a model provider; configure it via `jcode provider add`",
                desc.id
            ),
        },
        ResolvedProviderInput::NamedProfile(name) => {
            crate::provider_catalog::apply_named_provider_profile_env_from_config(
                name,
                crate::config::config(),
            )?;
            crate::env::set_var("JCODE_PROVIDER_PROFILE_ACTIVE", "1");
            crate::env::set_var("JCODE_PROVIDER_PROFILE_NAME", name);
            init_openai_compatible_runtime(None, Some(name), &init_notice)?
        }
        ResolvedProviderInput::Auto => {
            disable_subscription_runtime_mode_preserving_active_provider_profile();
            clear_initial_model_provider();
            let auto_detect_start = std::time::Instant::now();
            let availability = detect_auto_provider_flags().await;

            // resonix 化：`[provider] default_provider` 命中的命名配置 profile
            // 优先于 API key 探测。纯配置（本地 ollama、无 key 端点）也能用
            // `auto` 启动，配置是唯一模型接入方式。
            let cfg = crate::config::config();
            let config_default = cfg
                .provider
                .default_provider
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .filter(|name| cfg.providers.contains_key(*name))
                .map(str::to_string);

            let auto_detect_ms = auto_detect_start.elapsed().as_millis();
            crate::logging::info(&format!(
                "[TIMING] auto_provider_bootstrap: detect={}ms, config_default={}, final_has_any={}",
                auto_detect_ms,
                config_default.as_deref().unwrap_or(""),
                availability.has_any_provider()
            ));

            if let Some(profile_name) = config_default {
                crate::provider_catalog::apply_named_provider_profile_env_from_config(
                    &profile_name,
                    cfg,
                )?;
                crate::env::set_var("JCODE_PROVIDER_PROFILE_ACTIVE", "1");
                crate::env::set_var("JCODE_PROVIDER_PROFILE_NAME", &profile_name);
                init_openai_compatible_runtime(None, Some(&profile_name), &init_notice)?
            } else if availability.has_any_provider() {
                let multi = provider::MultiProvider::from_auth_status(availability.auth_status);
                init_notice(&format!(
                    "Using {} (use /model to switch models)",
                    multi.name()
                ));
                crate::env::set_var("JCODE_ACTIVE_PROVIDER", multi.name().to_lowercase());
                Arc::new(multi)
            } else {
                anyhow::bail!(
                    "No configured providers found. Add a model provider first:\n  jcode provider add <name> --base-url <url> --api-key-env <ENV_VAR>"
                );
            }
        }
    };

    if let Some(mode) = explicit_credential_mode(&resolved) {
        provider.set_credential_mode(mode).map_err(|err| {
            anyhow::anyhow!(
                "Failed to select the credential route for --provider {}: {err}",
                choice
            )
        })?;
    }

    if std::env::var_os("JCODE_PROVIDER_PROFILE_ACTIVE").is_none()
        && std::env::var_os("JCODE_NAMED_PROVIDER_PROFILE").is_none()
        && model.is_none()
        && let Some(profile) = compatible_profile_for_input(&resolved)
        && let Some(default_model) = resolved_profile_default_model(profile)
        && provider.set_model(&default_model).is_ok()
    {
        let resolved = resolve_openai_compatible_profile(profile);
        init_notice(&format!(
            "Using default model for {}: {}",
            resolved.display_name, default_model
        ));
    }

    // 命名 profile（cc-switch 等）表内没有 `default_model` 时，优先应用
    // 配置层的默认模型（`[provider] default_model` / resonix 顶层
    // `default_model`），避免 model 为空导致上下文窗口落到 200K 兜底。
    if model.is_none() && provider.model().trim().is_empty() {
        let cfg = crate::config::config();
        let config_default_model = cfg
            .effective_default_model()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if let Some(default_model) = config_default_model
            && provider.set_model(default_model).is_ok()
        {
            init_notice(&format!(
                "Using default model from config: {}",
                default_model
            ));
        } else {
            // 自动发现兜底（cc-switch 本地网关等，配置未指定 default_model 时
            // 跟随网关当前模型）。
            let _ = provider.prefetch_models().await;
            let selected = provider.model();
            if !selected.trim().is_empty() {
                init_notice(&format!(
                    "Using auto-discovered model for {}: {}",
                    provider.display_name(),
                    selected
                ));
            }
        }
    }

    if let Some(model_name) = model {
        if let Err(e) = provider.set_model(model_name) {
            init_notice(&format!(
                "Warning: failed to set model '{}': {}",
                model_name, e
            ));
        } else {
            init_notice(&format!("Using model: {}", model_name));
        }
    }

    Ok(provider)
}

pub async fn init_provider_and_registry(
    choice: &str,
    model: Option<&str>,
) -> Result<(Arc<dyn provider::Provider>, tool::Registry)> {
    let provider = init_provider(choice, model).await?;
    let registry = tool::Registry::new(provider.clone()).await;
    Ok((provider, registry))
}

pub async fn init_provider_and_registry_for_validation(
    choice: &str,
    model: Option<&str>,
) -> Result<(Arc<dyn provider::Provider>, tool::Registry)> {
    let provider = init_provider_for_validation(choice, model).await?;
    let registry = tool::Registry::new(provider.clone()).await;
    Ok((provider, registry))
}

/// 解析输入中命中的内置 openai-compatible profile（若有）。
fn compatible_profile_for_input(
    resolved: &ResolvedProviderInput,
) -> Option<OpenAiCompatibleProfile> {
    match resolved {
        ResolvedProviderInput::Login(desc) => match desc.target {
            LoginProviderTarget::OpenAiCompatible(profile) => Some(profile),
            _ => None,
        },
        _ => None,
    }
}

/// 统一初始化 openai-compatible（含命名配置 profile）运行时。
///
/// `profile` 为内置 profile（`--provider ollama` 等注册表条目命中），`named`
/// 为显式 `[providers.<name>]` 配置名（`--provider <name>` 命中）。二者互斥：
/// `named` 优先，`--provider-profile` 路径通过 `JCODE_NAMED_PROVIDER_PROFILE`
/// env 进入本函数（此时 `named` 为 None，内部从 env 读取）。
fn init_openai_compatible_runtime(
    profile: Option<OpenAiCompatibleProfile>,
    named: Option<&str>,
    init_notice: &dyn Fn(&str),
) -> Result<Arc<dyn provider::Provider>> {
    disable_subscription_runtime_mode();
    let named = named
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            std::env::var("JCODE_NAMED_PROVIDER_PROFILE")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        });
    if named.is_none()
        && let Some(profile) = profile
    {
        // An explicit `--provider <compatible>` selection should win over
        // any stale active-profile marker inherited from a previous
        // bootstrap/login flow. Named provider profiles still take
        // precedence when explicitly configured.
        force_apply_openai_compatible_profile_env(Some(profile));
    }
    let mut runtime_model_hint = None;
    let display_name = if let Some(named) = &named {
        if let Some(profile) = crate::config::config().providers.get(named) {
            runtime_model_hint = profile.default_model.clone();
        }
        named.clone()
    } else {
        let profile =
            profile.ok_or_else(|| anyhow::anyhow!("missing provider profile for choice"))?;
        let resolved = resolve_openai_compatible_profile(profile);
        if resolved.requires_api_key {
            ensure_external_api_key_auth_allowed_for_explicit_choice(&resolved.api_key_env)?;
        }
        runtime_model_hint = resolved.default_model.clone();
        resolved.display_name
    };
    // A named profile with `api = "anthropic"` speaks the Anthropic
    // Messages wire format against its own endpoint (Anthropic-
    // compatible gateways/routers). Everything else keeps the OpenAI
    // chat-completions transport, mirroring the composition-root
    // factory in `startup::register_external_provider_runtimes`.
    let anthropic_format = named
        .as_deref()
        .and_then(|name| crate::config::config().providers.get(name))
        .is_some_and(|profile| {
            profile.api_format == Some(crate::config::ProviderApiFormat::Anthropic)
        });
    if anthropic_format {
        init_notice(&format!(
            "Using {} via Anthropic-compatible API as the initial provider",
            display_name
        ));
    } else {
        init_notice(&format!(
            "Using {} via OpenAI-compatible API as the initial provider",
            display_name
        ));
        crate::provider::activation::apply_openai_compatible_runtime(runtime_model_hint)?;
    }
    if let Some(named) = named {
        let cfg = crate::config::config();
        let profile = cfg
            .providers
            .get(&named)
            .ok_or_else(|| anyhow::anyhow!("Unknown provider profile '{}'", named))?;
        if profile.api_format == Some(crate::config::ProviderApiFormat::Anthropic) {
            Ok(Arc::new(
                jcode_provider_anthropic_runtime::named::NamedAnthropicProvider::new_named(
                    &named, profile,
                )?,
            ))
        } else {
            Ok(Arc::new(
                jcode_provider_openrouter_runtime::OpenRouterProvider::new_named_openai_compatible(
                    &named, profile,
                )?,
            ))
        }
    } else {
        Ok(Arc::new(
            jcode_provider_openrouter_runtime::OpenRouterProvider::new()?,
        ))
    }
}

#[cfg(test)]
#[path = "provider_init_tests.rs"]
mod tests;
