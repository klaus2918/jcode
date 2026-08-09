// Runtime `/model` switch invariants (design: model switch handling logic):
// - busy sessions reject the switch instead of mutating the shared provider
//   mid-turn;
// - same-model requests are no-ops that keep the provider session id;
// - a real switch snapshots/saves the session and best-effort persists the
//   new default model.

/// Minimal provider that records `set_model` calls and exposes a mutable
/// active model so tests can observe no-op detection and persistence.
#[derive(Clone)]
struct ModelSwitchProbeProvider {
    model: StdArc<StdMutex<String>>,
    set_model_calls: StdArc<StdMutex<Vec<String>>>,
}

#[async_trait::async_trait]
impl Provider for ModelSwitchProbeProvider {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[crate::message::ToolDefinition],
        _system: &str,
        _resume_session_id: Option<&str>,
    ) -> Result<crate::provider::EventStream> {
        unimplemented!("ModelSwitchProbeProvider")
    }

    fn name(&self) -> &str {
        "probe"
    }

    fn model(&self) -> String {
        self.model.lock().unwrap().clone()
    }

    fn set_model(&self, model: &str) -> Result<()> {
        self.set_model_calls.lock().unwrap().push(model.to_string());
        *self.model.lock().unwrap() = model.trim().to_string();
        Ok(())
    }

    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(self.clone())
    }
}

fn create_model_switch_probe_app() -> (
    App,
    StdArc<StdMutex<String>>,
    StdArc<StdMutex<Vec<String>>>,
) {
    ensure_test_jcode_home_if_unset();
    clear_persisted_test_ui_state();
    crate::tui::ui::clear_test_render_state_for_tests();

    let model = StdArc::new(StdMutex::new("gpt-5.5".to_string()));
    let set_model_calls = StdArc::new(StdMutex::new(Vec::new()));
    let provider: Arc<dyn Provider> = Arc::new(ModelSwitchProbeProvider {
        model: model.clone(),
        set_model_calls: set_model_calls.clone(),
    });
    let rt = tokio::runtime::Runtime::new().unwrap();
    let registry = rt.block_on(crate::tool::Registry::new(provider.clone()));
    let mut app = App::new_for_test_harness(provider, registry);
    app.queue_mode = false;
    (app, model, set_model_calls)
}

#[test]
fn model_switch_rejects_while_turn_is_running() {
    let (mut app, _, set_model_calls) = create_model_switch_probe_app();
    app.is_processing = true;

    assert!(super::model_context::handle_model_command(&mut app, "/model gpt-5.6"));

    let last = app.display_messages.last().expect("error message");
    assert!(
        last.content.contains("Cannot switch models"),
        "expected busy rejection, got: {}",
        last.content
    );
    assert_eq!(app.status_notice(), Some("Model switch busy".to_string()));
    assert!(
        set_model_calls.lock().unwrap().is_empty(),
        "no set_model call may be issued while a turn is running"
    );
    assert_eq!(app.session.model.as_deref(), Some("gpt-5.5"));
}

#[test]
fn model_switch_rejects_while_followups_are_queued() {
    let (mut app, _, set_model_calls) = create_model_switch_probe_app();
    app.queued_messages.push("follow-up".to_string());

    assert!(super::model_context::handle_model_command(&mut app, "/model gpt-5.6"));

    let last = app.display_messages.last().expect("error message");
    assert!(
        last.content.contains("Cannot switch models"),
        "expected busy rejection, got: {}",
        last.content
    );
    assert!(set_model_calls.lock().unwrap().is_empty());
}

#[test]
fn model_switch_same_model_is_noop_and_keeps_provider_session() {
    let (mut app, _, set_model_calls) = create_model_switch_probe_app();
    app.provider_session_id = Some("upstream-session-9".to_string());
    app.session.provider_session_id = Some("upstream-session-9".to_string());

    assert!(super::model_context::handle_model_command(&mut app, "/model gpt-5.5"));

    let last = app.display_messages.last().expect("notice");
    assert!(
        last.content.contains("Already using model"),
        "expected no-op notice, got: {}",
        last.content
    );
    assert_eq!(
        app.status_notice(),
        Some("Already using model: gpt-5.5".to_string())
    );
    assert!(
        set_model_calls.lock().unwrap().is_empty(),
        "same-model switch must not call set_model"
    );
    assert_eq!(
        app.provider_session_id.as_deref(),
        Some("upstream-session-9"),
        "same-model switch must keep the provider session id"
    );
    assert_eq!(
        app.session.provider_session_id.as_deref(),
        Some("upstream-session-9")
    );
}

#[test]
fn model_switch_updates_session_and_persists_default_model() {
    with_temp_jcode_home(|| {
        let (mut app, model, set_model_calls) = create_model_switch_probe_app();
        app.provider_session_id = Some("stale-session".to_string());

        assert!(super::model_context::handle_model_command(&mut app, "/model gpt-5.6"));

        assert_eq!(*model.lock().unwrap(), "gpt-5.6");
        assert_eq!(
            set_model_calls.lock().unwrap().as_slice(),
            &["gpt-5.6".to_string()]
        );
        assert_eq!(app.session.model.as_deref(), Some("gpt-5.6"));
        assert_eq!(
            app.provider_session_id, None,
            "a real switch must reset the provider session id"
        );
        let cfg = crate::config::config();
        assert_eq!(
            cfg.provider.default_model.as_deref(),
            Some("gpt-5.6"),
            "/model switch must persist the new default model"
        );
    });
}

#[test]
fn model_switch_persist_failure_does_not_block_the_switch() {
    with_temp_jcode_home(|| {
        let (mut app, model, _) = create_model_switch_probe_app();
        // Make the config path unwritable by pointing JCODE_HOME at a file so
        // `Config::save` cannot create config.toml underneath it.
        let home = crate::storage::jcode_dir().expect("test home");
        std::fs::create_dir_all(&home).expect("home dir");
        let config_path = home.join("config.toml");
        std::fs::write(&config_path, b"[provider]\n").expect("seed config");
        // Replace the config file with a directory so load/save fails cleanly.
        std::fs::remove_file(&config_path).expect("remove seed config");
        std::fs::create_dir(&config_path).expect("config path as directory");
        crate::config::invalidate_config_cache();

        assert!(super::model_context::handle_model_command(&mut app, "/model gpt-5.6"));

        assert_eq!(*model.lock().unwrap(), "gpt-5.6", "switch must still apply");
        assert!(app.display_messages.iter().any(|message| {
            message
                .content
                .contains("failed to save as default model")
        }));
    });
}

#[test]
fn provider_command_without_argument_shows_usage() {
    with_temp_jcode_home(|| {
        // Seed only a named profile; the bare `/provider` listing must show
        // configured providers only - no hardcoded built-in ids that are not
        // configured (issue: advertised claude/openai/openrouter/copilot/...).
        let home = crate::storage::jcode_dir().expect("test home");
        std::fs::create_dir_all(&home).expect("home dir");
        std::fs::write(
            home.join("config.toml"),
            r#"
[providers.deepseek-official]
type = "openai-compatible"
base_url = "https://api.deepseek.com/anthropic"
api_key_env = "MY_DEEPSEEK_API_KEY"
default_model = "deepseek-v4-flash"
"#,
        )
        .expect("seed config");
        crate::config::invalidate_config_cache();
        crate::auth::AuthStatus::invalidate_cache();

        let (mut app, _, set_model_calls) = create_model_switch_probe_app();
        // A bare `/provider` must be recognized (not fall through to the
        // "Unknown skill" fallback) and show usage + available providers.
        assert!(super::model_context::handle_provider_command(
            &mut app,
            "/provider"
        ));
        let usage = app
            .display_messages
            .iter()
            .find(|message| message.content.contains("Usage: /provider <name>"))
            .map(|message| message.content.clone())
            .unwrap_or_else(|| {
                panic!(
                    "bare /provider should show usage, got: {:?}",
                    app.display_messages
                )
            });
        assert!(
            usage.contains("deepseek-official"),
            "listing must include the configured named profile: {usage}"
        );
        for unconfigured in ["claude", "openai", "openrouter", "copilot", "gemini"] {
            assert!(
                !usage.contains(unconfigured),
                "listing must not advertise unconfigured built-in provider '{unconfigured}': {usage}"
            );
        }
        assert!(
            set_model_calls.lock().unwrap().is_empty(),
            "bare /provider must not trigger a switch"
        );
    });
}

#[test]
fn provider_command_switches_to_named_profile_model() {
    with_temp_jcode_home(|| {
        // Seed a named provider profile so `provider_default_model_spec`
        // resolves `<profile>:<default_model>`.
        let home = crate::storage::jcode_dir().expect("test home");
        std::fs::create_dir_all(&home).expect("home dir");
        std::fs::write(
            home.join("config.toml"),
            r#"
[providers.deepseek-official]
type = "openai-compatible"
base_url = "https://api.deepseek.com/anthropic"
api = "anthropic"
auth = "header"
auth_header = "x-api-key"
api_key_env = "MY_DEEPSEEK_API_KEY"
default_model = "deepseek-v4-flash"
"#,
        )
        .expect("seed config");
        crate::config::invalidate_config_cache();

        let (mut app, model, set_model_calls) = create_model_switch_probe_app();
        assert!(super::model_context::handle_provider_command(
            &mut app,
            "/provider deepseek-official"
        ));
        assert_eq!(
            set_model_calls.lock().unwrap().as_slice(),
            &["deepseek-official:deepseek-v4-flash".to_string()]
        );
        assert_eq!(
            *model.lock().unwrap(),
            "deepseek-official:deepseek-v4-flash"
        );
        let cfg = crate::config::config();
        assert_eq!(
            cfg.provider.default_model.as_deref(),
            Some("deepseek-official:deepseek-v4-flash"),
            "/provider switch must persist the new default model"
        );
    });
}

#[test]
fn provider_command_unknown_provider_reports_error() {
    with_temp_jcode_home(|| {
        let (mut app, _, set_model_calls) = create_model_switch_probe_app();
        assert!(super::model_context::handle_provider_command(
            &mut app,
            "/provider nope"
        ));
        assert!(
            app.display_messages
                .iter()
                .any(|message| message.content.contains("Unknown provider")),
            "expected Unknown provider error, got: {:?}",
            app.display_messages
        );
        assert!(
            set_model_calls.lock().unwrap().is_empty(),
            "no set_model call for an unknown provider"
        );
    });
}

#[test]
fn provider_default_model_spec_maps_builtin_and_named() {
    with_temp_jcode_home(|| {
        let home = crate::storage::jcode_dir().expect("test home");
        std::fs::create_dir_all(&home).expect("home dir");
        std::fs::write(
            home.join("config.toml"),
            r#"
[providers.my-gw]
type = "openai-compatible"
base_url = "http://localhost:8080/v1"
default_model = "model-a"
"#,
        )
        .expect("seed config");
        crate::config::invalidate_config_cache();

        assert_eq!(
            super::model_context::provider_default_model_spec("claude").as_deref(),
            Some("claude-fable-5")
        );
        assert_eq!(
            super::model_context::provider_default_model_spec("openai").as_deref(),
            Some("gpt-5.6-sol")
        );
        assert_eq!(
            super::model_context::provider_default_model_spec("my-gw").as_deref(),
            Some("my-gw:model-a")
        );
        assert_eq!(
            super::model_context::provider_default_model_spec("nope"),
            None
        );
    });
}

#[test]
fn remote_model_switch_rejects_while_turn_is_running() {
    with_temp_jcode_home(|| {
        let mut app = create_test_app();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let mut remote = crate::tui::backend::RemoteConnection::dummy();

        app.is_remote = true;
        // Seed a concrete current model + provider key so the ranked command
        // suggestions are non-empty; the provider-prefixed request below then
        // matches no suggestion, leaving Enter to reach the runtime switch
        // handler instead of being consumed by the suggestion system.
        app.remote_provider_model = Some("gpt-5.5".to_string());
        app.session.provider_key = Some("openai-api".to_string());
        app.is_processing = true;
        app.input = "/model openai-api:gpt-5.6".to_string();
        rt.block_on(app.handle_remote_key(KeyCode::Enter, KeyModifiers::NONE, &mut remote))
        .expect("remote key handling should not error");

        let last = app.display_messages.last().expect("error message");
        assert!(
            last.content.contains("Cannot switch models"),
            "expected busy rejection, got: {}",
            last.content
        );
        assert_eq!(app.status_notice(), Some("Model switch busy".to_string()));
    });
}

#[test]
fn model_switch_accepts_slash_ref_as_resonix_alias() {
    // resonix 对齐：`/model provider/model` 斜杠引用与冒号前缀等价，
    // 由 MultiProvider::set_model 的 explicit_model_provider_prefix 解析。
    let (mut app, _, set_model_calls) = create_model_switch_probe_app();

    // Probe provider 没有 provider 前缀路由能力，斜杠会被原样传给 set_model
    // （作为普通模型名）；这不影响解析层正确性，只验证命令路径可达。
    assert!(super::model_context::handle_model_command(&mut app, "/model openai/gpt-5.6"));
    let calls = set_model_calls.lock().unwrap();
    assert_eq!(calls.last().map(String::as_str), Some("openai/gpt-5.6"));
}

#[test]
fn remote_model_switch_same_model_is_noop() {
    with_temp_jcode_home(|| {
        let mut app = create_test_app();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let mut remote = crate::tui::backend::RemoteConnection::dummy();

        app.is_remote = true;
        app.remote_provider_model = Some("gpt-5.5".to_string());
        app.session.provider_key = Some("openai-api".to_string());
        app.input = "/model openai-api:gpt-5.5".to_string();
        rt.block_on(app.handle_remote_key(KeyCode::Enter, KeyModifiers::NONE, &mut remote))
        .expect("remote key handling should not error");

        let last = app.display_messages.last().expect("notice");
        assert!(
            last.content.contains("Already using model"),
            "expected no-op notice, got: {}",
            last.content
        );
        assert_eq!(
            app.status_notice(),
            Some("Already using model: gpt-5.5".to_string())
        );
    });
}
