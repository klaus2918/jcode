use super::*;

#[test]
fn test_session_exists_roundtrip() {
    let tmp_dir = std::env::temp_dir().join(format!(
        "jcode-session-test-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(tmp_dir.join("sessions")).unwrap();

    assert!(!session_path_in_dir(&tmp_dir, "missing-session").exists());

    let session_path = session_path_in_dir(&tmp_dir, "exists-session");
    std::fs::write(&session_path, "{}").unwrap();
    assert!(session_path.exists());

    let random_id = format!(
        "missing-session-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    assert!(!session_exists(&random_id));
}

#[test]
fn test_debug_memory_profile_reports_messages_and_provider_cache() {
    let mut session = Session::create_with_id(
        "session_memory_profile_test".to_string(),
        None,
        Some("Memory profile".to_string()),
    );
    session.add_message(
        Role::User,
        vec![ContentBlock::Text {
            text: "hello world".to_string(),
            cache_control: None,
        }],
    );
    session.add_message(
        Role::Assistant,
        vec![
            ContentBlock::ToolUse {
                id: "tool_1".to_string(),
                name: "bash".to_string(),
                input: serde_json::json!({"command": "echo hi"}),
            },
            ContentBlock::ToolResult {
                tool_use_id: "tool_1".to_string(),
                content: "hi".to_string(),
                is_error: None,
            },
        ],
    );

    let _ = session.provider_messages();
    let profile = session.debug_memory_profile();

    assert_eq!(profile["messages"]["count"], 2);
    assert_eq!(profile["messages"]["memory"]["text_blocks"], 1);
    assert_eq!(profile["messages"]["memory"]["tool_use_blocks"], 1);
    assert_eq!(profile["messages"]["memory"]["tool_result_blocks"], 1);
    assert!(profile["messages"]["json_bytes"].as_u64().unwrap_or(0) > 0);
    assert_eq!(profile["provider_messages_cache"]["count"], 2);
    assert!(
        profile["provider_messages_cache"]["json_bytes"]
            .as_u64()
            .unwrap_or(0)
            > 0
    );
}

#[test]
fn load_startup_stub_preserves_metadata_but_skips_heavy_vectors() {
    let _env_lock = lock_env();
    let temp_home = tempfile::Builder::new()
        .prefix("jcode-startup-stub-test-")
        .tempdir()
        .expect("create temp JCODE_HOME");
    let _home = EnvVarGuard::set("JCODE_HOME", temp_home.path().as_os_str());

    let session_id = "session_startup_stub_roundtrip";
    let mut session = Session::create_with_id(
        session_id.to_string(),
        Some("parent_123".to_string()),
        Some("startup stub".to_string()),
    );
    session.model = Some("gpt-5.4".to_string());
    session.provider_key = Some("openai".to_string());
    session.set_canary("self-dev");
    session.append_stored_message(StoredMessage {
        id: "msg_1".to_string(),
        role: Role::User,
        content: vec![ContentBlock::Text {
            text: "hello world".to_string(),
            cache_control: None,
        }],
        display_role: None,
        timestamp: Some(Utc::now()),
        tool_duration_ms: None,
        token_usage: None,
    });
    session.record_env_snapshot(EnvSnapshot {
        captured_at: Utc::now(),
        reason: "resume".to_string(),
        session_id: session_id.to_string(),
        working_dir: Some(temp_home.path().to_string_lossy().to_string()),
        provider: "openai".to_string(),
        model: "gpt-5.4".to_string(),
        jcode_version: "test".to_string(),
        jcode_git_hash: Some("abc123".to_string()),
        jcode_git_dirty: Some(false),
        os: "linux".to_string(),
        arch: "x86_64".to_string(),
        pid: 123,
        is_selfdev: true,
        is_debug: false,
        is_canary: true,
        testing_build: Some("self-dev".to_string()),
        working_git: None,
    });
    session.record_memory_injection(
        "summary".to_string(),
        "content".to_string(),
        1,
        5,
        Vec::new(),
    );
    session.record_replay_display_message("system", Some("Launch".to_string()), "boot");
    session.save().expect("save session");

    let stub = Session::load_startup_stub(session_id).expect("load startup stub");
    assert_eq!(stub.id, session_id);
    assert_eq!(stub.parent_id.as_deref(), Some("parent_123"));
    assert_eq!(stub.title.as_deref(), Some("startup stub"));
    assert_eq!(stub.model.as_deref(), Some("gpt-5.4"));
    assert_eq!(stub.provider_key.as_deref(), Some("openai"));
    assert!(stub.is_canary);
    assert!(stub.messages.is_empty());
    assert!(stub.env_snapshots.is_empty());
    assert!(stub.memory_injections.is_empty());
    assert!(stub.replay_events.is_empty());
}

#[test]
fn load_for_remote_startup_preserves_messages_and_replay_but_skips_heavy_vectors() {
    let _env_lock = lock_env();
    let temp_home = tempfile::Builder::new()
        .prefix("jcode-remote-startup-test-")
        .tempdir()
        .expect("create temp JCODE_HOME");
    let _home = EnvVarGuard::set("JCODE_HOME", temp_home.path().as_os_str());

    let session_id = "session_remote_startup_roundtrip";
    let mut session = Session::create_with_id(
        session_id.to_string(),
        Some("parent_remote".to_string()),
        Some("remote startup".to_string()),
    );
    session.model = Some("gpt-5.4".to_string());
    session.append_stored_message(StoredMessage {
        id: "msg_remote_1".to_string(),
        role: Role::Assistant,
        content: vec![ContentBlock::Text {
            text: "hello remote startup".to_string(),
            cache_control: None,
        }],
        display_role: None,
        timestamp: Some(Utc::now()),
        tool_duration_ms: None,
        token_usage: None,
    });
    session.record_env_snapshot(EnvSnapshot {
        captured_at: Utc::now(),
        reason: "resume".to_string(),
        session_id: session_id.to_string(),
        working_dir: Some(temp_home.path().to_string_lossy().to_string()),
        provider: "openai".to_string(),
        model: "gpt-5.4".to_string(),
        jcode_version: "test".to_string(),
        jcode_git_hash: Some("abc123".to_string()),
        jcode_git_dirty: Some(false),
        os: "linux".to_string(),
        arch: "x86_64".to_string(),
        pid: 123,
        is_selfdev: false,
        is_debug: false,
        is_canary: false,
        testing_build: None,
        working_git: None,
    });
    session.record_memory_injection(
        "summary".to_string(),
        "content".to_string(),
        1,
        5,
        Vec::new(),
    );
    session.record_replay_display_message("system", Some("Launch".to_string()), "boot");
    session.save().expect("save session");

    let loaded = Session::load_for_remote_startup(session_id).expect("load remote startup");
    assert_eq!(loaded.id, session_id);
    assert_eq!(loaded.parent_id.as_deref(), Some("parent_remote"));
    assert_eq!(loaded.model.as_deref(), Some("gpt-5.4"));
    assert_eq!(loaded.messages.len(), 1);
    assert!(loaded.replay_events.is_empty());
    assert!(loaded.env_snapshots.is_empty());
    assert!(loaded.memory_injections.is_empty());
}

#[test]
fn test_create_marks_debug_when_test_session_env_enabled() {
    let _env_lock = lock_env();
    let _test_flag = EnvVarGuard::set("JCODE_TEST_SESSION", "1");

    let s1 = Session::create(None, None);
    assert!(s1.is_debug);

    let s2 = Session::create_with_id("session_test_1".to_string(), None, None);
    assert!(s2.is_debug);
}

#[test]
fn test_create_not_debug_when_test_session_env_disabled() {
    let _env_lock = lock_env();
    let _test_flag = EnvVarGuard::set("JCODE_TEST_SESSION", "0");

    let s = Session::create(None, None);
    assert!(!s.is_debug);
}

#[test]
fn test_recover_crashed_sessions_preserves_debug_flag() {
    let _env_lock = lock_env();
    let temp_home = tempfile::Builder::new()
        .prefix("jcode-recover-debug-test-")
        .tempdir()
        .expect("create temp JCODE_HOME");
    let _home = EnvVarGuard::set("JCODE_HOME", temp_home.path().as_os_str());
    let _test_flag = EnvVarGuard::set("JCODE_TEST_SESSION", "0");

    let mut crashed = Session::create_with_id(
        "session_recover_debug_source".to_string(),
        None,
        Some("debug source".to_string()),
    );
    crashed.is_debug = true;
    crashed.mark_crashed(Some("test crash".to_string()));
    crashed.add_message(
        Role::User,
        vec![ContentBlock::Text {
            text: "hello".to_string(),
            cache_control: None,
        }],
    );
    crashed.save().expect("save crashed session");

    let recovered_ids = recover_crashed_sessions().expect("recover crashed sessions");
    assert_eq!(recovered_ids.len(), 1);

    let recovered = Session::load(&recovered_ids[0]).expect("load recovered session");
    assert!(recovered.is_debug);
}

#[test]
fn test_save_persists_full_session_content() {
    let _env_lock = lock_env();
    let temp_home = tempfile::Builder::new()
        .prefix("jcode-session-save-test-")
        .tempdir()
        .expect("create temp JCODE_HOME");
    let _home = EnvVarGuard::set("JCODE_HOME", temp_home.path().as_os_str());

    let mut session = Session::create_with_id(
        "session_save_persist_test".to_string(),
        None,
        Some("save fidelity test".to_string()),
    );

    session.add_message(
        Role::User,
        vec![ContentBlock::ToolResult {
            tool_use_id: "tool_1".to_string(),
            content: "OPENROUTER_API_KEY=sk-or-v1-abcdefghijklmnopqrstuvwxyz0123456789".to_string(),
            is_error: None,
        }],
    );

    session.add_message(
        Role::Assistant,
        vec![ContentBlock::ToolUse {
            id: "tool_2".to_string(),
            name: "bash".to_string(),
            input: serde_json::json!({
                "command": "echo ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123"
            }),
        }],
    );

    session.save().expect("save session");

    let loaded = Session::load("session_save_persist_test").expect("load saved session");

    match &loaded.messages[0].content[0] {
        ContentBlock::ToolResult { content, .. } => {
            assert!(content.contains("sk-or-v1-abcdefghijklmnopqrstuvwxyz0123456789"));
            assert!(!content.contains("[REDACTED_SECRET]"));
        }
        _ => panic!("expected tool result block"),
    }

    match &loaded.messages[1].content[0] {
        ContentBlock::ToolUse { input, .. } => {
            let input_str = input.to_string();
            assert!(input_str.contains("ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123"));
            assert!(!input_str.contains("[REDACTED_SECRET]"));
        }
        _ => panic!("expected tool use block"),
    }
}

#[test]
fn test_save_persists_compaction_state() {
    let _env_lock = lock_env();
    let temp_home = tempfile::Builder::new()
        .prefix("jcode-session-compaction-save-test-")
        .tempdir()
        .expect("create temp JCODE_HOME");
    let _home = EnvVarGuard::set("JCODE_HOME", temp_home.path().as_os_str());

    let mut session = Session::create_with_id(
        "session_compaction_persist_test".to_string(),
        None,
        Some("compaction persistence test".to_string()),
    );
    session.compaction = Some(StoredCompactionState {
        summary_text: "saved summary".to_string(),
        openai_encrypted_content: None,
        covers_up_to_turn: 8,
        original_turn_count: 8,
        compacted_count: 8,
    });

    session.save().expect("save session with compaction state");

    let loaded = Session::load("session_compaction_persist_test").expect("load saved session");
    assert_eq!(loaded.compaction, session.compaction);
}

#[test]
fn test_save_persists_provider_key() {
    let _env_lock = lock_env();
    let temp_home = tempfile::Builder::new()
        .prefix("jcode-session-provider-key-save-test-")
        .tempdir()
        .expect("create temp JCODE_HOME");
    let _home = EnvVarGuard::set("JCODE_HOME", temp_home.path().as_os_str());

    let mut session = Session::create_with_id(
        "session_provider_key_persist_test".to_string(),
        None,
        Some("provider key persistence test".to_string()),
    );
    session.provider_key = Some("opencode".to_string());
    session.model = Some("anthropic/claude-sonnet-4".to_string());

    session.save().expect("save session with provider key");

    let loaded = Session::load("session_provider_key_persist_test")
        .expect("load saved session with provider key");
    assert_eq!(loaded.provider_key.as_deref(), Some("opencode"));
    assert_eq!(loaded.model.as_deref(), Some("anthropic/claude-sonnet-4"));
}

#[test]
fn test_save_appends_journal_and_load_replays_it() {
    let _env_lock = lock_env();
    let temp_home = tempfile::Builder::new()
        .prefix("jcode-session-journal-test-")
        .tempdir()
        .expect("create temp JCODE_HOME");
    let _home = EnvVarGuard::set("JCODE_HOME", temp_home.path().as_os_str());

    let mut session = Session::create_with_id(
        "session_journal_append_test".to_string(),
        None,
        Some("journal append test".to_string()),
    );
    session.add_message(
        Role::User,
        vec![ContentBlock::Text {
            text: "first".to_string(),
            cache_control: None,
        }],
    );
    session.save().expect("save initial snapshot");

    let snapshot_path = session_path("session_journal_append_test").expect("snapshot path");
    let journal_path = session_journal_path("session_journal_append_test").expect("journal path");
    assert!(snapshot_path.exists());
    assert!(!journal_path.exists());

    session.add_message(
        Role::Assistant,
        vec![ContentBlock::Text {
            text: "second".to_string(),
            cache_control: None,
        }],
    );
    session.save().expect("append journal delta");

    assert!(journal_path.exists());
    let journal = std::fs::read_to_string(&journal_path).expect("read journal");
    assert!(journal.contains("second"));

    let loaded = Session::load("session_journal_append_test").expect("load with journal");
    assert_eq!(loaded.messages.len(), 2);
    assert_eq!(loaded.messages[1].content_preview(), "second");
}

#[test]
fn test_save_checkpoints_after_full_mutation_and_clears_journal() {
    let _env_lock = lock_env();
    let temp_home = tempfile::Builder::new()
        .prefix("jcode-session-checkpoint-test-")
        .tempdir()
        .expect("create temp JCODE_HOME");
    let _home = EnvVarGuard::set("JCODE_HOME", temp_home.path().as_os_str());

    let mut session = Session::create_with_id(
        "session_journal_checkpoint_test".to_string(),
        None,
        Some("checkpoint test".to_string()),
    );
    session.add_message(
        Role::User,
        vec![ContentBlock::Text {
            text: "one".to_string(),
            cache_control: None,
        }],
    );
    session.save().expect("save initial snapshot");

    session.add_message(
        Role::Assistant,
        vec![ContentBlock::Text {
            text: "two".to_string(),
            cache_control: None,
        }],
    );
    session.save().expect("save journal append");

    let journal_path =
        session_journal_path("session_journal_checkpoint_test").expect("journal path");
    assert!(journal_path.exists());

    session.truncate_messages(1);
    session.title = Some("checkpointed title".to_string());
    session.save().expect("checkpoint snapshot");

    assert!(!journal_path.exists());

    let loaded =
        Session::load("session_journal_checkpoint_test").expect("load checkpointed session");
    assert_eq!(loaded.title.as_deref(), Some("checkpointed title"));
    assert_eq!(loaded.messages.len(), 1);
    assert_eq!(loaded.messages[0].content_preview(), "one");
}

#[test]
fn test_redacted_for_export_redacts_tool_result_and_tool_input() {
    let mut session = Session::create_with_id(
        "session_redact_persist_test".to_string(),
        None,
        Some("redaction test".to_string()),
    );

    session.add_message(
        Role::User,
        vec![ContentBlock::ToolResult {
            tool_use_id: "tool_1".to_string(),
            content: "OPENROUTER_API_KEY=sk-or-v1-abcdefghijklmnopqrstuvwxyz0123456789".to_string(),
            is_error: None,
        }],
    );

    session.add_message(
        Role::Assistant,
        vec![ContentBlock::ToolUse {
            id: "tool_2".to_string(),
            name: "bash".to_string(),
            input: serde_json::json!({
                "command": "echo ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123"
            }),
        }],
    );

    let persisted = session.redacted_for_export();

    let first_content = &persisted.messages[0].content[0];
    match first_content {
        ContentBlock::ToolResult { content, .. } => {
            assert!(content.contains("OPENROUTER_API_KEY=[REDACTED_SECRET]"));
            assert!(!content.contains("sk-or-v1-abcdefghijklmnopqrstuvwxyz0123456789"));
        }
        _ => panic!("expected tool result block"),
    }

    let second_content = &persisted.messages[1].content[0];
    match second_content {
        ContentBlock::ToolUse { input, .. } => {
            let input_str = input.to_string();
            assert!(input_str.contains("[REDACTED_SECRET]"));
            assert!(!input_str.contains("ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123"));
        }
        _ => panic!("expected tool use block"),
    }
}

#[test]
fn test_redacted_for_export_redacts_replay_events() {
    let mut session = Session::create_with_id(
        "session_redacted_replay_events_test".to_string(),
        None,
        Some("redacted replay events".to_string()),
    );

    session.record_replay_display_message(
        "swarm",
        Some("DM from fox".to_string()),
        "OPENROUTER_API_KEY=sk-or-v1-secret-value",
    );
    session.record_swarm_status_event(vec![crate::protocol::SwarmMemberStatus {
        session_id: "session_fox".to_string(),
        friendly_name: Some("fox".to_string()),
        status: "running".to_string(),
        detail: Some("ANTHROPIC_API_KEY=sk-ant-secret-value".to_string()),
        role: Some("agent".to_string()),
    }]);
    session.record_swarm_plan_event(
        "swarm_test".to_string(),
        1,
        vec![crate::plan::PlanItem {
            content: "OPENROUTER_API_KEY=sk-or-v1-abcdefghijklmnopqrstuvwxyz0123456789".to_string(),
            status: "pending".to_string(),
            priority: "high".to_string(),
            id: "task-1".to_string(),
            blocked_by: vec![],
            assigned_to: None,
        }],
        vec![],
        Some("ANTHROPIC_API_KEY=sk-ant-secret-value".to_string()),
    );

    let redacted = session.redacted_for_export();
    assert_eq!(redacted.replay_events.len(), 3);

    match &redacted.replay_events[0].kind {
        StoredReplayEventKind::DisplayMessage { content, .. } => {
            assert!(content.contains("OPENROUTER_API_KEY=[REDACTED_SECRET]"));
            assert!(!content.contains("sk-or-v1-secret-value"));
        }
        other => panic!("expected display message replay event, got {other:?}"),
    }

    match &redacted.replay_events[1].kind {
        StoredReplayEventKind::SwarmStatus { members } => {
            let detail = members[0].detail.as_deref().unwrap_or_default();
            assert!(detail.contains("ANTHROPIC_API_KEY=[REDACTED_SECRET]"));
            assert!(!detail.contains("sk-ant-secret-value"));
        }
        other => panic!("expected swarm status replay event, got {other:?}"),
    }

    match &redacted.replay_events[2].kind {
        StoredReplayEventKind::SwarmPlan { items, reason, .. } => {
            assert!(
                items[0]
                    .content
                    .contains("OPENROUTER_API_KEY=[REDACTED_SECRET]")
            );
            assert!(
                !items[0]
                    .content
                    .contains("sk-or-v1-abcdefghijklmnopqrstuvwxyz0123456789")
            );
            let reason = reason.as_deref().unwrap_or_default();
            assert!(reason.contains("ANTHROPIC_API_KEY=[REDACTED_SECRET]"));
            assert!(!reason.contains("sk-ant-secret-value"));
        }
        other => panic!("expected swarm plan replay event, got {other:?}"),
    }
}

#[test]
fn test_summarize_tool_calls_includes_tool_only_assistant_messages() {
    let mut session = Session::create_with_id(
        "session_tool_summary_test".to_string(),
        None,
        Some("tool summary test".to_string()),
    );

    session.add_message(
        Role::Assistant,
        vec![ContentBlock::ToolUse {
            id: "tool_1".to_string(),
            name: "bash".to_string(),
            input: serde_json::json!({
                "command": "pwd"
            }),
        }],
    );

    let summaries = summarize_tool_calls(&session, 10);
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].tool_name, "bash");
    assert!(summaries[0].brief_output.contains("pwd"));
}

#[test]
fn test_render_messages_honors_system_display_role_override() {
    let mut session = Session::create_with_id(
        "session_display_role_test".to_string(),
        None,
        Some("display role test".to_string()),
    );

    session.add_message_with_display_role(
        Role::User,
        vec![ContentBlock::Text {
            text: "[Background Task Completed]\nTask: abc123 (bash)".to_string(),
            cache_control: None,
        }],
        Some(StoredDisplayRole::System),
    );

    let rendered = render_messages(&session);
    assert_eq!(rendered.len(), 1);
    assert_eq!(rendered[0].role, "system");
    assert!(rendered[0].content.contains("Background Task Completed"));
}

#[test]
fn test_render_messages_honors_background_task_display_role_override() {
    let mut session = Session::create_with_id(
        "session_background_task_role_test".to_string(),
        None,
        Some("background task role test".to_string()),
    );

    session.add_message_with_display_role(
            Role::User,
            vec![ContentBlock::Text {
                text: "**Background task** `abc123` · `bash` · ✓ completed · 7.1s · exit 0\n\n_No output captured._\n\n_Full output:_ `bg action=\"output\" task_id=\"abc123\"`".to_string(),
                cache_control: None,
            }],
            Some(StoredDisplayRole::BackgroundTask),
        );

    let rendered = render_messages(&session);
    assert_eq!(rendered.len(), 1);
    assert_eq!(rendered[0].role, "background_task");
    assert!(rendered[0].content.contains("**Background task**"));
}
