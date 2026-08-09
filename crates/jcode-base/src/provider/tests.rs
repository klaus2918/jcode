use super::*;
use crate::provider::models::{ensure_model_allowed_for_subscription, filtered_display_models};

fn with_clean_provider_test_env<T>(f: impl FnOnce() -> T) -> T {
    let _guard = crate::storage::lock_test_env();
    // Concrete provider runtimes live downstream (jcode-provider-*-runtime),
    // so base tests register shared stubs through the same composition-root
    // registry the binary uses. Registration is idempotent (last write wins),
    // and per-test overrides can re-register a different stub.
    register_test_external_runtimes();
    let temp = tempfile::tempdir().expect("tempdir");
    let prev_home = std::env::var_os("JCODE_HOME");
    let prev_subscription =
        std::env::var_os(crate::subscription_catalog::JCODE_SUBSCRIPTION_ACTIVE_ENV);
    let mut profile_env_keys = vec![
        "OPENROUTER_API_KEY",
        "DEEPSEEK_API_KEY",
        "KIMI_API_KEY",
        "JCODE_OPENROUTER_API_BASE",
        "JCODE_OPENROUTER_API_KEY_NAME",
        "JCODE_OPENROUTER_ENV_FILE",
        "JCODE_OPENROUTER_CACHE_NAMESPACE",
        "JCODE_OPENROUTER_PROVIDER_FEATURES",
        "JCODE_OPENROUTER_TRANSPORT_STATE",
        "JCODE_OPENROUTER_ALLOW_NO_AUTH",
        "JCODE_OPENROUTER_MODEL_CATALOG",
        "JCODE_OPENROUTER_MODEL",
        "JCODE_OPENROUTER_STATIC_MODELS",
        "JCODE_OPENAI_COMPAT_API_BASE",
        "JCODE_OPENAI_COMPAT_API_KEY_NAME",
        "JCODE_OPENAI_COMPAT_ENV_FILE",
        "JCODE_OPENAI_COMPAT_DEFAULT_MODEL",
        "JCODE_OPENAI_COMPAT_LOCAL_ENABLED",
        "OPENAI_COMPAT_API_KEY",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "JCODE_RUNTIME_PROVIDER",
        "JCODE_ACTIVE_PROVIDER",
        "JCODE_INITIAL_PROVIDER_EXPLICIT",
        "JCODE_OPENAI_MODEL",
        "JCODE_NAMED_PROVIDER_PROFILE",
        "JCODE_PROVIDER_PROFILE_ACTIVE",
        "JCODE_PROVIDER_PROFILE_NAME",
    ];
    for profile in crate::provider_catalog::openai_compatible_profiles() {
        if !profile_env_keys.contains(&profile.api_key_env) {
            profile_env_keys.push(profile.api_key_env);
        }
    }
    let saved_profile_env = profile_env_keys
        .into_iter()
        .map(|key| (key, std::env::var_os(key)))
        .collect::<Vec<_>>();
    crate::env::set_var("JCODE_HOME", temp.path());
    for (key, _) in &saved_profile_env {
        crate::env::remove_var(key);
    }
    crate::subscription_catalog::clear_runtime_env();
    crate::auth::claude::set_active_account_override(None);
    crate::auth::codex::set_active_account_override(None);
    // The in-memory model catalog services are process-global; earlier tests
    // may have hydrated scopes (fixture models) that would corrupt this test's
    // known_*_model_ids() validation, and vice versa. Reset on entry and exit
    // so neither direction leaks.
    crate::provider::models::reset_model_catalog_services_for_tests();

    let result = f();

    crate::provider::models::reset_model_catalog_services_for_tests();
    crate::auth::claude::set_active_account_override(None);
    crate::auth::codex::set_active_account_override(None);
    if let Some(prev_home) = prev_home {
        crate::env::set_var("JCODE_HOME", prev_home);
    } else {
        crate::env::remove_var("JCODE_HOME");
    }
    if let Some(prev_subscription) = prev_subscription {
        crate::env::set_var(
            crate::subscription_catalog::JCODE_SUBSCRIPTION_ACTIVE_ENV,
            prev_subscription,
        );
    } else {
        crate::env::remove_var(crate::subscription_catalog::JCODE_SUBSCRIPTION_ACTIVE_ENV);
    }
    for (key, value) in saved_profile_env {
        if let Some(value) = value {
            crate::env::set_var(key, value);
        } else {
            crate::env::remove_var(key);
        }
    }
    crate::subscription_catalog::clear_runtime_env();
    result
}

fn enter_test_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build tokio runtime")
}

#[test]
fn openai_compatible_profile_catalog_cache_is_fresh_before_soft_refresh_boundary() {
    assert!(!openai_compatible_profile_catalog_cache_is_stale(
        1_000,
        1_000 + OPENAI_COMPATIBLE_PROFILE_CATALOG_SOFT_REFRESH_SECS - 1,
    ));
}

#[test]
fn openai_compatible_profile_catalog_cache_is_stale_at_soft_refresh_boundary() {
    assert!(openai_compatible_profile_catalog_cache_is_stale(
        1_000,
        1_000 + OPENAI_COMPATIBLE_PROFILE_CATALOG_SOFT_REFRESH_SECS,
    ));
    assert!(!openai_compatible_profile_catalog_cache_is_stale(
        2_000, 1_000
    ));
}

fn with_env_var<T>(key: &str, value: &str, f: impl FnOnce() -> T) -> T {
    let prev = std::env::var_os(key);
    crate::env::set_var(key, value);
    let result = f();
    if let Some(prev) = prev {
        crate::env::set_var(key, prev);
    } else {
        crate::env::remove_var(key);
    }
    result
}

fn save_test_openai_compatible_login_config(default_model: &str) {
    let env_file = crate::provider_catalog::OPENAI_COMPAT_PROFILE.env_file;
    crate::provider_catalog::save_env_value_to_env_file(
        "JCODE_OPENAI_COMPAT_API_BASE",
        env_file,
        Some("https://example-openai-compatible.test/v1"),
    )
    .expect("save api base");
    crate::provider_catalog::save_env_value_to_env_file(
        "OPENAI_COMPAT_API_KEY",
        env_file,
        Some("sk-test-openai-compatible"),
    )
    .expect("save api key");
    crate::provider_catalog::save_env_value_to_env_file(
        "JCODE_OPENAI_COMPAT_DEFAULT_MODEL",
        env_file,
        Some(default_model),
    )
    .expect("save default model");
}

fn save_test_openrouter_model_cache(namespace: &str, source_api_base: &str, model_ids: &[&str]) {
    let jcode_home = std::env::var_os("JCODE_HOME").expect("test JCODE_HOME should be set");
    let cache_dir = std::path::PathBuf::from(jcode_home).join("cache");
    std::fs::create_dir_all(&cache_dir).expect("create model cache dir");
    let cache = jcode_provider_openrouter::DiskCache {
        cached_at: jcode_provider_openrouter::current_unix_secs().expect("current unix time"),
        source_api_base: Some(source_api_base.to_string()),
        models: model_ids
            .iter()
            .map(|id| jcode_provider_openrouter::ModelInfo {
                id: (*id).to_string(),
                name: String::new(),
                context_length: None,
                pricing: jcode_provider_openrouter::ModelPricing::default(),
                created: None,
            })
            .collect(),
    };
    let path = cache_dir.join(format!("{namespace}_models.json"));
    std::fs::write(
        path,
        serde_json::to_string(&cache).expect("serialize model cache"),
    )
    .expect("write model cache");
}

fn clear_openai_compatible_runtime_env() {
    for key in [
        "JCODE_OPENAI_COMPAT_API_BASE",
        "JCODE_OPENAI_COMPAT_API_KEY_NAME",
        "JCODE_OPENAI_COMPAT_ENV_FILE",
        "JCODE_OPENAI_COMPAT_DEFAULT_MODEL",
        "JCODE_OPENAI_COMPAT_LOCAL_ENABLED",
        "OPENAI_COMPAT_API_KEY",
        "JCODE_OPENROUTER_CACHE_NAMESPACE",
    ] {
        crate::env::remove_var(key);
    }
}

fn save_test_openai_oauth_credentials() {
    crate::auth::codex::upsert_account_from_tokens(
        &crate::auth::codex::primary_account_label(),
        "test-oauth-access-token",
        "test-oauth-refresh-token",
        None,
        Some(chrono::Utc::now().timestamp_millis() + 86_400_000),
    )
    .expect("save test OpenAI OAuth credentials");
}

fn test_multi_provider_with_openai() -> MultiProvider {
    save_test_openai_oauth_credentials();
    crate::env::set_var("OPENAI_API_KEY", "sk-test-openai-api-key");
    MultiProvider {
        claude: RwLock::new(None),
        anthropic: RwLock::new(None),
        openai: RwLock::new(Some(test_openai_runtime() as Arc<dyn Provider>)),
        openrouter: RwLock::new(None),
        openai_compatible_profiles: RwLock::new(std::collections::HashMap::new()),
        active_openai_compatible_profile: RwLock::new(None),
        active: RwLock::new(ActiveProvider::OpenAI),
        startup_notices: RwLock::new(Vec::new()),
        initial_provider: None,
        routes_memo: std::sync::Mutex::new(None),
        post_auth_refreshes_pending: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    }
}

#[test]
fn model_picker_allowlist_blocks_direct_switch_outside_list() {
    with_clean_provider_test_env(|| {
        let jcode_home = std::env::var_os("JCODE_HOME").expect("test JCODE_HOME should be set");
        std::fs::write(
            std::path::PathBuf::from(jcode_home).join("config.toml"),
            r#"
[provider]
model_picker_providers = ["openai-compatible"]
"#,
        )
        .expect("write test config.toml");
        crate::config::invalidate_config_cache();

        let rt = enter_test_runtime();
        let _runtime_guard = rt.enter();
        let provider = test_multi_provider_with_openai();
        let allowed = "gpt-5.5";

        assert!(
            provider.ensure_model_switch_allowed(allowed).is_ok(),
            "openai-compatible model should remain switchable when openai-compatible is allowlisted"
        );
        assert!(
            provider
                .ensure_model_switch_allowed("some-other-model")
                .is_err(),
            "unlisted model must be rejected when the allowlist excludes it"
        );
    });
}

fn assert_openai_compatible_route_available(provider: &MultiProvider, model: &str) {
    let routes = provider.model_routes();
    assert!(
        routes.iter().any(|route| {
            route.provider == "OpenAI-compatible"
                && matches!(
                    route.api_method.as_str(),
                    "openai-compatible" | "openai-compatible:openai-compatible"
                )
                && route.model == model
                && route.available
        }),
        "configured OpenAI-compatible model should be immediately visible after API-key setup; routes: {routes:?}"
    );
}

#[test]
fn openai_compatible_api_key_setup_makes_configured_model_route_available() {
    with_clean_provider_test_env(|| {
        save_test_openai_compatible_login_config("glm-test-login-flow");

        assert!(
            crate::provider_catalog::openai_compatible_profile_is_configured(
                crate::provider_catalog::OPENAI_COMPAT_PROFILE,
            )
        );

        let provider = MultiProvider::new();
        assert_openai_compatible_route_available(&provider, "glm-test-login-flow");

        provider
            .set_model_on_openai_compatible_profile(
                crate::provider_catalog::OPENAI_COMPAT_PROFILE,
                "glm-test-login-flow",
            )
            .expect("configured OpenAI-compatible model should select without requiring another provider login");

        assert_eq!(provider.model(), "glm-test-login-flow");
    });
}

#[test]
fn openai_compatible_api_key_setup_survives_process_restart_without_relogin() {
    with_clean_provider_test_env(|| {
        save_test_openai_compatible_login_config("restart-visible-model");

        // Simulate a fresh process: the login command wrote the config file, but
        // none of the runtime env vars from the login process remain populated.
        clear_openai_compatible_runtime_env();

        let resolved = crate::provider_catalog::resolve_openai_compatible_profile(
            crate::provider_catalog::OPENAI_COMPAT_PROFILE,
        );
        assert_eq!(
            resolved.api_base,
            "https://example-openai-compatible.test/v1"
        );
        assert_eq!(
            resolved.default_model.as_deref(),
            Some("restart-visible-model")
        );
        assert!(
            crate::provider_catalog::openai_compatible_profile_is_configured(
                crate::provider_catalog::OPENAI_COMPAT_PROFILE,
            )
        );

        let provider = MultiProvider::new();
        assert_openai_compatible_route_available(&provider, "restart-visible-model");
        provider
            .set_model_on_openai_compatible_profile(
                crate::provider_catalog::OPENAI_COMPAT_PROFILE,
                "restart-visible-model",
            )
            .expect("saved credentials should be selectable after a fresh process restart");
        assert_eq!(provider.model(), "restart-visible-model");
    });
}

#[test]
#[ignore = "removed built-in provider profile; rewrite with a retained profile"]
fn configured_openai_compatible_profile_routes_use_live_cache_when_not_active_provider() {
    with_clean_provider_test_env(|| {
        crate::provider_catalog::save_env_value_to_env_file(
            "OPENROUTER_API_KEY",
            "openrouter.env",
            Some("sk-test-openrouter"),
        )
        .expect("save openrouter key");
        crate::provider_catalog::save_env_value_to_env_file(
            "OPENCODE_API_KEY",
            "opencode.env",
            Some("oc-test-opencode"),
        )
        .expect("save opencode key");
        save_test_openrouter_model_cache(
            "opencode",
            "https://opencode.ai/zen/v1",
            &["kimi-k2.6", "zen-live-only-model"],
        );

        let provider = MultiProvider::new();
        let routes = provider.model_routes();
        let opencode_routes = routes
            .iter()
            .filter(|route| route.provider == "OpenCode Zen")
            .collect::<Vec<_>>();

        assert!(
            opencode_routes
                .iter()
                .any(|route| route.model == "zen-live-only-model"
                    && route.api_method == "openai-compatible:opencode"
                    && !route
                        .detail
                        .contains("fallback: static provider model list")),
            "non-active configured direct profile should expose its live /models cache, routes: {opencode_routes:?}"
        );
        assert!(
            !opencode_routes.iter().any(|route| route.model == "glm-4.7"),
            "static fallback models should drop out once a live profile catalog is available, routes: {opencode_routes:?}"
        );
    });
}

#[test]
fn standard_openrouter_catalog_refresh_is_noop_when_cache_fresh() {
    with_clean_provider_test_env(|| {
        let runtime = enter_test_runtime();
        runtime.block_on(async {
            crate::provider_catalog::save_env_value_to_env_file(
                "OPENROUTER_API_KEY",
                "openrouter.env",
                Some("sk-test-openrouter"),
            )
            .expect("save openrouter key");
            // A fresh, non-empty standard OpenRouter cache should suppress the
            // background refresh entirely so we never fire a needless network
            // request on every picker render.
            save_test_openrouter_model_cache(
                "openrouter",
                "https://openrouter.ai/api/v1",
                &["openrouter/owl-alpha"],
            );

            assert!(
                !openrouter::maybe_schedule_standard_openrouter_catalog_refresh(
                    "unit test fresh cache"
                ),
                "a fresh non-empty standard OpenRouter cache must not trigger a refresh"
            );
        });
    });
}

#[test]
fn standard_openrouter_catalog_refresh_skips_without_key() {
    with_clean_provider_test_env(|| {
        let runtime = enter_test_runtime();
        runtime.block_on(async {
            // No OPENROUTER_API_KEY configured: the refresh must not be
            // scheduled regardless of cache state.
            assert!(
                !openrouter::maybe_schedule_standard_openrouter_catalog_refresh(
                    "unit test missing key"
                ),
                "standard OpenRouter refresh must be skipped when no key is configured"
            );
        });
    });
}

#[test]
fn standard_openrouter_catalog_refresh_fires_when_named_profile_owns_slot() {
    with_clean_provider_test_env(|| {
        let runtime = enter_test_runtime();
        runtime.block_on(async {
            crate::provider_catalog::save_env_value_to_env_file(
                "OPENROUTER_API_KEY",
                "openrouter.env",
                Some("sk-test-openrouter"),
            )
            .expect("save openrouter key");
            // Simulate an active named profile (e.g. Gemini API) occupying the
            // shared OpenRouter/OpenAI-compatible slot: it sets the runtime env
            // vars to point at a non-openrouter.ai endpoint. The standard
            // OpenRouter catalog refresh must STILL fire so `/model` can list
            // openrouter.ai models (issue #292). Cache is missing -> not fresh.
            crate::env::set_var(
                "JCODE_OPENROUTER_API_BASE",
                "https://integrate.api.nvidia.com/v1",
            );
            crate::env::set_var("JCODE_OPENROUTER_CACHE_NAMESPACE", "mynvidia");

            // Other tests in this process may already have attempted (or be
            // running) an `openrouter` catalog refresh; clear the process-wide
            // backoff/in-flight tracker or this assertion is flaky under
            // parallel test execution.
            jcode_provider_openrouter_runtime::reset_profile_catalog_refresh_tracker_for_tests();

            assert!(
                openrouter::maybe_schedule_standard_openrouter_catalog_refresh(
                    "unit test named profile owns slot"
                ),
                "standard OpenRouter refresh must fire even when a named profile sets JCODE_OPENROUTER_* env"
            );
        });
    });
}

/// Parameterized test stand-in for provider runtimes that live downstream
/// (jcode-provider-{gemini,cursor,antigravity}-runtime) and therefore cannot
/// be constructed from base tests. Mirrors each runtime's catalog surface
/// (static model list plus `ModelRoute`s) so routing/fallback tests stay
/// meaningful.
struct StubExternalRuntime {
    name: &'static str,
    provider_label: &'static str,
    api_method: &'static str,
    models: &'static [&'static str],
    model: std::sync::RwLock<String>,
    credential_mode: std::sync::RwLock<jcode_provider_core::CredentialMode>,
}

impl StubExternalRuntime {
    fn new(
        name: &'static str,
        provider_label: &'static str,
        api_method: &'static str,
        models: &'static [&'static str],
    ) -> Self {
        Self {
            name,
            provider_label,
            api_method,
            models,
            model: std::sync::RwLock::new(models[0].to_string()),
            credential_mode: std::sync::RwLock::new(jcode_provider_core::CredentialMode::Auto),
        }
    }

    fn anthropic() -> Self {
        Self::new(
            "anthropic",
            "Anthropic",
            "https",
            anthropic::AVAILABLE_MODELS,
        )
    }

    fn openai() -> Self {
        Self::new("openai", "OpenAI", "https", &["gpt-5.5", "gpt-5-mini"])
    }
}

#[async_trait::async_trait]
impl Provider for StubExternalRuntime {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[ToolDefinition],
        _system: &str,
        _resume_session_id: Option<&str>,
    ) -> anyhow::Result<EventStream> {
        anyhow::bail!("stub {} runtime does not stream", self.name)
    }
    fn name(&self) -> &'static str {
        self.name
    }
    fn model(&self) -> String {
        self.model
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
    fn set_model(&self, model: &str) -> anyhow::Result<()> {
        let trimmed = model.trim();
        if trimmed.is_empty() {
            anyhow::bail!("{} model cannot be empty", self.provider_label);
        }
        // Mirror the real runtimes' family validation: the registry is
        // process-global, so hot-init can hand this stub to tests that expect
        // cross-provider models to be rejected (e.g. a Claude model under a
        // forced-OpenAI selection).
        if !self.models.contains(&trimmed) {
            anyhow::bail!(
                "Unsupported {} model '{}'. Use /model to choose from the models available to your account.",
                self.provider_label,
                trimmed,
            );
        }
        *self
            .model
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = trimmed.to_string();
        Ok(())
    }
    fn available_models(&self) -> Vec<&'static str> {
        self.models.to_vec()
    }
    fn available_models_display(&self) -> Vec<String> {
        self.models.iter().map(|model| model.to_string()).collect()
    }
    fn available_models_for_switching(&self) -> Vec<String> {
        self.available_models_display()
    }
    fn model_routes(&self) -> Vec<ModelRoute> {
        self.available_models_display()
            .into_iter()
            .map(|model| ModelRoute {
                capability: None,
                model,
                provider: self.provider_label.to_string(),
                api_method: self.api_method.to_string(),
                available: true,
                detail: String::new(),
                cheapness: None,
            })
            .collect()
    }
    fn credential_mode(&self) -> jcode_provider_core::CredentialMode {
        *self
            .credential_mode
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
    fn set_credential_mode(&self, mode: jcode_provider_core::CredentialMode) -> anyhow::Result<()> {
        *self
            .credential_mode
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = mode;
        Ok(())
    }
    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(StubExternalRuntime::new(
            self.name,
            self.provider_label,
            self.api_method,
            self.models,
        ))
    }
}

fn test_anthropic_runtime() -> Arc<StubExternalRuntime> {
    Arc::new(StubExternalRuntime::anthropic())
}

fn test_openai_runtime() -> Arc<StubExternalRuntime> {
    Arc::new(StubExternalRuntime::openai())
}

/// Register the shared external-runtime stubs for every downstream provider
/// slot base can hot-initialize. Called by `with_clean_provider_test_env` so
/// hot-init/startup tests find a runtime the way the real binary does.
fn register_test_external_runtimes() {
    external::register_external_provider(external::ANTHROPIC_RUNTIME, || {
        test_anthropic_runtime() as Arc<dyn Provider>
    });
    external::register_external_provider(external::OPENAI_RUNTIME, || {
        test_openai_runtime() as Arc<dyn Provider>
    });
    // OpenRouter tests exercise the real runtime (profile-scoped catalogs,
    // transport identities), so register the real factory like the binary's
    // composition root does. The dev-dependency cycle is test-only.
    external::register_openrouter_factory(|spec| {
        use external::OpenRouterRuntimeSpec;
        use jcode_provider_openrouter_runtime::OpenRouterProvider;
        let provider: Arc<dyn Provider> = match spec {
            OpenRouterRuntimeSpec::Default => Arc::new(OpenRouterProvider::new()?),
            OpenRouterRuntimeSpec::OpenRouterApiKey => {
                Arc::new(OpenRouterProvider::new_openrouter_api_key_runtime()?)
            }
            OpenRouterRuntimeSpec::CompatibleProfile(profile) => Arc::new(
                OpenRouterProvider::new_openai_compatible_profile_runtime(profile)?,
            ),
            OpenRouterRuntimeSpec::NamedProfile { name, config } => {
                // Mirrors the binary's composition root
                // (`startup::register_external_provider_runtimes`): a named
                // profile with `api = "anthropic"` speaks the Anthropic
                // Messages wire format against its own endpoint.
                if config.api_format == Some(crate::config::ProviderApiFormat::Anthropic) {
                    Arc::new(
                        jcode_provider_anthropic_runtime::named::NamedAnthropicProvider::new_named(
                            &name, &config,
                        )?,
                    )
                } else {
                    Arc::new(OpenRouterProvider::new_named_openai_compatible(
                        &name, &config,
                    )?)
                }
            }
        };
        Ok(provider)
    });
    external::register_profile_catalog_refresh(
        jcode_provider_openrouter_runtime::maybe_schedule_openai_compatible_profile_catalog_refresh,
    );
    external::register_standard_openrouter_catalog_refresh(
        jcode_provider_openrouter_runtime::maybe_schedule_standard_openrouter_catalog_refresh,
    );
}

/// Construct a real OpenRouter/OpenAI-compatible runtime for tests through
/// the registry, mirroring production construction.
fn test_openrouter_runtime() -> anyhow::Result<Arc<dyn Provider>> {
    external::instantiate_openrouter_runtime(external::OpenRouterRuntimeSpec::Default)
}

#[test]
fn new_session_fork_reloads_changed_config_provider_and_model() {
    with_clean_provider_test_env(|| {
        let runtime = enter_test_runtime();
        runtime.block_on(async {
            crate::env::set_var("ANTHROPIC_API_KEY", "sk-ant-test");
            crate::env::set_var("OPENAI_API_KEY", "sk-openai-test");

            crate::config::Config::set_default_model(Some("claude-fable-5"), Some("anthropic-api"))
                .expect("save initial Claude default");
            let template = MultiProvider::new_fast();
            assert_eq!(template.name(), "Claude");
            assert_eq!(template.model(), "claude-fable-5");

            crate::config::Config::set_default_model(Some("gpt-5.5"), Some("openai-api"))
                .expect("save changed OpenAI default");

            let fresh = template.fork_for_new_session();
            assert_eq!(fresh.name(), "OpenAI");
            assert_eq!(fresh.model(), "gpt-5.5");
            assert_eq!(
                fresh.active_resolved_credential(),
                Some(jcode_provider_core::ResolvedCredential::ApiKey)
            );

            // Ordinary forks still preserve the existing session's selection.
            let preserved = template.fork();
            assert_eq!(preserved.name(), "Claude");
            assert_eq!(preserved.model(), "claude-fable-5");
        });
    });
}

include!("tests/auth_refresh.rs");
include!("tests/model_resolution.rs");
include!("tests/issue_534_profile_preservation.rs");
include!("tests/fallback_failover.rs");
include!("tests/catalog_subscription.rs");

/// Regression: a resonix-style `[[providers]]` array entry (the config style
/// the user migrates to, mirroring Reasonix) must support in-session model
/// switching. The named profile runtime is installed at startup from
/// `default_provider` + `default_model`, and both the bare model id and the
/// `<profile>:<model>` picker spec must resolve to the same runtime so
/// `available_models_for_switching()` lists the configured models.
#[test]
fn resonix_array_profile_supports_model_switching_and_lists_configured_models() {
    with_clean_provider_test_env(|| {
        let runtime = enter_test_runtime();
        runtime.block_on(async {
            let jcode_home = std::env::var_os("JCODE_HOME").expect("test JCODE_HOME");
            std::fs::write(
                std::path::PathBuf::from(jcode_home).join("config.toml"),
                r#"
[provider]
default_model = "deepseek-v4-flash"
default_provider = "self-deepseek"
model_picker_providers = ["self-deepseek"]

[[providers]]
name = "self-deepseek"
type = "openai-compatible"
kind = "openai"
base_url = "https://api.deepseek.com"
model = "deepseek-v4-flash"
api_key_env = "DEEPSEEK_API_KEY"
context_window = 1000000
models = ["deepseek-v4-flash", "deepseek-v4-pro"]
"#,
            )
            .expect("write test config.toml");
            crate::env::set_var("DEEPSEEK_API_KEY", "sk-test-deepseek");
            crate::config::invalidate_config_cache();

            let template = MultiProvider::new_fast();
            assert_eq!(template.model(), "deepseek-v4-flash");
            assert_eq!(
                ProviderRegistry::new(&template)
                    .active_compatible_profile_id()
                    .as_deref(),
                Some("self-deepseek")
            );

            let switching = template.available_models_for_switching();
            assert!(
                !switching.is_empty(),
                "template switching models must not be empty: {switching:?}"
            );
            assert!(
                switching.iter().any(|m| m == "deepseek-v4-flash")
                    && switching.iter().any(|m| m == "deepseek-v4-pro"),
                "template switching models should list the configured pair: {switching:?}"
            );

            // What the server agent actually uses: a session fork of the
            // template. This is the path that produced "Model switching is not
            // available for this provider." for the user.
            let session = template.fork_for_new_session();
            assert_eq!(session.model(), "deepseek-v4-flash");
            let switching = session.available_models_for_switching();
            assert!(
                !switching.is_empty(),
                "session-fork switching models must not be empty: {switching:?}"
            );
            assert!(
                switching.iter().any(|m| m == "deepseek-v4-flash")
                    && switching.iter().any(|m| m == "deepseek-v4-pro"),
                "session-fork switching models should list the configured pair: {switching:?}"
            );

            // In-session switch to the other configured model (bare id).
            session
                .set_model("deepseek-v4-pro")
                .expect("bare in-session switch should succeed");
            assert_eq!(session.model(), "deepseek-v4-pro");

            // Switch back via the picker's prefixed route spec.
            session
                .set_model("self-deepseek:deepseek-v4-flash")
                .expect("prefixed in-session switch should succeed");
            assert_eq!(session.model(), "deepseek-v4-flash");

            // The route catalog (model picker list) must be limited to the
            // configured provider by the allowlist, and must contain both
            // configured models.
            let routes = session.model_routes();
            let route_models: Vec<&str> = routes.iter().map(|route| route.model.as_str()).collect();
            assert!(
                routes.iter().any(|r| r.model == "deepseek-v4-flash")
                    && routes.iter().any(|r| r.model == "deepseek-v4-pro"),
                "routes should contain the configured models: {route_models:?}"
            );
            // With `model_picker_providers = ["self-deepseek"]` the picker must
            // not advertise unrelated built-in providers (Claude/OpenAI/etc.).
            for builtin in ["claude-sonnet-4", "gpt-5.5", "openrouter/owl-alpha"] {
                assert!(
                    !routes.iter().any(|r| r.model == builtin),
                    "allowlist must hide built-in model '{builtin}': {route_models:?}"
                );
            }
            let switching = session.available_models_for_switching();
            for builtin in ["claude-sonnet-4", "gpt-5.5"] {
                assert!(
                    !switching.iter().any(|m| m == builtin),
                    "switching list must hide built-in model '{builtin}': {switching:?}"
                );
            }
        });
    });
}

/// Regression: the remote `/model` picker builds its route list through
/// `remote_model_routes_fallback`, which previously ignored user-defined
/// named provider profiles (`[providers.<name>]`) entirely. Models owned by a
/// configured named profile surfaced as `unavailable · no matching configured
/// provider route` and could not be selected, and the fallback `provider_key`
/// of `None` then wiped `default_provider` on the next default-model save.
#[test]
fn remote_model_routes_fallback_includes_named_provider_profile_models() {
    with_clean_provider_test_env(|| {
        let jcode_home = std::env::var_os("JCODE_HOME").expect("test JCODE_HOME");
        std::fs::write(
            std::path::PathBuf::from(jcode_home).join("config.toml"),
            r#"
[providers.deepseek-official]
type = "openai-compatible"
base_url = "https://api.deepseek.com/anthropic"
api = "anthropic"
auth = "header"
auth_header = "x-api-key"
api_key_env = "MY_DEEPSEEK_API_KEY"
default_model = "deepseek-v4-flash"

[[providers.deepseek-official.models]]
id = "deepseek-v4-flash"

[[providers.deepseek-official.models]]
id = "deepseek-v4-pro"
"#,
        )
        .expect("write test config.toml");
        crate::env::set_var("MY_DEEPSEEK_API_KEY", "sk-test-deepseek");
        crate::config::invalidate_config_cache();

        let entries = vec![
            "deepseek-v4-flash".to_string(),
            "deepseek-v4-pro".to_string(),
        ];
        let routes = crate::provider::remote_model_routes_fallback(None, &entries);
        for model in &entries {
            let route = routes
                .iter()
                .find(|r| r.model == *model)
                .unwrap_or_else(|| panic!("{model} should have a route: {routes:?}"));
            assert!(
                route.available,
                "{model} route should be available: {route:?}"
            );
            assert_eq!(
                route.api_method, "openai-compatible:deepseek-official",
                "route must point back at the named profile: {route:?}"
            );
        }
        assert!(
            routes
                .iter()
                .all(|r| r.detail != "no matching configured provider route"),
            "no route should fall through to the unavailable placeholder: {routes:?}"
        );
    });
}

/// The configured named profile must keep the model list and in-session
/// switching usable even when the API key is missing (fresh machine, unset
/// env var, different environment than the one where the key exists). A
/// missing key must not collapse the switching list to empty, which surfaces
/// as the confusing "Model switching is not available for this provider."
/// error. Requests themselves may still fail with a clear missing-key error.
#[test]
fn resonix_array_profile_lists_models_even_without_api_key() {
    with_clean_provider_test_env(|| {
        let runtime = enter_test_runtime();
        runtime.block_on(async {
            let jcode_home = std::env::var_os("JCODE_HOME").expect("test JCODE_HOME");
            std::fs::write(
                std::path::PathBuf::from(jcode_home).join("config.toml"),
                r#"
[provider]
default_model = "deepseek-v4-flash"
default_provider = "self-deepseek"
model_picker_providers = ["self-deepseek"]

[[providers]]
name = "self-deepseek"
type = "openai-compatible"
kind = "openai"
base_url = "https://api.deepseek.com"
model = "deepseek-v4-flash"
api_key_env = "DEEPSEEK_API_KEY"
context_window = 1000000
models = ["deepseek-v4-flash", "deepseek-v4-pro"]
"#,
            )
            .expect("write test config.toml");
            // Deliberately do NOT set DEEPSEEK_API_KEY: the env var is unset,
            // like a fresh machine before the user exports it.
            crate::env::remove_var("DEEPSEEK_API_KEY");
            crate::config::invalidate_config_cache();

            let template = MultiProvider::new_fast();
            let switching = template.available_models_for_switching();
            assert!(
                !switching.is_empty(),
                "switching models must not be empty without an API key: {switching:?}"
            );
            assert!(
                switching.iter().any(|m| m == "deepseek-v4-flash")
                    && switching.iter().any(|m| m == "deepseek-v4-pro"),
                "switching should still list the configured pair without a key: {switching:?}"
            );

            let session = template.fork_for_new_session();
            let switching = session.available_models_for_switching();
            assert!(
                !switching.is_empty(),
                "session-fork switching must not be empty without an API key: {switching:?}"
            );
        });
    });
}

/// cc-switch 本地代理（resonix 风格 `[[providers]]` 数组 + `auth = "none"` +
/// `kind = "anthropic"`）在**不写 `api_key_env`** 时也必须可用：被识别为
/// 已配置、出现在 /model 列表、可切换。
///
/// 回归：旧版数组条目解析会静默丢弃 `auth` 字段（退化成 Bearer+无 key），
/// 于是"没有 api_key_env = DEEPSEEK_API_KEY 就无法使用"——用户被迫在配置里
/// 补一行指向统一 .env 的 api_key_env 才能绕过。
#[test]
fn resonix_array_cc_switch_auth_none_works_without_api_key_env() {
    with_clean_provider_test_env(|| {
        let runtime = enter_test_runtime();
        runtime.block_on(async {
            let jcode_home = std::env::var_os("JCODE_HOME").expect("test JCODE_HOME");
            std::fs::write(
                std::path::PathBuf::from(jcode_home).join("config.toml"),
                r#"
[provider]
default_provider = "cc-switch"
default_model = "deepseek-v4-flash"

[[providers]]
name        = "cc-switch"
type        = "openai-compatible"
kind        = "anthropic"
base_url    = "http://127.0.0.1:15721"   # cc-switch 本地代理地址
auth        = "none"                     # cc-switch 在代理侧注入真实 API key
models      = ["deepseek-v4-flash"]
default     = "deepseek-v4-flash"
model_overrides   = { "deepseek-v4-flash" = { context_window = 1000000 } }
"#,
            )
            .expect("write test config.toml");
            // 故意不设置任何 API key：auth=none 本地网关不应需要 key。
            crate::env::remove_var("DEEPSEEK_API_KEY");
            crate::config::invalidate_config_cache();

            let cfg = crate::config::config();
            let profile = cfg
                .providers
                .get("cc-switch")
                .expect("cc-switch entry parsed");
            assert_eq!(profile.auth, crate::config::NamedProviderAuth::None);
            assert_eq!(
                profile.api_format,
                Some(crate::config::ProviderApiFormat::Anthropic)
            );
            assert_eq!(profile.default_model.as_deref(), Some("deepseek-v4-flash"));
            assert_eq!(
                profile.api_key_env, None,
                "this config must not carry api_key_env"
            );
            assert_eq!(profile.models[0].context_window, Some(1_000_000));
            assert!(
                crate::provider_catalog::named_provider_profile_is_configured("cc-switch", profile),
                "auth=none profile must count as configured without any API key"
            );

            let template = MultiProvider::new_fast();
            assert_eq!(template.model(), "deepseek-v4-flash");
            assert_eq!(
                ProviderRegistry::new(&template)
                    .active_compatible_profile_id()
                    .as_deref(),
                Some("cc-switch")
            );

            let switching = template.available_models_for_switching();
            assert!(
                switching.iter().any(|m| m == "deepseek-v4-flash"),
                "switching should list the configured model without a key: {switching:?}"
            );

            // /model 路由：cc-switch 路由必须 available（可切换）。
            let routes = template.model_routes();
            let cc_route = routes
                .iter()
                .find(|route| route.provider == "cc-switch")
                .expect("cc-switch route must exist in the picker");
            assert!(cc_route.available, "cc-switch route must be switchable");

            let session = template.fork_for_new_session();
            session
                .set_model("cc-switch:deepseek-v4-flash")
                .expect("in-session switch to the cc-switch model should succeed");
            assert_eq!(session.model(), "deepseek-v4-flash");
        });
    });
}

/// Rendering the route catalog must never schedule network work.
///
/// Regression guard for the "spawning a session refetches every provider
/// catalog" bug: route building used to schedule a background `/models` fetch
/// for each stale or missing profile cache, so every session attach and picker
/// open fanned out dozens of HTTP requests. Refresh cadence now belongs solely
/// to the background catalog scheduler.
#[test]
fn building_direct_profile_routes_does_not_schedule_catalog_refreshes() {
    with_clean_provider_test_env(|| {
        let runtime = enter_test_runtime();
        runtime.block_on(async {
            crate::provider_catalog::save_env_value_to_env_file(
                "OPENROUTER_API_KEY",
                "openrouter.env",
                Some("sk-test-openrouter"),
            )
            .expect("save openrouter key");

            // Deliberately leave every profile cache missing/stale: under the
            // old behavior this was the worst case that scheduled a refresh
            // for each configured profile.
            jcode_provider_openrouter_runtime::reset_profile_catalog_refresh_tracker_for_tests();

            for profile in crate::provider_catalog::openai_compatible_profiles()
                .iter()
                .copied()
            {
                let _ = super::direct_openai_compatible_profile_routes(profile);
            }

            // If route building had scheduled refreshes, the profile tracker
            // would have recorded attempts, and this direct call for the
            // standard OpenRouter namespace would be throttled/in-flight.
            assert!(
                openrouter::maybe_schedule_standard_openrouter_catalog_refresh(
                    "unit test post-render scheduling"
                ),
                "route building must leave the refresh tracker untouched"
            );
        });
    });
}

/// The scheduler's staleness predicate must treat a missing or mismatched
/// cache as needing a refresh, so the sweeper actually populates cold caches.
#[test]
fn profile_catalog_cache_needs_refresh_for_missing_cache() {
    with_clean_provider_test_env(|| {
        let profile = crate::provider_catalog::openai_compatible_profiles()
            .first()
            .copied()
            .expect("at least one OpenAI-compatible profile is defined");
        assert!(
            super::catalog_scheduler::profile_catalog_cache_needs_refresh(profile),
            "a missing catalog cache must be reported as needing a refresh"
        );
    });
}

/// 运行时验证：`/model openai/<model>` 斜杠引用必须真正路由到
/// OpenAI 子 provider，而不是被当作字面模型名或误路由到其他厂商。
#[test]
fn slash_ref_routes_to_openai_subprovider_at_runtime() {
    with_clean_provider_test_env(|| {
        let runtime = enter_test_runtime();
        runtime.block_on(async {
            let provider = test_multi_provider_with_openai();
            // OpenAI stub 认 gpt-5.5（见 test_openai_runtime 的 models）。
            provider
                .set_model("openai/gpt-5.5")
                .expect("slash ref should route to the OpenAI sub-provider");
            assert_eq!(provider.model(), "gpt-5.5");
            // 显式前缀之后 active provider 是 OpenAI。
            assert_eq!(
                provider.active_provider(),
                jcode_provider_core::ActiveProvider::OpenAI
            );
        });
    });
}

/// 用户报告场景：resonix 风格 `[[providers]]` 数组条目使用 `kind = "anthropic"`
/// （Anthropic Messages 协议网关，如 cch.skytech.io 直连网关），API key 统一
/// 存放在 `<jcode home>/.env`。必须满足：
/// 1. 启动时按 `default_provider` + `default_model` 挂载默认命名 provider；
/// 2. `/model` picker 的 route catalog 列出全部已配置 provider 的模型；
/// 3. bare 模型 id 与 `<profile>:<model>` 前缀均能切换。
#[test]
fn resonix_anthropic_kind_profiles_support_picker_routes_and_switching() {
    with_clean_provider_test_env(|| {
        let runtime = enter_test_runtime();
        runtime.block_on(async {
            let jcode_home = std::env::var_os("JCODE_HOME").expect("test JCODE_HOME");
            std::fs::write(
                std::path::PathBuf::from(&jcode_home).join("config.toml"),
                r#"
[provider]
default_provider = "deepseek-flash"
default_model = "deepseek-v4-flash"

[[providers]]
name        = "deepseek-flash"
kind        = "anthropic"
base_url    = "http://cch.skytech.io"
models      = ["deepseek-v4-flash"]
default     = "deepseek-v4-flash"
api_key_env = "DEEPSEEK_API_KEY"
context_window = 1000000
price       = { cache_hit = 0.02, input = 1, output = 2, currency = "¥" }
thinking    = "adaptive"
effort      = "max"

[[providers]]
name        = "kimi-1M"
kind        = "anthropic"
base_url    = "http://cch.skytech.io"
models      = ["kimi-k3", "kimi-k2.7"]
default     = "kimi-k3"
api_key_env = "KIMI_API_KEY"
context_window = 1000000
"#,
            )
            .expect("write test config.toml");
            // 统一 `.env` 是密钥权威来源（resonix 对齐）：DEEPSEEK_API_KEY /
            // KIMI_API_KEY 都写在这里，而不是进程环境变量。
            std::fs::write(
                std::path::PathBuf::from(jcode_home).join(".env"),
                "DEEPSEEK_API_KEY=sk-test-deepseek\nKIMI_API_KEY=sk-test-kimi\n",
            )
            .expect("write unified .env");
            crate::config::invalidate_config_cache();

            let template = MultiProvider::new_fast();
            // 启动即按 default_provider + default_model 挂载 deepseek-flash。
            assert_eq!(template.model(), "deepseek-v4-flash");

            // `/model` picker 的 route catalog 必须列出全部配置模型的模型。
            let routes = template.model_routes();
            let route_models: Vec<&str> = routes.iter().map(|r| r.model.as_str()).collect();
            for expected in ["deepseek-v4-flash", "kimi-k3", "kimi-k2.7"] {
                assert!(
                    routes.iter().any(|r| r.model == expected),
                    "route catalog must contain '{expected}': {route_models:?}"
                );
            }
            // 命名 provider 的路由 provider 列必须是 profile 名，available 状态
            // 必须为 true（key 已在统一 .env 中）。
            let kimi_route = routes
                .iter()
                .find(|r| r.model == "kimi-k3")
                .expect("kimi-k3 route");
            assert_eq!(kimi_route.provider, "kimi-1M");
            assert!(kimi_route.available, "configured kimi-1M must be available");

            // 服务器会话 fork 同样可见配置模型。
            let session = template.fork_for_new_session();
            let switching = session.available_models_for_switching();
            assert!(
                switching.iter().any(|m| m == "kimi-k3"),
                "session switching list must include kimi-k3: {switching:?}"
            );

            // bare 模型 id 切换（同一网关下模型属于哪个 profile 由静态模型表
            // 决定，切换后仍应可用）。
            session
                .set_model("kimi-k3")
                .expect("bare in-session switch should succeed");
            assert_eq!(session.model(), "kimi-k3");

            // picker 发出的 `<profile>:<model>` 前缀切换。
            session
                .set_model("kimi-1M:kimi-k2.7")
                .expect("prefixed in-session switch should succeed");
            assert_eq!(session.model(), "kimi-k2.7");

            // 切回默认 profile 的前缀形式。
            session
                .set_model("deepseek-flash:deepseek-v4-flash")
                .expect("switch back to deepseek-flash should succeed");
            assert_eq!(session.model(), "deepseek-v4-flash");
        });
    });
}

/// The user's real CC Switch + cch config (named-table style with per-model
/// `api_key_env`) must surface every configured model in the /model picker and
/// support in-session switching both between models and between providers.
#[test]
fn user_named_style_cc_switch_and_cch_config_supports_picker_and_switching() {
    with_clean_provider_test_env(|| {
        let runtime = enter_test_runtime();
        runtime.block_on(async {
            let jcode_home = std::env::var_os("JCODE_HOME").expect("test JCODE_HOME");
            std::fs::write(
                std::path::PathBuf::from(jcode_home).join("config.toml"),
                r#"
[provider]
default_provider = "cch"
default_model = "deepseek-v4-flash"

[providers.cc-switch]
type = "openai-compatible"
base_url = "http://127.0.0.1:15721"
api = "anthropic"
auth = "none"

[[providers.cc-switch.models]]
id = "deepseek-v4-flash"
context_window = 1000000
auth = "none"

[providers.cch]
type = "openai-compatible"
base_url = "http://cch.skytech.io"
api = "anthropic"
auth = "header"
auth_header = "x-api-key"
api_key_env = "DEEPSEEK_API_KEY"
default_model = "deepseek-v4-flash"

[[providers.cch.models]]
id = "deepseek-v4-flash"
api_key_env = "DEEPSEEK_API_KEY"
context_window = 1000000

[[providers.cch.models]]
id = "MiniMax-M3"
api_key_env = "MINIMAX_API_KEY"
context_window = 1000000

[[providers.cch.models]]
id = "glm-5.2"
api_key_env = "GLM_API_KEY"
context_window = 1000000

[[providers.cch.models]]
id = "kimi-k3"
api_key_env = "KIMI_API_KEY"
context_window = 1000000

[[providers.cch.models]]
id = "mimo-v2.5-pro"
api_key_env = "XIAOMI_MIMO_API_KEY"
context_window = 1000000
"#,
            )
            .expect("write test config.toml");
            crate::env::set_var("DEEPSEEK_API_KEY", "sk-deepseek");
            crate::env::set_var("MINIMAX_API_KEY", "sk-minimax");
            crate::env::set_var("GLM_API_KEY", "sk-glm");
            crate::env::set_var("KIMI_API_KEY", "sk-kimi");
            crate::env::set_var("XIAOMI_MIMO_API_KEY", "sk-mimo");
            crate::config::invalidate_config_cache();

            let template = MultiProvider::new_fast();
            assert_eq!(template.model(), "deepseek-v4-flash");
            assert_eq!(
                ProviderRegistry::new(&template)
                    .active_compatible_profile_id()
                    .as_deref(),
                Some("cch")
            );

            // /model picker route catalog: every configured model across both
            // providers must be listed.
            let routes = template.model_routes();
            for expected in [
                ("cc-switch", "deepseek-v4-flash"),
                ("cch", "deepseek-v4-flash"),
                ("cch", "MiniMax-M3"),
                ("cch", "glm-5.2"),
                ("cch", "kimi-k3"),
                ("cch", "mimo-v2.5-pro"),
            ] {
                assert!(
                    routes
                        .iter()
                        .any(|r| r.provider == expected.0 && r.model == expected.1),
                    "picker must list {expected:?}; got: {:?}",
                    routes
                        .iter()
                        .map(|r| (r.provider.as_str(), r.model.as_str()))
                        .collect::<Vec<_>>()
                );
            }
            assert!(
                routes.iter().all(|r| r.available),
                "all configured named routes must be switchable: {:?}",
                routes
                    .iter()
                    .map(|r| (r.provider.as_str(), r.model.as_str(), r.available))
                    .collect::<Vec<_>>()
            );

            let session = template.fork_for_new_session();
            // Bare-id switch within the default cch provider.
            session
                .set_model("MiniMax-M3")
                .expect("in-session switch to MiniMax-M3");
            assert_eq!(session.model(), "MiniMax-M3");
            // Prefixed picker route to another cch model.
            session
                .set_model("cch:glm-5.2")
                .expect("prefixed switch to glm-5.2");
            assert_eq!(session.model(), "glm-5.2");
            // Cross-provider switch to the local cc-switch gateway.
            session
                .set_model("cc-switch:deepseek-v4-flash")
                .expect("switch to cc-switch");
            assert_eq!(session.model(), "deepseek-v4-flash");
            assert_eq!(session.display_name(), "cc-switch");
        });
    });
}
