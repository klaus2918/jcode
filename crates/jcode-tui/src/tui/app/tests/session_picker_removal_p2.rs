// P2 removal verification (#13): the finished × removal race, failure
// injection for the stop request, and the hidden-view restore path. Together
// with the #9/#12 state-machine unit tests this pins the removal flow end to
// end where automation is possible; the interactive TUI checklist lives in
// verify-log #13.
//
// Note: this file is `include!`d into the shared tests module, so crossterm,
// storage and protocol types are spelled out in full (module-level imports
// from sibling test files are not relied on).

fn removal_presence(session_id: &str) -> crate::session::SessionPresence {
    crate::session::SessionPresence {
        session_id: session_id.to_string(),
        pid: std::process::id(),
        streaming: false,
        streaming_since: None,
        background: false,
        background_since: None,
        internal: false,
    }
}

/// Arm + confirm the Ctrl+X removal on the currently highlighted row.
fn confirm_remove_twice(app: &mut crate::tui::app::App) {
    app.handle_session_picker_key(
        crossterm::event::KeyCode::Char('x'),
        crossterm::event::KeyModifiers::CONTROL,
    )
    .unwrap();
    app.handle_session_picker_key(
        crossterm::event::KeyCode::Char('x'),
        crossterm::event::KeyModifiers::CONTROL,
    )
    .unwrap();
}

#[test]
fn removal_race_live_then_finished_routes_each_way() {
    let id = "session_ctrlx_race_finished";
    crate::storage::unhide_session(id);
    let mut app = create_test_app();
    app.session_picker_overlay = Some(std::cell::RefCell::new(picker_with_session(id, "Race row")));

    // The row is still live (running in another process) when the user
    // confirms: the local tick must refuse it with a reason and keep the row.
    app.session_picker_overlay
        .as_ref()
        .unwrap()
        .borrow_mut()
        .set_live_presence_for_test(vec![removal_presence(id)]);
    confirm_remove_twice(&mut app);
    let _ = crate::tui::app::local::handle_tick(&mut app);
    let notice = app.status_notice().unwrap_or_default();
    assert!(
        notice.contains("runs in another process"),
        "live rows must be refused with a reason, got: {notice:?}"
    );
    assert!(
        !crate::storage::session_is_hidden(id),
        "a refused removal must not hide the row"
    );

    // The process finishes before the retry: the same gesture now hides it.
    app.session_picker_overlay
        .as_ref()
        .unwrap()
        .borrow_mut()
        .set_live_presence_for_test(vec![]);
    confirm_remove_twice(&mut app);
    let _ = crate::tui::app::local::handle_tick(&mut app);
    let notice = app.status_notice().unwrap_or_default();
    assert!(
        notice.contains("removed from the board"),
        "finished rows hide on the local tick, got: {notice:?}"
    );
    assert!(crate::storage::session_is_hidden(id));
    crate::storage::unhide_session(id);
}

#[test]
fn close_failure_keeps_the_row_and_reports_the_reason() {
    let id = "session_ctrlx_close_fail";
    let mut app = create_test_app();
    app.session_picker_overlay =
        Some(std::cell::RefCell::new(picker_with_session(id, "Close fail row")));

    // The remote tick sent the close request and is waiting for its reply.
    app.track_session_close(
        4242,
        crate::tui::app::PendingSessionRemoval {
            session_id: id.to_string(),
            display_name: "Close fail row".to_string(),
            live: true,
        },
    );

    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();
    let mut remote = crate::tui::backend::RemoteConnection::dummy();
    let handled = app.handle_server_event(
        crate::protocol::ServerEvent::Error {
            id: 4242,
            message: "'Close fail row' is still finishing its turn; try again in a moment."
                .to_string(),
            retry_after_secs: Some(2),
        },
        &mut remote,
    );

    assert!(handled, "the reply must be consumed by the close request");
    let notice = app.status_notice().unwrap_or_default();
    assert!(
        notice.starts_with("✗") && notice.contains("still finishing"),
        "the failure reason must reach the status bar, got: {notice:?}"
    );
    assert!(
        !crate::storage::session_is_hidden(id),
        "a failed close keeps the row for a retry"
    );
}

#[test]
fn removed_sessions_never_come_back_and_have_no_recovery_view() {
    let id = "session_ctrlx_removed";
    crate::storage::unhide_session(id);
    let mut app = create_test_app();
    app.session_picker_overlay =
        Some(std::cell::RefCell::new(picker_with_session(id, "Removed row")));

    // Ctrl+X twice removes the finished row for good.
    confirm_remove_twice(&mut app);
    let _ = crate::tui::app::local::handle_tick(&mut app);
    assert!(
        crate::storage::session_is_hidden(id),
        "removal marks the row removed"
    );
    assert!(
        app.status_notice()
            .unwrap_or_default()
            .contains("removed from the board"),
        "the removal is confirmed in the status bar"
    );

    // Removal is permanent: no filter step ever surfaces the row again, and no
    // second gesture restores it either (there is no recovery view).
    for _ in 0..12 {
        app.handle_session_picker_key(
            crossterm::event::KeyCode::Char('s'),
            crossterm::event::KeyModifiers::empty(),
        )
        .unwrap();
        let selectable_again = app
            .session_picker_overlay
            .as_ref()
            .unwrap()
            .borrow()
            .selected_session()
            .is_some_and(|session| session.id == id);
        assert!(
            !selectable_again,
            "the removed row must never become selectable again"
        );
    }
    confirm_remove_twice(&mut app);
    assert!(
        crate::storage::session_is_hidden(id),
        "no restore path exists after removal"
    );
    crate::storage::unhide_session(id);
}

#[test]
fn live_removal_closes_the_owner_or_refuses_safely() {
    // (a) No owning process any more (the session finished while the
    // confirmation was in flight): the stale row is removed instead of
    // reporting a bogus failure.
    let stale_id = "session_ctrlx_stale_owner";
    crate::storage::unhide_session(stale_id);
    let mut app = create_test_app();
    app.session_picker_overlay =
        Some(std::cell::RefCell::new(picker_with_session(stale_id, "Stale row")));
    app.session_picker_overlay
        .as_ref()
        .unwrap()
        .borrow_mut()
        .set_live_presence_for_test(vec![removal_presence(stale_id)]);
    confirm_remove_twice(&mut app);
    let _ = crate::tui::app::local::handle_tick(&mut app);
    assert!(
        crate::storage::session_is_hidden(stale_id),
        "a live row whose owning process is gone is still removed"
    );
    crate::storage::unhide_session(stale_id);

    // (b) The registry names this very process (mis-registration or a stale
    // pid): the removal must refuse rather than terminate the running jcode.
    let self_id = "session_ctrlx_self_owner";
    crate::storage::unhide_session(self_id);
    crate::storage::register_active_pid(self_id, std::process::id());
    let mut app = create_test_app();
    app.session_picker_overlay =
        Some(std::cell::RefCell::new(picker_with_session(self_id, "Self row")));
    app.session_picker_overlay
        .as_ref()
        .unwrap()
        .borrow_mut()
        .set_live_presence_for_test(vec![removal_presence(self_id)]);
    confirm_remove_twice(&mut app);
    let _ = crate::tui::app::local::handle_tick(&mut app);
    let notice = app.status_notice().unwrap_or_default();
    assert!(
        notice.contains("cannot close itself"),
        "the guard must refuse to close its own process, got: {notice:?}"
    );
    assert!(
        !crate::storage::session_is_hidden(self_id),
        "a refused close keeps the row for a retry"
    );
    crate::storage::unregister_active_pid(self_id);
}
