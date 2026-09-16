// Ctrl+X removal flow boundary (#12): a *held* Ctrl+X (key auto-repeat) must
// never arm or confirm a removal. The contract is two deliberate presses;
// terminals with the kitty keyboard protocol report a held key as
// `KeyEventKind::Repeat`, which both the local and the remote/client key paths
// must drop before the session picker overlay sees it.
//
// Note: this file is `include!`d into the shared tests module alongside other
// test files that already import KeyCode/KeyModifiers, so crossterm types are
// spelled out in full here instead of being imported again.

fn picker_with_session(
    session_id: &str,
    title: &str,
) -> crate::tui::session_picker::SessionPicker {
    crate::tui::session_picker::SessionPicker::new(vec![
        crate::tui::session_picker::SessionInfo {
            id: session_id.to_string(),
            parent_id: None,
            short_name: "remove".to_string(),
            icon: "t".to_string(),
            title: title.to_string(),
            message_count: 1,
            user_message_count: 1,
            assistant_message_count: 0,
            created_at: chrono::Utc::now(),
            last_message_time: chrono::Utc::now(),
            last_active_at: None,
            working_dir: None,
            model: None,
            provider_key: None,
            is_canary: false,
            is_debug: false,
            saved: false,
            save_label: None,
            status: crate::session::SessionStatus::Closed,
            needs_catchup: false,
            estimated_tokens: 0,
            first_user_prompt: None,
            messages_preview: Vec::new(),
            search_index: session_id.to_string(),
            server_name: None,
            server_icon: None,
            source: crate::tui::session_picker::SessionSource::Jcode,
            resume_target: crate::tui::session_picker::ResumeTarget::JcodeSession {
                session_id: session_id.to_string(),
            },
            external_path: None,
        },
    ])
}

fn ctrl_x(kind: crossterm::event::KeyEventKind) -> crossterm::event::KeyEvent {
    crossterm::event::KeyEvent::new_with_kind(
        crossterm::event::KeyCode::Char('x'),
        crossterm::event::KeyModifiers::CONTROL,
        kind,
    )
}

fn remove_armed(app: &crate::tui::app::App) -> bool {
    app.session_picker_overlay
        .as_ref()
        .is_some_and(|picker| picker.borrow().remove_arm_active())
}

#[test]
fn held_ctrl_x_never_arms_or_confirms_in_the_local_path() {
    let mut app = create_test_app();
    app.session_picker_overlay = Some(std::cell::RefCell::new(picker_with_session("session_remove", "Remove me")));

    // A held (repeat) Ctrl+X must not arm...
    app.handle_key_event(ctrl_x(crossterm::event::KeyEventKind::Repeat));
    assert!(!remove_armed(&app), "a held Ctrl+X must not arm a removal");

    // ...a deliberate press does...
    app.handle_key_event(ctrl_x(crossterm::event::KeyEventKind::Press));
    assert!(remove_armed(&app), "the deliberate press arms the removal");

    // ...and a held key must not supply the confirming second press.
    app.handle_key_event(ctrl_x(crossterm::event::KeyEventKind::Repeat));
    assert!(
        remove_armed(&app),
        "a held Ctrl+X must not confirm (the row stays armed)"
    );
    assert!(
        app.pending_session_removal.is_none(),
        "no removal may be queued by key auto-repeat"
    );

    // The deliberate second press still confirms.
    app.handle_key_event(ctrl_x(crossterm::event::KeyEventKind::Press));
    assert!(!remove_armed(&app), "confirmation disarms");
    assert!(
        app.pending_session_removal.is_some(),
        "the deliberate second press queues the removal"
    );
}

#[test]
fn held_ctrl_x_never_arms_in_the_remote_path() {
    let mut app = create_test_app();
    app.session_picker_overlay = Some(std::cell::RefCell::new(picker_with_session("session_remove", "Remove me")));
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();
    let mut remote = crate::tui::backend::RemoteConnection::dummy();

    rt.block_on(crate::tui::app::remote::handle_remote_key_event(
        &mut app,
        ctrl_x(crossterm::event::KeyEventKind::Repeat),
        &mut remote,
    ))
    .unwrap();
    assert!(
        !remove_armed(&app),
        "held Ctrl+X must not arm in remote/client mode either"
    );

    rt.block_on(crate::tui::app::remote::handle_remote_key_event(
        &mut app,
        ctrl_x(crossterm::event::KeyEventKind::Press),
        &mut remote,
    ))
    .unwrap();
    assert!(
        remove_armed(&app),
        "a deliberate press still arms in remote/client mode"
    );
}

#[test]
fn held_ctrl_x_outside_the_picker_is_left_alone() {
    let mut app = create_test_app();
    // No picker overlay: the guard must not swallow the input box's own
    // Ctrl+X handling (cut line) when the removal flow is not on screen.
    app.handle_key_event(ctrl_x(crossterm::event::KeyEventKind::Repeat));
    assert!(app.session_picker_overlay.is_none());
}
