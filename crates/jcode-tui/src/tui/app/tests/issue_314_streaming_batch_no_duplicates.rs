// Issue #314: live transcript duplicates assistant commentary and tool calls.
//
// Server conversation history is the source of truth (one copy per message);
// the client's `display_messages` is what actually renders live. The repro
// harness (scripts/repro_live_duplicate.py) detects the divergence by seeding
// unique MARK_xxxx tokens into a session and counting how many display
// messages contain each one. These tests lock that invariant at the client
// append layer: one streaming turn batch (assistant text commit + tool call +
// in-place tool result update — the exact shape of the ToolResult handler in
// turn.rs that bumps `display_messages_version`) must leave exactly one
// display message per logical message. A future regression that appends a
// second copy of any of these (or replaces instead of updating the tool
// message) fails the marker-count assertion.

fn marker_presence_count(msgs: &[DisplayMessage], marker: &str) -> usize {
    msgs.iter()
        .filter(|msg| {
            let mut blob = msg.content.clone();
            if let Some(title) = &msg.title {
                blob.push_str(title);
            }
            if let Some(tool) = &msg.tool_data {
                blob.push_str(&tool.id);
                blob.push_str(&format!("{:?}", tool.input));
            }
            blob.contains(marker)
        })
        .count()
}

fn assert_each_marker_once(app: &App, markers: &[&str]) {
    for marker in markers {
        assert_eq!(
            marker_presence_count(app.display_messages(), marker),
            1,
            "marker {marker} must appear in exactly one display message (issue #314 duplicate check)"
        );
    }
}

fn bash_tool_call(id: &str, in_marker: &str) -> crate::message::ToolCall {
    crate::message::ToolCall {
        id: id.to_string(),
        name: "bash".to_string(),
        input: serde_json::json!({
            "command": format!("echo {in_marker}"),
            "intent": "repro step",
        }),
        intent: Some("repro step".to_string()),
        thought_signature: None,
    }
}

#[test]
fn streaming_turn_batch_appends_each_message_exactly_once() {
    let mut app = create_test_app();

    app.push_display_message(DisplayMessage::user(
        "MARK_USER_OPEN please run the repro tasks",
    ));

    // Assistant commentary streams in and is committed to the transcript.
    app.append_streaming_text("MARK_ASSIST_00 I'll inspect step 0.");
    assert!(app.commit_pending_streaming_assistant_message());

    // The tool call renders as its own display message; `tool_data.id` is how
    // the ToolResult handler (turn.rs) locates it for the in-place update.
    app.push_display_message(DisplayMessage::tool(
        "running…".to_string(),
        bash_tool_call("toolu_314_a", "MARK_TOOLIN_00"),
    ));

    // ToolResult arrives: content is updated in place (mirrors turn.rs's
    // `display_messages.iter_mut().rev().find(...)` + bump), never appended.
    assert!(app.replace_latest_tool_display_message(
        "toolu_314_a",
        None,
        "MARK_TOOLOUT_00 step 0 ok".to_string(),
    ));

    // A second assistant segment commits on top.
    app.append_streaming_text("MARK_ASSIST_FINAL all steps complete");
    assert!(app.commit_pending_streaming_assistant_message());

    assert_each_marker_once(
        &app,
        &[
            "MARK_USER_OPEN",
            "MARK_ASSIST_00",
            "MARK_TOOLIN_00",
            "MARK_TOOLOUT_00",
            "MARK_ASSIST_FINAL",
        ],
    );
    assert_eq!(app.display_messages().len(), 4);
}

#[test]
fn tool_result_in_place_update_bumps_display_messages_version() {
    let mut app = create_test_app();
    app.push_display_message(DisplayMessage::tool(
        "running…".to_string(),
        bash_tool_call("toolu_314_b", "MARK_TOOLIN_01"),
    ));
    let before = app.display_messages_version;

    assert!(app.replace_latest_tool_display_message(
        "toolu_314_b",
        None,
        "MARK_TOOLOUT_01 done".to_string(),
    ));
    assert_ne!(
        app.display_messages_version, before,
        "in-place tool result update must bump display_messages_version (turn.rs ToolResult handler)"
    );
    // The update is in place: still one tool message, no duplicate copy.
    assert_eq!(app.display_messages().len(), 1);
}

#[test]
fn history_restore_then_streaming_batch_keeps_each_message_once() {
    let mut app = create_test_app();

    // Fresh-resume path: the client renders server history into
    // display_messages (replace — one copy per stored message).
    app.replace_display_messages(vec![
        DisplayMessage::user("MARK_USER_OPEN please run the repro tasks"),
        DisplayMessage::assistant("MARK_ASSIST_00 I'll inspect step 0."),
        DisplayMessage::tool(
            "MARK_TOOLOUT_00 step 0 ok".to_string(),
            bash_tool_call("toolu_314_c", "MARK_TOOLIN_00"),
        ),
    ]);

    // A live turn streams on top of the restored history.
    app.append_streaming_text("MARK_ASSIST_01 I'll inspect step 1.");
    assert!(app.commit_pending_streaming_assistant_message());
    app.push_display_message(DisplayMessage::tool(
        "running…".to_string(),
        bash_tool_call("toolu_314_d", "MARK_TOOLIN_01"),
    ));
    assert!(app.replace_latest_tool_display_message(
        "toolu_314_d",
        None,
        "MARK_TOOLOUT_01 step 1 ok".to_string(),
    ));

    assert_each_marker_once(
        &app,
        &[
            "MARK_USER_OPEN",
            "MARK_ASSIST_00",
            "MARK_TOOLIN_00",
            "MARK_TOOLOUT_00",
            "MARK_ASSIST_01",
            "MARK_TOOLIN_01",
            "MARK_TOOLOUT_01",
        ],
    );
    assert_eq!(app.display_messages().len(), 5);
}
