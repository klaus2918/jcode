use super::*;
use crate::message::{Message, StreamEvent, ToolDefinition};
use crate::provider::ModelRoute;
use crate::provider::{EventStream, Provider};
use crate::tool::Registry;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::mpsc as tokio_mpsc;
use tokio_stream::wrappers::ReceiverStream;

struct TestProvider;

#[async_trait]
impl Provider for TestProvider {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[ToolDefinition],
        _system: &str,
        _resume_session_id: Option<&str>,
    ) -> Result<EventStream> {
        let (tx, rx) = tokio_mpsc::channel::<Result<StreamEvent>>(4);
        tokio::spawn(async move {
            let _ = tx.send(Ok(StreamEvent::TextDelta("ok".to_string()))).await;
            let _ = tx
                .send(Ok(StreamEvent::MessageEnd {
                    stop_reason: Some("end_turn".to_string()),
                }))
                .await;
        });
        Ok(Box::pin(ReceiverStream::new(rx)))
    }

    fn name(&self) -> &str {
        "test"
    }

    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(Self)
    }
}

#[test]
fn test_parse_tailscale_dns_name_trims_trailing_dot() {
    let payload = br#"{"Self":{"DNSName":"yashmacbook.tailabc.ts.net."}}"#;
    let parsed = parse_tailscale_dns_name(payload);
    assert_eq!(parsed.as_deref(), Some("yashmacbook.tailabc.ts.net"));
}

#[test]
fn test_parse_tailscale_dns_name_handles_missing_or_empty() {
    let missing = br#"{"Self":{}}"#;
    assert!(parse_tailscale_dns_name(missing).is_none());

    let empty = br#"{"Self":{"DNSName":"   "}}"#;
    assert!(parse_tailscale_dns_name(empty).is_none());
}

#[test]
fn test_parse_tailscale_dns_name_invalid_json() {
    assert!(parse_tailscale_dns_name(b"not-json").is_none());
}

#[test]
fn collect_cli_model_names_prefers_available_routes_and_dedupes() {
    let routes = vec![
        ModelRoute {
            capability: None,
            model: "gpt-5.4".to_string(),
            provider: "OpenAI".to_string(),
            api_method: "openai-oauth".to_string(),
            available: true,
            detail: String::new(),
            cheapness: None,
        },
        ModelRoute {
            capability: None,
            model: "gpt-5.4".to_string(),
            provider: "auto".to_string(),
            api_method: "openrouter".to_string(),
            available: true,
            detail: String::new(),
            cheapness: None,
        },
        ModelRoute {
            capability: None,
            model: "openrouter models".to_string(),
            provider: "—".to_string(),
            api_method: "openrouter".to_string(),
            available: false,
            detail: "OPENROUTER_API_KEY not set".to_string(),
            cheapness: None,
        },
    ];

    let models = collect_cli_model_names(
        &routes,
        vec!["gpt-5.4".to_string(), "claude-sonnet-4".to_string()],
    );

    assert_eq!(models, vec!["gpt-5.4", "claude-sonnet-4"]);
}

fn test_route(model: &str, provider: &str, api_method: &str) -> ModelRoute {
    ModelRoute {
        capability: None,
        model: model.to_string(),
        provider: provider.to_string(),
        api_method: api_method.to_string(),
        available: true,
        detail: String::new(),
        cheapness: None,
    }
}

#[test]
fn cli_route_display_uses_typed_api_methods() {
    assert_eq!(cli_api_method_display("openai-oauth"), "oauth");
    assert_eq!(cli_api_method_display("openai-api-key"), "api key");
    assert_eq!(
        cli_api_method_display("openai-compatible:cerebras"),
        "api key"
    );
    assert_eq!(cli_api_method_display("mock-auth:profile"), "mock-auth");
    assert_eq!(
        cli_route_provider_display("DeepSeek", "openrouter"),
        "OpenRouter/DeepSeek"
    );
}

fn test_todo(
    id: &str,
    status: &str,
    priority: &str,
    confidence: Option<u8>,
    completion_confidence: Option<u8>,
) -> crate::todo::TodoItem {
    crate::todo::TodoItem {
        id: id.to_string(),
        content: format!("todo {id}"),
        status: status.to_string(),
        priority: priority.to_string(),
        confidence,
        completion_confidence,
        ..Default::default()
    }
}

#[test]
fn run_auto_poke_followup_targets_below_threshold_todos() {
    let todos = vec![
        test_todo("a", "completed", "high", Some(90), Some(90)),
        test_todo("b", "completed", "low", Some(80), Some(80)),
    ];

    let followup = build_run_auto_poke_follow_up_from_todos(&todos, false, None);

    match followup {
        Some(RunAutoPokeFollowUp::ConfidenceSummary {
            total_todos,
            message,
            ..
        }) => {
            assert_eq!(total_todos, 2);
            assert_eq!(message, crate::todo::TODO_COMPLETION_CONTINUATION_MESSAGE);
            assert!(!message.chars().any(|ch| ch.is_ascii_digit()));
            assert!(message.contains("completion confidence"));
            assert!(!message.to_ascii_lowercase().contains("threshold"));
        }
        _ => panic!("expected confidence-summary follow-up"),
    }
}

#[test]
fn run_auto_poke_followup_challenges_abrupt_confidence_once() {
    let mut todo = test_todo("a", "completed", "high", Some(0), Some(100));
    todo.confidence_history = vec![0, 100];

    let todos = [todo];
    match build_run_auto_poke_follow_up_from_todos(&todos, false, None) {
        Some(RunAutoPokeFollowUp::ConfidenceSummary {
            message,
            confidence_spike_challenge,
            ..
        }) => {
            assert!(confidence_spike_challenge);
            assert_eq!(
                message,
                crate::todo::TODO_CONFIDENCE_SPIKE_CONTINUATION_MESSAGE
            );
        }
        _ => panic!("expected confidence-spike challenge"),
    }
    assert!(build_run_auto_poke_follow_up_from_todos(&todos, true, None).is_none());
}

#[test]
fn run_auto_poke_followup_silent_when_confident_and_earned() {
    // All above threshold and no spikes: the old behavior sent an "all good"
    // summary anyway; now we spend no tokens and end the run.
    let todos = vec![
        {
            let mut todo = test_todo("a", "completed", "high", Some(100), Some(100));
            todo.confidence_history = vec![70, 80, 90, 100];
            todo
        },
        test_todo("b", "completed", "low", Some(98), Some(98)),
    ];
    assert!(build_run_auto_poke_follow_up_from_todos(&todos, false, None).is_none());
}

#[test]
fn run_auto_poke_followup_prioritizes_incomplete_todos() {
    let todos = vec![
        test_todo("a", "completed", "high", Some(95), Some(95)),
        test_todo("b", "in_progress", "medium", Some(80), None),
    ];

    let followup = build_run_auto_poke_follow_up_from_todos(&todos, false, None);

    match followup {
        Some(RunAutoPokeFollowUp::Incomplete { count, message }) => {
            assert_eq!(count, 1);
            assert_eq!(
                message,
                "You have 1 incomplete todo. Continue working, or update the todo tool."
            );
        }
        _ => panic!("expected incomplete-todo follow-up"),
    }
}

/// Headless `jcode run` is what the benchmarks and scripted use go through, so
/// the deferred quality review must reach that path too, not only the TUI.
#[test]
fn run_auto_poke_delivers_the_deferred_gate_digest_before_confidence() {
    let todos = vec![test_todo("a", "completed", "high", Some(80), Some(80))];
    // Without a digest, the confidence gate is what fires.
    assert!(matches!(
        build_run_auto_poke_follow_up_from_todos(&todos, false, None),
        Some(RunAutoPokeFollowUp::ConfidenceSummary { .. })
    ));
    // With one, the weak points are reviewed first, since that work can change
    // the very assessments the confidence gate judges.
    match build_run_auto_poke_follow_up_from_todos(
        &todos,
        false,
        Some("review these points".to_string()),
    ) {
        Some(RunAutoPokeFollowUp::GateDigest { message }) => {
            assert_eq!(message, "review these points");
        }
        _ => panic!("expected the gate digest to take precedence"),
    }
}

/// Open todos mean the agent is still working, so the review must wait for the
/// turn to actually end rather than interrupting mid-flight.
#[test]
fn run_auto_poke_prefers_incomplete_todos_over_the_gate_digest() {
    let todos = vec![test_todo("a", "in_progress", "high", Some(80), None)];
    assert!(matches!(
        build_run_auto_poke_follow_up_from_todos(
            &todos,
            false,
            Some("review these points".to_string())
        ),
        Some(RunAutoPokeFollowUp::Incomplete { .. })
    ));
}

/// Regression: the digest is consumed from the log before the follow-up is
/// chosen, so a turn with open todos must not destroy it. Auto-poke iterates
/// many times with open todos on a long run, and each pass used to silently
/// discard the observations, meaning the reminder never survived to delivery.
#[test]
fn open_todos_do_not_consume_the_pending_gate_digest() {
    let _guard = crate::storage::lock_test_env();
    let previous_home = std::env::var_os("JCODE_HOME");
    let dir = tempfile::TempDir::new().expect("tempdir");
    crate::env::set_var("JCODE_HOME", dir.path());
    let session = "run-gate-digest-open-todos";

    crate::todo::append_gate_observations(
        session,
        &[crate::todo::GateObservation {
            kind: crate::todo::GateObservationKind::IntentUnderstanding,
            group: None,
            score: Some(70),
        }],
    )
    .expect("append");

    let open = vec![test_todo("a", "in_progress", "high", Some(80), None)];
    assert!(matches!(
        build_run_auto_poke_follow_up_from_todos(
            &open,
            false,
            take_run_gate_digest_if_turn_ended(session, false, &open),
        ),
        Some(RunAutoPokeFollowUp::Incomplete { .. })
    ));
    assert!(
        !crate::todo::load_gate_observations(session)
            .expect("reload")
            .is_empty(),
        "observations must survive a poke iteration that still has open work"
    );

    // Once the work closes, the reminder is still there to deliver.
    let done = vec![test_todo("a", "completed", "high", Some(80), Some(100))];
    match build_run_auto_poke_follow_up_from_todos(
        &done,
        false,
        take_run_gate_digest_if_turn_ended(session, false, &done),
    ) {
        Some(RunAutoPokeFollowUp::GateDigest { message }) => {
            assert!(message.starts_with(crate::todo::TODO_GATE_DIGEST_PREFIX));
        }
        other => panic!("expected the preserved digest to be delivered, got {other:?}"),
    }
    assert!(
        crate::todo::load_gate_observations(session)
            .expect("reload")
            .is_empty(),
        "delivering the digest should consume the log"
    );

    match previous_home {
        Some(value) => crate::env::set_var("JCODE_HOME", value),
        None => crate::env::remove_var("JCODE_HOME"),
    }
}

/// The log must be consumed on delivery, or one turn's observations would leak
/// into the next turn and be raised again against work they never described.
#[test]
fn take_run_gate_digest_consumes_the_log_and_respects_delivery() {
    let _guard = crate::storage::lock_test_env();
    let previous_home = std::env::var_os("JCODE_HOME");
    let dir = tempfile::TempDir::new().expect("tempdir");
    crate::env::set_var("JCODE_HOME", dir.path());
    let session = "run-gate-digest";

    crate::todo::append_gate_observations(
        session,
        &[crate::todo::GateObservation {
            kind: crate::todo::GateObservationKind::IntentUnderstanding,
            group: None,
            score: Some(70),
        }],
    )
    .expect("append");

    // Already delivered this turn: no second reminder, and the log is left for
    // the delivering path to have handled.
    assert!(take_run_gate_digest(session, true).is_none());

    let digest = take_run_gate_digest(session, false).expect("unresolved point should surface");
    assert!(digest.starts_with(crate::todo::TODO_GATE_DIGEST_PREFIX));
    // Consumed, so the next turn starts clean.
    assert!(
        crate::todo::load_gate_observations(session)
            .expect("reload")
            .is_empty()
    );
    assert!(take_run_gate_digest(session, false).is_none());

    match previous_home {
        Some(value) => crate::env::set_var("JCODE_HOME", value),
        None => crate::env::remove_var("JCODE_HOME"),
    }
}

#[test]
fn run_auto_poke_followup_rechecks_completion_confidence_until_it_passes() {
    let needs_validation = vec![test_todo("a", "completed", "high", Some(80), Some(80))];
    assert!(matches!(
        build_run_auto_poke_follow_up_from_todos(&needs_validation, false, None),
        Some(RunAutoPokeFollowUp::ConfidenceSummary { .. })
    ));
    assert!(matches!(
        build_run_auto_poke_follow_up_from_todos(&needs_validation, false, None),
        Some(RunAutoPokeFollowUp::ConfidenceSummary { .. })
    ));

    let validated = vec![test_todo("a", "completed", "high", Some(80), Some(100))];
    assert!(matches!(
        build_run_auto_poke_follow_up_from_todos(&validated, false, None),
        Some(RunAutoPokeFollowUp::ConfidenceSummary {
            confidence_spike_challenge: true,
            ..
        })
    ));
    assert!(build_run_auto_poke_follow_up_from_todos(&validated, true, None).is_none());
}

#[test]
fn cli_provider_choice_filter_uses_typed_api_methods() {
    let routes = vec![
        test_route("fixture-model", "OpenAI-compatible", "openai-compatible"),
        test_route("llama-3.1-8b", "Ollama", "openai-compatible"),
        test_route("deepseek/deepseek-v4-pro", "auto", "openrouter"),
    ];

    // openai 已从注册表移除，resolve 返回 None → 过滤保持全量。
    let openai = filter_cli_model_routes_for_choice("openai", &routes);
    assert_eq!(openai.len(), routes.len());

    // ollama 是保留的 OpenRouterLike provider；过滤按通用分支全保留。
    let ollama = filter_cli_model_routes_for_choice("ollama", &routes);
    assert_eq!(ollama.len(), routes.len());
}

#[test]
fn collect_cli_model_names_falls_back_when_no_routes_are_available() {
    let routes = vec![ModelRoute {
        capability: None,
        model: "claude-opus-4-6".to_string(),
        provider: "Anthropic".to_string(),
        api_method: "claude-oauth".to_string(),
        available: false,
        detail: "no credentials".to_string(),
        cheapness: None,
    }];

    let models = collect_cli_model_names(&routes, vec!["gpt-5.4".to_string()]);

    assert_eq!(models, vec!["claude-opus-4-6", "gpt-5.4"]);
}

#[test]
fn list_cli_providers_includes_auto_and_openai() {
    let providers = super::report_info::list_cli_providers();
    assert!(providers.iter().any(|provider| provider.id == "auto"));
    assert!(providers.iter().any(|provider| {
        provider.id == "openai-compatible" && provider.display_name == "OpenAI-compatible"
    }));
}

#[test]
fn version_command_plain_output_includes_core_fields() {
    let report = super::report_info::VersionReport {
        version: "v1.2.3 (abc1234)".to_string(),
        semver: "1.2.3".to_string(),
        base_semver: "1.2.0".to_string(),
        update_semver: "1.2.0".to_string(),
        git_hash: "abc1234".to_string(),
        git_tag: "v1.2.3".to_string(),
        build_time: "2026-03-18 18:00:00 +0000".to_string(),
        git_date: "2026-03-18 17:59:00 +0000".to_string(),
        release_build: false,
    };
    let text = format!(
        "version\t{}\nsemver\t{}\nbase_semver\t{}\nupdate_semver\t{}\ngit_hash\t{}\ngit_tag\t{}\nbuild_time\t{}\ngit_date\t{}\nrelease_build\t{}\n",
        report.version,
        report.semver,
        report.base_semver,
        report.update_semver,
        report.git_hash,
        report.git_tag,
        report.build_time,
        report.git_date,
        report.release_build
    );

    assert!(text.contains("version\tv1.2.3 (abc1234)"));
    assert!(text.contains("semver\t1.2.3"));
    assert!(text.contains("git_hash\tabc1234"));
    assert!(text.contains("release_build\tfalse"));
}

#[tokio::test]
async fn restore_agent_session_if_requested_restores_resumed_session() {
    let _guard = crate::storage::lock_test_env();

    let provider: Arc<dyn Provider> = Arc::new(TestProvider);
    let registry = Registry::new(provider.clone()).await;
    let mut original = crate::agent::Agent::new(provider.clone(), registry);
    let original_session_id = original.session_id().to_string();
    original
        .run_once_capture("seed session for resume test")
        .await
        .expect("seed session");

    let registry = Registry::new(provider.clone()).await;
    let mut resumed = crate::agent::Agent::new(provider, registry);
    let fresh_session_id = resumed.session_id().to_string();
    assert_ne!(fresh_session_id, original_session_id);

    restore_agent_session_if_requested(&mut resumed, Some(&original_session_id))
        .expect("restore session");

    assert_eq!(resumed.session_id(), original_session_id);
}
