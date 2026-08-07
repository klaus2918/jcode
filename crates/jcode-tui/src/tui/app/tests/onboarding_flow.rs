// Integration tests for the first-run onboarding flow control logic.

use super::onboarding_flow::{OnboardingFlow, OnboardingPhase};

#[derive(Clone)]
struct QualityFirstOpenAiProvider {
    model: std::sync::Arc<std::sync::RwLock<String>>,
}

#[async_trait::async_trait]
impl Provider for QualityFirstOpenAiProvider {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[crate::message::ToolDefinition],
        _system: &str,
        _resume_session_id: Option<&str>,
    ) -> Result<crate::provider::EventStream> {
        unimplemented!("QualityFirstOpenAiProvider")
    }

    fn name(&self) -> &str {
        "OpenAI"
    }

    fn model(&self) -> String {
        self.model.read().unwrap().clone()
    }

    fn model_routes(&self) -> Vec<crate::provider::ModelRoute> {
        vec![
            crate::provider::ModelRoute { capability: None,
                model: "claude-opus-5".to_string(),
                provider: "Anthropic".to_string(),
                api_method: "claude-oauth".to_string(),
                available: true,
                detail: String::new(),
                cheapness: None,
            },
            crate::provider::ModelRoute { capability: None,
                model: "gpt-5.1".to_string(),
                provider: "OpenAI".to_string(),
                api_method: "openai-api-key".to_string(),
                available: true,
                detail: String::new(),
                cheapness: None,
            },
            crate::provider::ModelRoute { capability: None,
                model: "gpt-5.5".to_string(),
                provider: "OpenAI".to_string(),
                api_method: "openai-api-key".to_string(),
                available: true,
                detail: String::new(),
                cheapness: None,
            },
        ]
    }

    fn set_model(&self, model: &str) -> Result<()> {
        let bare = model.rsplit_once(':').map_or(model, |(_, bare)| bare);
        *self.model.write().unwrap() = bare.to_string();
        Ok(())
    }

    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(self.clone())
    }
}

fn quality_first_openai_test_app() -> App {
    let provider: Arc<dyn Provider> = Arc::new(QualityFirstOpenAiProvider {
        model: std::sync::Arc::new(std::sync::RwLock::new("gpt-5.1".to_string())),
    });
    let runtime = tokio::runtime::Runtime::new().expect("registry runtime");
    let registry = runtime.block_on(crate::tool::Registry::new(provider.clone()));
    App::new_for_test_harness(provider, registry)
}

fn onboarding_test_app() -> App {
    let mut app = create_test_app();
    // Force the flow on regardless of the on-disk new-user heuristic.
    app.onboarding_flow = Some(OnboardingFlow::begin());
    app
}

#[test]
fn onboarding_strongest_model_only_runs_without_explicit_defaults() {
    with_temp_jcode_home(|| {
        let previous_explicit = std::env::var_os("JCODE_INITIAL_PROVIDER_EXPLICIT");
        crate::env::remove_var("JCODE_INITIAL_PROVIDER_EXPLICIT");

        let mut app = onboarding_test_app();
        assert!(app.onboarding_should_prefer_strongest_model());

        let mut config = crate::config::Config::load();
        config.provider.default_model = Some("claude-fable-5".to_string());
        config.save().expect("save explicit model default");
        assert!(!app.onboarding_should_prefer_strongest_model());

        config.provider.default_model = None;
        config.provider.default_provider = Some("openai".to_string());
        config.save().expect("save explicit provider default");
        assert!(!app.onboarding_should_prefer_strongest_model());

        config.provider.default_provider = None;
        config.save().expect("clear explicit defaults");
        crate::env::set_var("JCODE_INITIAL_PROVIDER_EXPLICIT", "1");
        assert!(!app.onboarding_should_prefer_strongest_model());

        app.onboarding_auto_model_selection_active
            .store(true, std::sync::atomic::Ordering::Release);
        app.onboarding_finish();
        assert!(
            !app.onboarding_auto_model_selection_active
                .load(std::sync::atomic::Ordering::Acquire),
            "finishing onboarding must cancel a delayed catalog selection"
        );

        if let Some(value) = previous_explicit {
            crate::env::set_var("JCODE_INITIAL_PROVIDER_EXPLICIT", value);
        } else {
            crate::env::remove_var("JCODE_INITIAL_PROVIDER_EXPLICIT");
        }
    });
}

#[test]
fn onboarding_begins_and_advances_past_model_select() {
    let mut app = create_test_app();
    app.onboarding_flow = None;
    app.begin_onboarding_flow();
    // `begin_onboarding_flow` immediately advances past the legacy ModelSelect
    // phase into the action-only start choice.
    assert!(matches!(
        app.onboarding_phase(),
        Some(OnboardingPhase::StartChoice { .. })
    ));
    // begin is idempotent: a second call does not reset the phase.
    app.begin_onboarding_flow();
    assert!(matches!(
        app.onboarding_phase(),
        Some(OnboardingPhase::StartChoice { .. })
    ));
}

#[test]
fn onboarding_can_begin_at_configure_phase() {
    let mut app = create_test_app();
    app.onboarding_flow = None;
    app.begin_onboarding_flow_at_configure();
    // The live flow always starts at the config-guided provider prompt; the
    // external-login import walkthrough is no longer part of live onboarding.
    assert!(matches!(
        app.onboarding_phase(),
        Some(OnboardingPhase::ConfigureProvider { yes_highlighted: true })
    ));
    // begin_at_configure is idempotent: a second call does not reset the phase.
    if let Some(flow) = app.onboarding_flow.as_mut() {
        flow.phase = OnboardingPhase::Suggestions;
    }
    app.begin_onboarding_flow_at_configure();
    assert!(matches!(
        app.onboarding_phase(),
        Some(OnboardingPhase::Suggestions)
    ));
}

#[test]
fn configure_phase_advances_to_model_select_without_telemetry_prompt() {
    with_temp_jcode_home(|| {
        let mut app = create_test_app();
        app.onboarding_flow = None;
        // Force the "Configure a model provider?" phase so we exercise
        // onboarding_advance_from_configure directly regardless of host logins.
        app.begin_onboarding_flow_at_configure();
        assert!(matches!(
            app.onboarding_phase(),
            Some(OnboardingPhase::ConfigureProvider { .. })
        ));
        // After provider setup we no longer ask a telemetry-consent question; we
        // advance straight through model selection into the first-run start
        // choice and leave content sharing off.
        app.onboarding_advance_from_configure();
        assert!(matches!(
            app.onboarding_phase(),
            Some(OnboardingPhase::ModelSelect)
                | Some(OnboardingPhase::Suggestions)
                | Some(OnboardingPhase::StartChoice { .. })
        ));
    });
}

#[test]
fn configure_phase_is_default_even_when_external_logins_exist() {
    use crate::tui::OnboardingWelcomeKind;
    with_temp_jcode_home(|| {
        // Seed a real, importable Codex login. The first-run flow must NOT
        // walk the user through importing Anthropic/Codex (or any other tool's)
        // auth during onboarding: model access is config-driven, so the welcome
        // screen asks a simple "Configure a model provider?" Yes/No instead.
        let legacy_auth = crate::auth::codex::legacy_auth_file_path().expect("legacy auth path");
        std::fs::create_dir_all(legacy_auth.parent().expect("legacy auth parent"))
            .expect("create legacy auth dir");
        std::fs::write(
            legacy_auth,
            r#"{"OPENAI_API_KEY":"sk-onboarding-test"}"#,
        )
        .expect("seed importable Codex key");
        crate::auth::AuthStatus::invalidate_cache();
        assert!(
            crate::auth::codex::has_unconsented_legacy_credentials(),
            "precondition: the seeded Codex login must be detected as importable"
        );

        let mut app = create_test_app();
        app.onboarding_flow = None;
        app.begin_onboarding_flow_at_configure();
        // No import walkthrough: always the "Configure a model provider?"
        // Yes/No prompt.
        assert!(matches!(
            app.onboarding_phase(),
            Some(OnboardingPhase::ConfigureProvider {
                yes_highlighted: true
            })
        ));
        assert!(matches!(
            app.onboarding_welcome_kind(),
            OnboardingWelcomeKind::ConfigureProvider {
                yes_highlighted: true
            }
        ));
    });
}

#[test]
fn configure_no_finishes_onboarding_with_provider_hint() {
    with_temp_jcode_home(|| {
        let mut app = create_test_app();
        app.onboarding_flow = None;
        app.begin_onboarding_flow_at_configure();
        if let Some(flow) = app.onboarding_flow.as_mut() {
            flow.phase = OnboardingPhase::ConfigureProvider {
                yes_highlighted: true,
            };
        }
        assert!(app.inline_interactive_state.is_none());
        let before = app.display_messages().len();
        // 'n' exits onboarding straight to the normal screen (no flaky inline
        // provider picker) and tells the user to run /login when ready.
        assert!(app.handle_onboarding_continue_prompt_key(KeyCode::Char('n')));
        // No inline picker is opened.
        assert!(app.inline_interactive_state.is_none());
        // Onboarding is finished (Done phase is inactive, so the accessor
        // reports no active phase).
        assert!(app.onboarding_phase().is_none());
        assert!(!app.onboarding_flow_active());
        // A system message guides the user to `jcode provider add`.
        let messages = app.display_messages();
        assert_eq!(messages.len(), before + 1, "exactly one guidance message");
        assert!(
            messages.last().unwrap().content.contains("jcode provider add"),
            "guidance message should mention jcode provider add: {:?}",
            messages.last().unwrap().content
        );
    });
}

#[test]
fn configure_arrows_toggle_highlight() {
    with_temp_jcode_home(|| {
        let mut app = create_test_app();
        app.onboarding_flow = None;
        app.begin_onboarding_flow_at_configure();
        if let Some(flow) = app.onboarding_flow.as_mut() {
            flow.phase = OnboardingPhase::ConfigureProvider {
                yes_highlighted: true,
            };
        }
        // Right highlights No, Left highlights Yes; nothing commits yet.
        assert!(app.handle_onboarding_continue_prompt_key(KeyCode::Right));
        assert!(matches!(
            app.onboarding_phase(),
            Some(OnboardingPhase::ConfigureProvider {
                yes_highlighted: false
            })
        ));
        assert!(app.handle_onboarding_continue_prompt_key(KeyCode::Left));
        assert!(matches!(
            app.onboarding_phase(),
            Some(OnboardingPhase::ConfigureProvider {
                yes_highlighted: true
            })
        ));
        assert!(app.inline_interactive_state.is_none());
    });
}

#[test]
fn continue_prompt_key_ignored_when_not_in_phase() {
    let mut app = create_test_app();
    app.onboarding_flow = None;
    assert!(!app.handle_onboarding_continue_prompt_key(KeyCode::Char('y')));
}

#[test]
fn onboarding_start_choice_is_action_only_and_defaults_to_review() {
    let mut app = onboarding_test_app();
    if let Some(flow) = app.onboarding_flow.as_mut() {
        flow.phase = OnboardingPhase::ModelSelect;
    }

    app.onboarding_after_model_select();

    assert!(matches!(
        app.onboarding_phase(),
        Some(OnboardingPhase::StartChoice { .. })
    ));
    assert_eq!(app.session_picker_mode, SessionPickerMode::Onboarding);
    let picker = app
        .session_picker_overlay
        .as_ref()
        .expect("start choice picker")
        .borrow();
    assert_eq!(picker.visible_session_count(), 0);
    assert!(picker.onboarding_review_recent_project_highlighted());
    assert!(!picker.onboarding_start_new_highlighted());
}

#[test]
fn startup_check_skips_when_session_already_has_activity() {
    with_temp_jcode_home(|| {
        let mut app = create_test_app();
        app.onboarding_flow = None;
        app.onboarding_startup_checked = false;
        // Simulate a resumed session with a real user message.
        app.push_display_message(DisplayMessage::user("what does this repo do?".to_string()));

        app.maybe_begin_onboarding_flow_on_startup();

        // Settled, non-empty state: guard is committed and no flow starts.
        assert!(app.onboarding_startup_checked);
        assert!(app.onboarding_flow.is_none());
    });
}

#[test]
fn startup_check_ignores_synthetic_scaffolding_messages() {
    with_temp_jcode_home(|| {
        let mut app = create_test_app();
        app.onboarding_flow = None;
        app.onboarding_startup_checked = false;
        // Fresh sessions still carry a synthetic system-reminder (role=user) and
        // assorted system scaffolding. These must not count as real activity.
        app.push_display_message(DisplayMessage::user(
            "<system-reminder>\n# Session Context\nDate: 2026-05-30".to_string(),
        ));
        app.push_display_message(DisplayMessage::system("Switched to model: x".to_string()));

        app.maybe_begin_onboarding_flow_on_startup();

        // The guard must not be tripped by scaffolding alone. In a temp home with
        // no working credentials the flow begins at the in-TUI Login phase (the
        // fresh-install path no longer logs in at the CLI before the TUI).
        // Parallel tests can leak credential env vars (ANTHROPIC_API_KEY etc.),
        // which legitimately routes the fresh install through the credentialed
        // post-login path instead. Either way, the flow must have *started*:
        // scaffolding messages must not be mistaken for real activity.
        assert!(
            !app.display_messages.is_empty(),
            "precondition: scaffolding messages present"
        );
        assert!(app.onboarding_startup_checked);
        assert!(
            app.onboarding_flow_active(),
            "scaffolding-only sessions must still enter first-run onboarding"
        );
    });
}

#[test]
fn startup_check_skips_when_input_is_present() {
    with_temp_jcode_home(|| {
        let mut app = create_test_app();
        app.onboarding_flow = None;
        app.onboarding_startup_checked = false;
        app.input = "restored draft".to_string();

        app.maybe_begin_onboarding_flow_on_startup();

        assert!(app.onboarding_startup_checked);
        assert!(app.onboarding_flow.is_none());
    });
}

#[test]
fn startup_check_is_noop_once_committed() {
    with_temp_jcode_home(|| {
        let mut app = create_test_app();
        app.onboarding_flow = None;
        app.onboarding_startup_checked = true;

        app.maybe_begin_onboarding_flow_on_startup();

        // Already committed: never touches the flow.
        assert!(app.onboarding_flow.is_none());
    });
}

#[test]
fn startup_check_skips_selfdev_canary_session() {
    with_temp_jcode_home(|| {
        let mut app = create_test_app();
        app.onboarding_flow = None;
        app.onboarding_startup_checked = false;
        // Self-dev / canary sessions (e.g. the niri `jcode self-dev` hotkey) take
        // a launch path that never bumps `launch_count`, so without this guard the
        // new-user heuristic would re-onboard on every spawn.
        app.session.is_canary = true;

        app.maybe_begin_onboarding_flow_on_startup();

        assert!(app.onboarding_startup_checked);
        assert!(
            app.onboarding_flow.is_none(),
            "self-dev/canary sessions must never auto-start onboarding"
        );
    });
}

#[test]
fn model_validation_success_appends_single_ready_line() {
    let mut app = create_test_app();
    let session_id = app.session.id.clone();
    let before = app.display_messages().len();

    let consumed = app.handle_onboarding_model_validated(crate::bus::OnboardingModelValidated {
        session_id,
        model_label: "GPT-5.5 (low)".to_string(),
        provider_key: Some("openai".to_string()),
        ok: true,
        detail: None,
    });

    assert!(consumed);
    let messages = app.display_messages();
    assert_eq!(messages.len(), before + 1, "exactly one summary block");
    let line = &messages.last().unwrap().content;
    assert!(line.contains("Ready to use"), "has a ready section: {line:?}");
    assert!(
        line.contains("GPT-5.5 (low) (default)"),
        "names the default model: {line:?}"
    );
    assert!(
        line.contains('\u{2713}'),
        "marks ready rows with a check: {line:?}"
    );
}

#[test]
fn model_validation_failure_appends_single_warning_line_with_detail() {
    let mut app = create_test_app();
    let session_id = app.session.id.clone();
    let before = app.display_messages().len();

    let consumed = app.handle_onboarding_model_validated(crate::bus::OnboardingModelValidated {
        session_id,
        model_label: "Claude Opus 4.8".to_string(),
        provider_key: Some("anthropic".to_string()),
        ok: false,
        detail: Some("timed out after 30s".to_string()),
    });

    assert!(consumed);
    let messages = app.display_messages();
    assert_eq!(messages.len(), before + 1, "exactly one summary block");
    let line = &messages.last().unwrap().content;
    assert!(
        line.contains("Needs attention"),
        "has an attention section: {line:?}"
    );
    assert!(
        line.contains("Claude Opus 4.8 (default)"),
        "names the default model: {line:?}"
    );
    assert!(line.contains("timed out after 30s"), "includes detail: {line:?}");
    assert!(line.contains("/model"), "offers a way out: {line:?}");
    assert!(
        line.contains('\u{2715}'),
        "marks attention rows with a cross: {line:?}"
    );
}

#[test]
fn model_validation_auth_failure_offers_login_fix() {
    let mut app = create_test_app();
    let session_id = app.session.id.clone();

    let consumed = app.handle_onboarding_model_validated(crate::bus::OnboardingModelValidated {
        session_id,
        model_label: "Claude Opus 4.8".to_string(),
        provider_key: Some("anthropic".to_string()),
        ok: false,
        detail: Some(
            "Anthropic API error (401 Unauthorized): Invalid authentication credentials"
                .to_string(),
        ),
    });

    assert!(consumed);
    let messages = app.display_messages();
    let line = &messages.last().unwrap().content;
    // Auth failures should point the user at `jcode provider add` to fix
    // credentials, while still offering /model as an alternative.
    assert!(
        line.contains("jcode provider add"),
        "auth failure offers provider config: {line:?}"
    );
    assert!(line.contains("/model"), "still offers /model: {line:?}");
}

#[test]
fn model_validation_ignores_stale_session_result() {
    let mut app = create_test_app();
    let before = app.display_messages().len();

    let consumed = app.handle_onboarding_model_validated(crate::bus::OnboardingModelValidated {
        session_id: "some-other-session".to_string(),
        model_label: "GPT-5.5".to_string(),
        provider_key: Some("openai".to_string()),
        ok: true,
        detail: None,
    });

    assert!(!consumed, "stale result is not consumed");
    assert_eq!(
        app.display_messages().len(),
        before,
        "stale result appends nothing"
    );
}

#[test]
fn remote_post_login_validation_waits_for_catalog_refresh() {
    use crate::tui::app::onboarding_flow::OnboardingPendingValidation;
    with_temp_jcode_home(|| {
        let mut app = create_test_app();
        app.is_remote = true;
        // Simulate the state right after a remote login: a pending validation
        // armed to wait for the catalog generation to advance past 3.
        app.remote_model_catalog_generation = 3;
        app.onboarding_pending_model_validation = Some(
            OnboardingPendingValidation::awaiting_catalog_refresh(app.session.id.clone(), 3),
        );

        // Catalog hasn't refreshed yet (generation unchanged): not ready to fire.
        assert!(!app.onboarding_pending_validation_ready_to_fire());

        // The server pushes the post-login catalog (generation advances): now
        // the validation is ready to fire with the freshly-selected model.
        app.remote_model_catalog_generation = 4;
        assert!(app.onboarding_pending_validation_ready_to_fire());
    });
}

#[test]
fn local_post_import_validation_waits_for_model_activation() {
    use crate::tui::app::onboarding_flow::OnboardingPendingValidation;
    with_temp_jcode_home(|| {
        let mut app = create_test_app();
        app.is_remote = false;
        app.auth_catalog_refresh_pending = true;
        app.onboarding_pending_model_validation = Some(
            OnboardingPendingValidation::awaiting_catalog_refresh(app.session.id.clone(), 0),
        );

        assert!(
            !app.onboarding_pending_validation_ready_to_fire(),
            "Continue must not validate the stale pre-import model"
        );

        app.auth_catalog_refresh_pending = false;
        assert!(
            app.onboarding_pending_validation_ready_to_fire(),
            "validation should start once local provider/model activation finishes"
        );
    });
}

#[test]
fn startup_check_skips_user_with_established_session_history() {
    with_temp_jcode_home(|| {
        // A missing/short launch history alone must NOT classify someone as a
        // new user when their jcode home has a substantial native session
        // history. Seed >=10 native session files in the temp home.
        let sessions_dir = crate::storage::jcode_dir()
            .expect("jcode dir")
            .join("sessions");
        std::fs::create_dir_all(&sessions_dir).expect("create sessions dir");
        for i in 0..10 {
            std::fs::write(
                sessions_dir.join(format!("session_test_{i:02}.json")),
                "{}",
            )
            .expect("write session file");
        }

        let mut app = create_test_app();
        app.onboarding_flow = None;
        app.onboarding_startup_checked = false;

        app.maybe_begin_onboarding_flow_on_startup();

        assert!(app.onboarding_startup_checked);
        assert!(
            app.onboarding_flow.is_none(),
            "established users (many native sessions) must never re-onboard"
        );
    });
}

#[test]
fn startup_check_imported_transcripts_do_not_count_as_history() {
    with_temp_jcode_home(|| {
        // Imported Codex/Claude transcripts exist on genuinely fresh installs
        // that chose to import history; they must not suppress onboarding.
        let sessions_dir = crate::storage::jcode_dir()
            .expect("jcode dir")
            .join("sessions");
        std::fs::create_dir_all(&sessions_dir).expect("create sessions dir");
        for i in 0..20 {
            std::fs::write(
                sessions_dir.join(format!("imported_codex_{i:02}.json")),
                "{}",
            )
            .expect("write imported file");
        }

        let mut app = create_test_app();
        app.onboarding_flow = None;
        app.onboarding_startup_checked = false;

        app.maybe_begin_onboarding_flow_on_startup();

        assert!(app.onboarding_startup_checked);
        assert!(
            app.onboarding_flow.is_some(),
            "imported transcripts alone should still onboard a fresh install"
        );
    });
}

// ---------------------------------------------------------------------------
// Liveness: a first-run user can never be permanently stranded.
//
// The dangerous failure mode is a phase whose only exit depends on an external
// async event (a `LoginCompleted` bus message) that might never arrive. These
// tests prove that from every reachable phase there is *always* a forward path
// using only inputs the user is guaranteed to have: a key press, or the passage
// of time via the tick watchdog. No test here depends on an async event firing.
// ---------------------------------------------------------------------------

/// A phase is a "safe resting/exit state" if the user is no longer trapped by
/// the guided flow: onboarding finished (`None`/`Done`), they reached a ready
/// surface (`Suggestions`/`StartChoice`), or an interactive picker overlay is
/// open for them to act in.
fn onboarding_state_is_escapable(app: &App) -> bool {
    use crate::tui::app::onboarding_flow::OnboardingPhase;
    if app.inline_interactive_state.is_some() || app.session_picker_overlay.is_some() {
        return true;
    }
    match app.onboarding_phase() {
        None => true, // flow finished / inactive
        Some(OnboardingPhase::Suggestions) => true,
        Some(OnboardingPhase::StartChoice { .. }) => true,
        Some(OnboardingPhase::Done) => true,
        _ => false,
    }
}

#[test]
fn liveness_every_configure_phase_has_a_single_keypress_exit() {
    use crate::tui::app::onboarding_flow::OnboardingPhase;
    with_temp_jcode_home(|| {
        // Each interactive Login-family phase must leave itself after exactly one
        // decisive key, with no dependence on an async event. We use the "skip /
        // decline" key, which is always synchronous.
        let cases: Vec<(&str, OnboardingPhase, KeyCode)> = vec![
            // "Configure a model provider?" prompt: "n" declines and finishes
            // onboarding immediately.
            (
                "ConfigureProvider",
                OnboardingPhase::ConfigureProvider {
                    yes_highlighted: true,
                },
                KeyCode::Char('n'),
            ),
        ];
        for (label, phase, key) in cases {
            let mut app = create_test_app();
            app.onboarding_flow = None;
            app.begin_onboarding_flow_at_configure();
            if let Some(flow) = app.onboarding_flow.as_mut() {
                flow.phase = phase;
            }
            assert!(
                !onboarding_state_is_escapable(&app),
                "{label}: precondition - should start trapped in the flow"
            );
            let consumed = app.handle_onboarding_continue_prompt_key(key);
            assert!(consumed, "{label}: the exit key must be consumed");
            assert!(
                onboarding_state_is_escapable(&app),
                "{label}: one key press must reach an escapable state"
            );
        }
    });
}

#[test]
fn liveness_esc_always_exits_onboarding_from_every_guided_phase() {
    use crate::tui::app::onboarding_flow::OnboardingPhase;
    with_temp_jcode_home(|| {
        // The universal escape hatch: from ANY guided pre-ready phase, a single
        // Esc must leave onboarding to the normal screen. This is the strongest
        // liveness guarantee - it doesn't matter how the flow got wedged, Esc
        // always works.
        let phases: Vec<(&str, OnboardingPhase)> = vec![
            (
                "ConfigureProvider",
                OnboardingPhase::ConfigureProvider {
                    yes_highlighted: true,
                },
            ),
            ("ModelSelect", OnboardingPhase::ModelSelect),
        ];
        for (label, phase) in phases {
            let mut app = create_test_app();
            app.onboarding_flow = None;
            app.begin_onboarding_flow_at_configure();
            if let Some(flow) = app.onboarding_flow.as_mut() {
                flow.phase = phase;
            }
            assert!(
                !onboarding_state_is_escapable(&app),
                "{label}: precondition - should start trapped in the flow"
            );
            let consumed = app.handle_onboarding_continue_prompt_key(KeyCode::Esc);
            assert!(consumed, "{label}: Esc must be consumed");
            assert!(
                onboarding_state_is_escapable(&app),
                "{label}: Esc must reach an escapable state"
            );
        }
    });
}

#[test]
fn recent_project_review_prompt_is_bounded_read_only_and_requires_approval() {
    let repository = std::path::Path::new("/home/example/projects/demo");
    let prompt = App::onboarding_recent_project_review_prompt(repository);

    assert_eq!(
        prompt,
        "Find the most critical architecture problems in the repository at \"/home/example/projects/demo\". Do not fix them yet, and ask me whether I want them fixed once you find them."
    );
}

#[test]
fn preparing_recent_project_review_finishes_onboarding_and_seeds_the_first_turn() {
    let mut app = onboarding_test_app();
    let repository = app
        .onboarding_recent_project_path()
        .expect("test session should start in a Git repository");
    let expected = App::onboarding_recent_project_review_prompt(&repository);

    assert!(app.onboarding_prepare_recent_project_review());

    assert!(!app.onboarding_flow_active());
    assert_eq!(app.input, expected);
    assert_eq!(app.cursor_pos, app.input.len());
}

#[test]
fn starting_recent_project_review_runs_as_a_visible_local_turn() {
    let mut app = onboarding_test_app();
    let repository = app
        .onboarding_recent_project_path()
        .expect("test session should start in a Git repository");
    let expected = App::onboarding_recent_project_review_prompt(&repository);

    app.onboarding_start_recent_project_review();

    assert!(!app.onboarding_flow_active());
    assert!(app.pending_turn, "local review should start a local turn");
    assert!(app.is_processing, "local review should enter Sending");
    assert!(app.queued_messages.is_empty());
    assert_eq!(
        app.session.messages.last().and_then(|message| {
            message.content.iter().find_map(|block| match block {
                crate::message::ContentBlock::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
        }),
        Some(expected.as_str())
    );
}

#[test]
fn starting_recent_project_review_queues_remote_turn_without_stuck_sending() {
    let mut app = onboarding_test_app();
    app.is_remote = true;
    let repository = app
        .onboarding_recent_project_path()
        .expect("remote session should provide its working directory");
    let expected = App::onboarding_recent_project_review_prompt(&repository);

    app.onboarding_start_recent_project_review();

    assert!(!app.onboarding_flow_active());
    assert!(
        !app.pending_turn,
        "remote review must not set the local pending-turn flag"
    );
    assert!(
        !app.is_processing,
        "remote review must stay idle until the remote queue dispatches"
    );
    assert!(app.input.is_empty());
    assert_eq!(
        app.queued_messages,
        vec![expected]
    );
}

#[test]
fn recent_project_review_falls_back_cleanly_when_no_repo_is_known() {
    let mut app = onboarding_test_app();
    app.is_remote = true;
    app.session.working_dir = dirs::home_dir().map(|path| path.to_string_lossy().into_owned());

    app.onboarding_start_recent_project_review();

    assert!(!app.pending_turn);
    assert!(app.queued_messages.is_empty());
    assert!(matches!(app.onboarding_phase(), Some(OnboardingPhase::Suggestions)));
    assert!(app.status_notice.as_ref().is_some_and(|(notice, _)| {
        notice.contains("No recent Git repository found")
    }));
}



#[test]
fn start_choice_prefetches_recent_project_so_enter_does_not_block() {
    let mut app = onboarding_test_app();
    assert!(
        app.onboarding_recent_project_prefetch.is_none(),
        "no prefetch before the start choice is shown"
    );

    app.onboarding_open_start_choice();

    let slot = app
        .onboarding_recent_project_prefetch
        .clone()
        .expect("opening the start choice should warm the recent-project lookup");

    // Wait briefly for the background scan; the action must not depend on it.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        if slot.lock().expect("prefetch slot").is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        slot.lock().expect("prefetch slot").is_some(),
        "prefetch should resolve in the background"
    );

    // Opening the choice twice must not spawn a second scan.
    app.onboarding_open_start_choice();
    assert!(
        std::sync::Arc::ptr_eq(
            &slot,
            app.onboarding_recent_project_prefetch
                .as_ref()
                .expect("prefetch retained")
        ),
        "the warm prefetch should be reused"
    );

    // The resolved path is still the repository the session runs in.
    assert_eq!(
        app.onboarding_recent_project_path(),
        crate::import::repo_ranking::resolve_git_root(std::path::Path::new(
            app.session
                .working_dir
                .as_deref()
                .expect("test session working dir")
        ))
    );
}
