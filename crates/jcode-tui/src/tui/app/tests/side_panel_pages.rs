// Side-panel page list (`Alt+P`) coverage.
//
// `include!`d into `crate::tui::app::tests`, so `create_test_app` and the other
// shared helpers are already in scope here.

fn picker_test_page(id: &str, title: &str) -> crate::side_panel::SidePanelPage {
    crate::side_panel::SidePanelPage {
        id: id.to_string(),
        title: title.to_string(),
        content: format!("# {title}"),
        updated_at_ms: 1,
        ..Default::default()
    }
}

fn picker_test_snapshot() -> crate::side_panel::SidePanelSnapshot {
    crate::side_panel::SidePanelSnapshot {
        focused_page_id: Some("plan".to_string()),
        pages: vec![
            picker_test_page("plan", "Plan"),
            picker_test_page("progress", "Progress"),
            picker_test_page("split_view", "Split View"),
        ],
    }
}

#[test]
fn test_alt_p_opens_page_list_and_enter_focuses_selected_page() {
    let mut app = create_test_app();
    app.side_panel = picker_test_snapshot();
    app.last_side_panel_focus_id = Some("plan".to_string());

    // Alt+P opens the list without disturbing the current page.
    app.handle_key(KeyCode::Char('p'), KeyModifiers::ALT)
        .unwrap();
    assert!(app.side_panel_page_picker.is_some());
    assert_eq!(app.side_panel.focused_page_id.as_deref(), Some("plan"));

    // Typing filters the list; Enter opens the remaining match.
    app.handle_key(KeyCode::Char('s'), KeyModifiers::NONE)
        .unwrap();
    app.handle_key(KeyCode::Enter, KeyModifiers::NONE).unwrap();
    assert!(app.side_panel_page_picker.is_none());
    assert_eq!(
        app.side_panel.focused_page_id.as_deref(),
        Some("split_view")
    );
    assert!(
        app.diff_pane_focus,
        "opening a page from the list lands with the pane focused"
    );
}

#[test]
fn test_alt_p_esc_closes_without_changing_the_page() {
    let mut app = create_test_app();
    app.side_panel = picker_test_snapshot();

    app.handle_key(KeyCode::Char('p'), KeyModifiers::ALT)
        .unwrap();
    app.handle_key(KeyCode::Char('x'), KeyModifiers::NONE)
        .unwrap();
    app.handle_key(KeyCode::Esc, KeyModifiers::NONE).unwrap();

    assert!(app.side_panel_page_picker.is_none());
    assert_eq!(app.side_panel.focused_page_id.as_deref(), Some("plan"));
}

#[test]
fn test_alt_p_on_an_empty_panel_reports_and_does_not_open() {
    let mut app = create_test_app();
    app.side_panel = crate::side_panel::SidePanelSnapshot::default();

    app.handle_key(KeyCode::Char('p'), KeyModifiers::ALT)
        .unwrap();

    assert!(app.side_panel_page_picker.is_none());
    assert_eq!(
        app.status_notice(),
        Some("Side panel: no pages".to_string())
    );
}
