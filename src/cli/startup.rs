use anyhow::Result;
use clap::Parser;

use crate::{logging, perf, server, startup_profile, storage};

use super::{
    args::{Args, Command},
    dispatch, hot_exec, output, terminal,
};

fn sync_output_style_from_config() {
    crate::output_style::set_emoji_enabled(crate::config::config().display.emoji);
}

pub async fn run() -> Result<()> {
    startup_profile::init();

    terminal::install_panic_hook();
    startup_profile::mark("panic_hook");

    logging::init();
    startup_profile::mark("logging_init");
    // Old log pruning now runs on a background thread inside logging::init(),
    // so it no longer blocks startup. Memory-event logs have a separate,
    // longer (14-day) retention, so prune them on their own background thread.
    std::thread::Builder::new()
        .name("jcode-memlog-cleanup".to_string())
        .spawn(crate::memory_log::cleanup_old_memory_logs)
        .ok();
    // Prune stale per-session `.bak` recovery copies (never the transcripts
    // themselves) so the sessions directory does not grow without bound.
    std::thread::Builder::new()
        .name("jcode-session-bak-prune".to_string())
        .spawn(crate::session::prune_old_session_backups)
        .ok();
    logging::info("jcode starting");

    // resonix 对齐：把旧分散 env 文件（openai-compatible.env 等）合并进统一
    // `<jcode home>/.env`。幂等且静默失败不阻塞启动。
    if let Err(err) = crate::provider_catalog::maybe_migrate_legacy_env_files() {
        logging::warn(&format!("Failed to migrate legacy env files: {err:#}"));
    }

    // Wire config-reload reactions without making config depend on auth/bus:
    // when the config cache reloads, invalidate the auth-status cache and
    // broadcast a models-updated event.
    sync_output_style_from_config();
    crate::config::on_config_reloaded(sync_output_style_from_config);
    crate::config::on_config_reloaded(crate::auth::AuthStatus::invalidate_cache);
    crate::config::on_config_reloaded(|| crate::bus::Bus::global().publish_models_updated());
    crate::config::on_config_reloaded(|| {
        crate::bus::Bus::global().publish(crate::bus::BusEvent::ConfigReloaded)
    });
    // 配置热重载后重建 api_base 已变化的 provider runtimes（server 模板；
    // 运行中会话的 fork 实例由 server 的 ConfigReloaded bus 消费点对账）。
    crate::config::on_config_reloaded(
        crate::provider::reconcile_active_provider_runtimes_with_config,
    );

    // Invert the legacy provider_catalog -> auth dependency: provider_catalog
    // consults registered fallback resolvers, and auth (the higher layer)
    // registers its external-CLI credential scan here.
    crate::provider_catalog::register_api_key_fallback_resolver(
        crate::auth::external::load_api_key_for_env,
    );

    // Register externally-implemented provider runtimes with the base
    // provider registry. These crates sit downstream of jcode-base (so
    // provider edits do not rebuild the app spine), which means base cannot
    // name their concrete types; this composition root wires them up instead.
    register_external_provider_runtimes();

    // Invert the legacy safety -> notifications dependency: safety raises a
    // permission request and the notifications layer (which depends on safety
    // types) delivers it via the dispatcher registered here.
    crate::safety::register_permission_notifier(|action, description, request_id| {
        crate::notifications::NotificationDispatcher::new().dispatch_permission_request(
            action,
            description,
            request_id,
        );
    });

    // Invert the legacy memory -> skill dependency: memory collects synthetic
    // entries from registered providers, and skill (the higher layer that
    // depends on MemoryEntry) registers its registry->memory adapter here.
    // The shared snapshot holds global skills only; memory retrieval is
    // process-scoped, so compose the project overlay from the process cwd
    // (issue #457 keeps session overlays out of the shared registry).
    crate::memory::register_synthetic_entry_provider(|| {
        let global = crate::skill::SkillRegistry::shared_snapshot();
        crate::skill::SkillRegistry::effective_for_working_dir(&global, None)
            .list()
            .into_iter()
            .map(|skill| skill.as_memory_entry())
            .collect()
    });

    // Invert the legacy server -> tui dependency: the TUI session picker owns
    // the session-list cache and registers its invalidator here, so the server
    // can drop the cache (e.g. after a rename) without referencing tui.
    crate::session_list_cache::register_invalidator(
        crate::tui::session_picker::invalidate_session_list_cache,
    );

    // Invert the legacy tui -> cli dependency for shared-server spawning: the
    // CLI owns the provider-bootstrap spawn logic and registers it here, so the
    // TUI reconnect loop can request a replacement server via server_spawn
    // without referencing cli.
    crate::server_spawn::register_default_server_spawner(Box::new(|| {
        Box::pin(async { dispatch::spawn_server("auto", None, None).await })
    }));

    crate::tui::keybind::log_keybinding_default_warnings();
    crate::platform::raise_nofile_limit_best_effort(8_192);
    startup_profile::mark("nofile_limit");

    storage::harden_user_config_permissions();
    startup_profile::mark("perm_harden");

    perf::init_background();
    startup_profile::mark("perf_init");

    let args = parse_and_prepare_args()?;
    spawn_background_update_check(&args);

    if let Err(e) = dispatch::run_main(args).await {
        report_main_error(&e);
        return Err(e);
    }

    Ok(())
}

/// Register provider runtimes that live downstream of `jcode-base` with the
/// base crate's external provider registry. Keep every downstream runtime
/// registration in this one function so the composition-root wiring stays
/// discoverable as more providers move out of the base crate.
pub fn register_external_provider_runtimes() {
    crate::provider::external::register_external_provider(
        crate::provider::external::ANTHROPIC_RUNTIME,
        || std::sync::Arc::new(jcode_provider_anthropic_runtime::AnthropicProvider::new()),
    );
    // OpenRouter serves several identities (aggregator, pinned API-key
    // runtime, direct OpenAI-compatible profiles, named config profiles)
    // through one concrete type, so it registers a parameterized factory.
    crate::provider::external::register_openrouter_factory(|spec| {
        use crate::provider::external::OpenRouterRuntimeSpec;
        use jcode_provider_openrouter_runtime::OpenRouterProvider;
        let provider: std::sync::Arc<dyn crate::provider::Provider> = match spec {
            OpenRouterRuntimeSpec::Default => std::sync::Arc::new(OpenRouterProvider::new()?),
            OpenRouterRuntimeSpec::OpenRouterApiKey => {
                std::sync::Arc::new(OpenRouterProvider::new_openrouter_api_key_runtime()?)
            }
            OpenRouterRuntimeSpec::CompatibleProfile(profile) => std::sync::Arc::new(
                OpenRouterProvider::new_openai_compatible_profile_runtime(profile)?,
            ),
            OpenRouterRuntimeSpec::NamedProfile { name, config } => {
                // A named profile with `api = "anthropic"` speaks the Anthropic
                // Messages wire format against its own endpoint (Anthropic-
                // compatible gateways/routers). Everything else keeps the
                // OpenAI chat-completions transport.
                if config.api_format == Some(crate::config::ProviderApiFormat::Anthropic) {
                    std::sync::Arc::new(
                        jcode_provider_anthropic_runtime::named::NamedAnthropicProvider::new_named(
                            &name, &config,
                        )?,
                    )
                } else {
                    std::sync::Arc::new(OpenRouterProvider::new_named_openai_compatible(
                        &name, &config,
                    )?)
                }
            }
        };
        Ok(provider)
    });
    crate::provider::external::register_profile_catalog_refresh(
        jcode_provider_openrouter_runtime::maybe_schedule_openai_compatible_profile_catalog_refresh,
    );
    crate::provider::external::register_standard_openrouter_catalog_refresh(
        jcode_provider_openrouter_runtime::maybe_schedule_standard_openrouter_catalog_refresh,
    );
    // API-backed OpenAI routes use Codex/platform credentials. Without valid
    // credentials the runtime is not registered (provider unavailable) rather
    // than falling back to a browser-backed transport.
    crate::provider::external::register_external_provider_fallible(
        crate::provider::external::OPENAI_RUNTIME,
        || {
            let provider = match crate::auth::codex::load_credentials() {
                Ok(credentials) => jcode_provider_openai_runtime::OpenAIProvider::new(credentials),
                Err(err) => {
                    logging::info(&format!(
                        "OpenAI runtime not registered: no usable credentials ({err:#}). \
                         Run `jcode provider add openai --base-url https://api.openai.com/v1 --api-key-env OPENAI_API_KEY` to add them."
                    ));
                    return None;
                }
            };
            Some(std::sync::Arc::new(provider) as std::sync::Arc<dyn crate::provider::Provider>)
        },
    );
}

fn parse_and_prepare_args() -> Result<Args> {
    let args = Args::parse();
    startup_profile::mark("args_parse");

    output::set_quiet_enabled(args.quiet);

    if let Some(cwd) = &args.cwd {
        std::env::set_current_dir(cwd)?;
        logging::info(&format!("Changed working directory to: {}", cwd));
    }

    validate_remote_working_dir(args.remote_working_dir.as_deref())?;

    if args.trace {
        crate::env::set_var("JCODE_TRACE", "1");
    }

    if let Some(ref socket) = args.socket {
        server::set_socket_path(socket);
    }

    crate::cli::proctitle::set_initial_title(&args);

    Ok(args)
}

fn validate_remote_working_dir(remote_working_dir: Option<&str>) -> Result<()> {
    if let Some(remote_working_dir) = remote_working_dir
        && !remote_working_dir_is_absolute(remote_working_dir)
    {
        anyhow::bail!("--remote-working-dir must be an absolute path");
    }
    Ok(())
}

fn remote_working_dir_is_absolute(path: &str) -> bool {
    if path.starts_with('/') || path.starts_with('\\') {
        return true;
    }

    let bytes = path.as_bytes();
    bytes.len() >= 3
        && bytes[1] == b':'
        && (bytes[2] == b'/' || bytes[2] == b'\\')
        && bytes[0].is_ascii_alphabetic()
}

fn spawn_background_update_check(args: &Args) {
    let check_updates = should_spawn_background_update_check(args);
    let auto_update = should_auto_install_update(args);

    if !check_updates {
        return;
    }

    std::thread::spawn(move || {
        use crate::bus::{Bus, BusEvent, UpdateStatus};

        let start = std::time::Instant::now();
        Bus::global().publish(BusEvent::UpdateStatus(UpdateStatus::Checking));
        if let Some(update_available) = hot_exec::check_for_updates()
            && update_available
        {
            // A checkout with local commits can never fast-forward, so the
            // pull below would always fail and surface a noisy "Update
            // diverged. Press Ctrl+Y..." card in every new session.
            // Developers with local work expect divergence; log it once
            // and stay quiet in the UI (no Available/Error cards).
            if hot_exec::local_commits_ahead_of_upstream() == Some(true) {
                logging::info(
                    "Auto-update skipped: local commits are ahead of upstream (diverged). \
                     Merge or rebase manually when ready.",
                );
                Bus::global().publish(BusEvent::UpdateStatus(UpdateStatus::UpToDate));
            } else {
                Bus::global().publish(BusEvent::UpdateStatus(UpdateStatus::Available {
                    current: jcode_build_meta::version().to_string(),
                    latest: "latest source".to_string(),
                }));
                if auto_update {
                    logging::info("Update available - auto-updating...");
                    Bus::global().publish(BusEvent::UpdateStatus(UpdateStatus::Installing {
                        version: "latest source".to_string(),
                    }));
                    if let Err(e) = hot_exec::run_auto_update() {
                        Bus::global()
                            .publish(BusEvent::UpdateStatus(UpdateStatus::Error(e.to_string())));
                        logging::error(&format!(
                            "Auto-update failed: {}. Continuing with current version.",
                            e
                        ));
                    }
                } else {
                    logging::info("Update available! Run `jcode update` or `/reload` to update.");
                }
            }
        } else {
            Bus::global().publish(BusEvent::UpdateStatus(UpdateStatus::UpToDate));
        }
        logging::info(&format!(
            "[TIMING] background_update_check: auto_update={}, total={}ms",
            auto_update,
            start.elapsed().as_millis()
        ));
    });
}

fn should_spawn_background_update_check(args: &Args) -> bool {
    !args.quiet
        && !args.no_update
        && !matches!(
            args.command,
            Some(Command::Update { .. }) | Some(Command::Serve { .. }) | Some(Command::Acp)
        )
        && args.resume.is_none()
}

fn should_auto_install_update(args: &Args) -> bool {
    args.auto_update
}

fn report_main_error(error: &anyhow::Error) {
    let error_str = format!("{:?}", error);
    logging::error(&error_str);

    if let Some(session_id) = terminal::get_current_session() {
        output::stderr_blank_line();
        output::stderr_info("\x1b[33mTo restore this session, run:\x1b[0m");
        output::stderr_info(format!("  jcode --resume {}", session_id));
        output::stderr_blank_line();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::{Args, Command};
    use clap::Parser;

    fn parse_args(argv: &[&str]) -> Args {
        Args::parse_from(argv)
    }

    #[test]
    fn auto_install_allowed_without_live_terminal() {
        let args = parse_args(&["jcode", "version"]);
        assert!(should_auto_install_update(&args));
    }

    #[test]
    fn auto_install_allowed_with_live_terminal_attached() {
        let args = parse_args(&["jcode", "version"]);
        assert!(should_auto_install_update(&args));
    }

    #[test]
    fn auto_install_respects_explicit_disable_even_without_terminal() {
        let mut args = parse_args(&["jcode", "version"]);
        args.auto_update = false;
        assert!(!should_auto_install_update(&args));
    }

    #[test]
    fn remote_working_dir_validation_requires_absolute_path() {
        assert!(validate_remote_working_dir(Some("/home/agent/project")).is_ok());
        assert!(validate_remote_working_dir(Some("C:\\Users\\agent\\project")).is_ok());
        assert!(validate_remote_working_dir(None).is_ok());

        let error = validate_remote_working_dir(Some("relative/project")).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("--remote-working-dir must be an absolute path")
        );
    }

    #[test]
    fn update_command_still_skips_background_check_before_auto_install_logic() {
        let args = parse_args(&["jcode", "update"]);
        assert!(matches!(args.command, Some(Command::Update { .. })));
        assert!(!should_spawn_background_update_check(&args));
        assert!(should_auto_install_update(&args));
    }

    #[test]
    fn external_provider_runtimes_register_and_instantiate() {
        let _guard = crate::storage::lock_test_env();
        // Inject usable OpenAI credentials so the OpenAI runtime factory
        // instantiates regardless of the machine's real auth state.
        crate::auth::codex::upsert_account_from_tokens(
            &crate::auth::codex::primary_account_label(),
            "test-oauth-access-token",
            "test-oauth-refresh-token",
            None,
            Some(chrono::Utc::now().timestamp_millis() + 86_400_000),
        )
        .expect("save test OpenAI OAuth credentials");
        register_external_provider_runtimes();
        for (key, expected_name) in [
            (crate::provider::external::ANTHROPIC_RUNTIME, "anthropic"),
            (crate::provider::external::OPENAI_RUNTIME, "openai"),
        ] {
            assert!(
                crate::provider::external::external_provider_registered(key),
                "{key} runtime should be registered"
            );
            let provider = crate::provider::external::instantiate_external_provider(key)
                .unwrap_or_else(|| panic!("{key} runtime factory should instantiate"));
            assert_eq!(provider.name(), expected_name);
            // resonix 化：模型列表由配置/目录驱动，裸构造的 provider 没有内置
            // 默认模型（`jcode provider add` / `/model` 负责选择）。这里只验证
            // 运行时能实例化并正确命名。
        }
    }
}
