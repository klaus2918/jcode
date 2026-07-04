// Tests for the SwarmPlan -> inline chat plan-graph pipeline and the
// plan-scope notification quieting (status line only, no chat card).

#[test]
fn test_swarm_plan_event_pushes_inline_plan_graph_message() {
    let mut app = create_test_app();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();
    let mut remote = crate::tui::backend::RemoteConnection::dummy();
    remote.mark_history_loaded();

    let item = crate::plan::PlanItem {
        content: "write a haiku".to_string(),
        status: "running".to_string(),
        priority: "high".to_string(),
        id: "haiku-1".to_string(),
        subsystem: None,
        file_scope: Vec::new(),
        blocked_by: Vec::new(),
        assigned_to: Some("worker-fox".to_string()),
    };

    app.handle_server_event(
        crate::protocol::ServerEvent::SwarmPlan {
            swarm_id: "test-swarm".to_string(),
            version: 3,
            items: vec![item.clone()],
            participants: vec!["session_a".to_string()],
            reason: None,
            summary: None,
        },
        &mut remote,
    );

    let graph_msg = app
        .display_messages()
        .iter()
        .find(|m| m.role == "swarm" && m.title.as_deref() == Some("Plan graph · v3"))
        .expect("SwarmPlan event should push an inline plan graph chat message");
    assert!(
        graph_msg.content.starts_with("```mermaid\nflowchart TD"),
        "plan graph message should carry a mermaid fence: {}",
        &graph_msg.content[..graph_msg.content.len().min(80)]
    );
    assert!(
        graph_msg.content.contains("t_haiku_1") && graph_msg.content.contains("write a haiku"),
        "graph should include the task node: {}",
        graph_msg.content
    );

    // A follow-up plan version updates the trailing graph message in place
    // instead of stacking a second diagram.
    let count_before = app.display_messages().len();
    let mut updated = item;
    updated.status = "completed".to_string();
    app.handle_server_event(
        crate::protocol::ServerEvent::SwarmPlan {
            swarm_id: "test-swarm".to_string(),
            version: 4,
            items: vec![updated],
            participants: vec!["session_a".to_string()],
            reason: None,
            summary: None,
        },
        &mut remote,
    );
    assert_eq!(
        app.display_messages().len(),
        count_before,
        "rapid plan updates must coalesce into the trailing plan graph message"
    );
    let graph_count = app
        .display_messages()
        .iter()
        .filter(|m| {
            m.role == "swarm"
                && m.title
                    .as_deref()
                    .is_some_and(|t| t.starts_with("Plan graph · "))
        })
        .count();
    assert_eq!(graph_count, 1, "only one trailing plan graph message expected");
    let latest = app
        .display_messages()
        .iter()
        .find(|m| m.title.as_deref() == Some("Plan graph · v4"))
        .expect("trailing graph message should carry the new version");
    assert!(latest.content.contains(":::done"), "updated status should recolor the node");
}

#[test]
fn test_plan_scope_notification_stays_off_the_transcript() {
    let mut app = create_test_app();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();
    let mut remote = crate::tui::backend::RemoteConnection::dummy();
    remote.mark_history_loaded();

    let count_before = app.display_messages().len();
    app.handle_server_event(
        crate::protocol::ServerEvent::Notification {
            from_session: "session_dove_123".to_string(),
            from_name: Some("dove".to_string()),
            notification_type: crate::protocol::NotificationType::Message {
                scope: Some("plan".to_string()),
                channel: None,
                tldr: None,
            },
            message: "Plan updated: task 'fix-debug-tests' assigned to session_blowfish_9."
                .to_string(),
        },
        &mut remote,
    );

    assert_eq!(
        app.display_messages().len(),
        count_before,
        "plan-scope churn must not add chat messages"
    );

    // Non-plan swarm notifications still land in the transcript.
    app.handle_server_event(
        crate::protocol::ServerEvent::Notification {
            from_session: "session_dove_123".to_string(),
            from_name: Some("dove".to_string()),
            notification_type: crate::protocol::NotificationType::Message {
                scope: Some("dm".to_string()),
                channel: None,
                tldr: None,
            },
            message: "DM from dove: hello".to_string(),
        },
        &mut remote,
    );
    assert_eq!(
        app.display_messages().len(),
        count_before + 1,
        "dm notifications keep their chat card"
    );
}
