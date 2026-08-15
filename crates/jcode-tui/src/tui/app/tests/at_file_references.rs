// Tests for `@` workspace-file references and multiline-paste temp files.

use std::path::PathBuf;

fn temp_workspace(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("jcode-at-file-e2e-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("README.md"), "# readme\n").unwrap();
    std::fs::write(dir.join("src/main.rs"), "fn main() {}\n").unwrap();
    std::fs::create_dir_all(dir.join("target")).unwrap();
    std::fs::write(dir.join("target/out.bin"), "junk").unwrap();
    dir
}

#[test]
fn at_reference_picker_filters_and_inserts_path() {
    let root = temp_workspace("picker");
    let mut app = create_test_app();
    app.session.working_dir = Some(root.to_string_lossy().to_string());

    // Typing `@` at a word boundary opens the picker and inserts `@`.
    assert!(super::input::handle_text_input(&mut app, "@"));
    assert!(app.file_pick.is_some(), "picker should open on word-boundary @");
    assert_eq!(app.input, "@");
    assert!(!app.file_pick.as_ref().unwrap().entries.is_empty());

    // Typing a filter narrows the list via fuzzy matching.
    assert!(super::at_file::handle_file_pick_key(
        &mut app,
        KeyCode::Char('m'),
        KeyModifiers::NONE,
    ));
    let state = app.file_pick.as_ref().unwrap();
    assert_eq!(state.filter, "m");
    assert!(state
        .filtered
        .iter()
        .any(|&i| state.entries[i].contains("main") || state.entries[i].contains("README")));

    // Enter accepts the top match and closes the picker.
    assert!(super::at_file::handle_file_pick_key(
        &mut app,
        KeyCode::Enter,
        KeyModifiers::NONE,
    ));
    assert!(app.file_pick.is_none(), "picker closes after accept");
    assert!(app.input.starts_with('@'));
    assert!(app.input.len() > 1, "path inserted after @");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn at_reference_not_triggered_mid_word() {
    let root = temp_workspace("midword");
    let mut app = create_test_app();
    app.session.working_dir = Some(root.to_string_lossy().to_string());

    // Email-ish context: typing @ after a word char must not open the picker.
    app.input = "foo".to_string();
    app.cursor_pos = 3;
    super::input::handle_text_input(&mut app, "@");
    assert!(app.file_pick.is_none(), "mid-word @ must not open the picker");
    assert_eq!(app.input, "foo@");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn at_reference_triggers_in_remote_session() {
    // A TUI attached to the shared server is a remote session, but the
    // workspace root is the server-side working directory, which is local when
    // the server runs on this machine. The picker must open there too; only a
    // genuinely unreachable root keeps `@` as plain text.
    let root = temp_workspace("remote");
    let mut app = create_test_app();
    app.session.working_dir = Some(root.to_string_lossy().to_string());
    app.is_remote = true;

    app.input = String::new();
    app.cursor_pos = 0;
    super::input::handle_text_input(&mut app, "@");
    assert!(app.file_pick.is_some(), "remote @ must open the picker");
    assert_eq!(app.input, "@");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn multiline_paste_uses_temp_file_marker_and_expands_on_submit() {
    let root = temp_workspace("paste");
    let mut app = create_test_app();
    app.session.working_dir = Some(root.to_string_lossy().to_string());

    let text = "line1\nline2\nline3\n";
    super::input::handle_text_paste(&mut app, text.to_string());
    assert_eq!(app.paste_files.len(), 1, "multiline paste goes to a temp file");
    assert!(
        app.input.contains(super::at_file::PASTE_MARKER_PREFIX),
        "input shows the simplified marker, got: {}",
        app.input
    );
    assert!(
        app.input.len() < text.len(),
        "marker must be shorter than the raw text"
    );
    let tmp_path = app.paste_files[0].path.clone();
    assert!(tmp_path.is_file(), "temp file exists on disk");

    // Submitting expands the marker to the original content, newline preserved.
    let input_snapshot = app.input.clone();
    let expanded = super::input::expand_paste_placeholders(&mut app, &input_snapshot);
    assert_eq!(expanded, text, "marker expands to original text");

    // take_prepared_input consumes and cleans up the temp file.
    app.input = app.input.clone();
    let prepared = super::input::take_prepared_input(&mut app);
    assert!(prepared.expanded.contains("line2"));
    assert!(!tmp_path.exists(), "temp file removed after submit");
    assert!(app.paste_files.is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn single_line_paste_stays_inline() {
    let mut app = create_test_app();
    super::input::handle_text_paste(&mut app, "one line".to_string());
    assert!(app.paste_files.is_empty());
    assert_eq!(app.input, "one line");
}

#[test]
fn paste_escape_cancel_removes_opening_at_and_noop_elsewhere() {
    let root = temp_workspace("esc");
    let mut app = create_test_app();
    app.session.working_dir = Some(root.to_string_lossy().to_string());

    assert!(super::input::handle_text_input(&mut app, "@"));
    assert!(app.file_pick.is_some());
    assert!(super::at_file::handle_file_pick_key(
        &mut app,
        KeyCode::Esc,
        KeyModifiers::NONE,
    ));
    assert!(app.file_pick.is_none());
    assert_eq!(app.input, "", "Esc undoes the auto-inserted @");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn at_reference_expands_into_submitted_message() {
    let root = temp_workspace("expand");
    let mut app = create_test_app();
    app.session.working_dir = Some(root.to_string_lossy().to_string());

    app.set_input_for_test("@README.md");
    app.submit_input();

    // Display keeps the reference as typed; the model receives the reference
    // preserved plus a lightweight list guiding it to `read` the files.
    assert_eq!(app.display_messages().len(), 1);
    assert_eq!(app.display_messages()[0].content, "@README.md");
    let provider_messages = app.materialized_provider_messages();
    let user_message = provider_messages
        .iter()
        .rev()
        .find(|message| message.role == crate::message::Role::User)
        .expect("expected submitted user message");
    match &user_message.content[0] {
        crate::message::ContentBlock::Text { text, .. } => {
            assert!(
                text.contains("@README.md"),
                "reference preserved as typed, got: {text}"
            );
            assert!(
                text.contains("README.md"),
                "reference list mentions the path, got: {text}"
            );
            assert!(
                !text.contains("# readme"),
                "file content must NOT be inlined, got: {text}"
            );
        }
        _ => panic!("Expected Text content block"),
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn at_reference_missing_file_stays_as_typed_without_reference_list() {
    let root = temp_workspace("missing");
    let mut app = create_test_app();
    app.session.working_dir = Some(root.to_string_lossy().to_string());

    app.set_input_for_test("@nope.txt");
    app.submit_input();

    let provider_messages = app.materialized_provider_messages();
    let user_message = provider_messages
        .iter()
        .rev()
        .find(|message| message.role == crate::message::Role::User)
        .expect("expected submitted user message");
    match &user_message.content[0] {
        crate::message::ContentBlock::Text { text, .. } => {
            assert_eq!(text, "@nope.txt", "missing file keeps the typed reference");
        }
        _ => panic!("Expected Text content block"),
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn reproduction_local_key_dispatch_opens_picker() {
    let root = temp_workspace("repro-local");
    let mut app = create_test_app();
    app.session.working_dir = Some(root.to_string_lossy().to_string());
    app.handle_key(KeyCode::Char('@'), KeyModifiers::NONE).unwrap();
    assert!(
        app.file_pick.is_some(),
        "local handle_key(Char('@')) must open the picker"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn reproduction_remote_key_dispatch_opens_picker() {
    use crossterm::event::{KeyEvent, KeyEventKind};
    let root = temp_workspace("repro-remote");
    let mut app = create_test_app();
    app.is_remote = true;
    app.session.working_dir = Some(root.to_string_lossy().to_string());
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();
    let mut remote = crate::tui::backend::RemoteConnection::dummy();
    rt.block_on(remote::handle_remote_key_event(
        &mut app,
        KeyEvent::new_with_kind(KeyCode::Char('@'), KeyModifiers::NONE, KeyEventKind::Press),
        &mut remote,
    ))
    .unwrap();
    assert!(
        app.file_pick.is_some(),
        "remote handle_remote_key_event(Char('@')) must open the picker"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn reproduction_altgr_at_still_opens_picker() {
    let root = temp_workspace("repro-altgr");
    let mut app = create_test_app();
    app.session.working_dir = Some(root.to_string_lossy().to_string());
    app.handle_key(KeyCode::Char('@'), KeyModifiers::CONTROL | KeyModifiers::ALT)
        .unwrap();
    assert!(
        app.file_pick.is_some(),
        "Ctrl+Alt @ (AltGr) must open the picker"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn at_triggers_after_cjk_character_mid_sentence() {
    let root = temp_workspace("cjk");
    let mut app = create_test_app();
    app.session.working_dir = Some(root.to_string_lossy().to_string());

    app.input = "请参考".to_string();
    app.cursor_pos = app.input.len();
    super::input::handle_text_input(&mut app, "@");
    assert!(app.file_pick.is_some(), "CJK-preceding @ must open the picker");
    assert_eq!(app.input, "请参考@");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn at_tab_accepts_selected_path() {
    let root = temp_workspace("tab");
    let mut app = create_test_app();
    app.session.working_dir = Some(root.to_string_lossy().to_string());

    super::input::handle_text_input(&mut app, "@");
    assert!(app.file_pick.is_some());
    assert!(super::at_file::handle_file_pick_key(
        &mut app,
        KeyCode::Tab,
        KeyModifiers::NONE,
    ));
    assert!(app.file_pick.is_none(), "Tab closes the picker");
    assert!(app.input.starts_with('@'));
    assert!(app.input.len() > 1, "Tab inserts the selected path");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn at_ctrl_np_navigation_moves_selection() {
    let root = temp_workspace("nav");
    let mut app = create_test_app();
    app.session.working_dir = Some(root.to_string_lossy().to_string());

    super::input::handle_text_input(&mut app, "@");
    assert!(app.file_pick.is_some());
    let total = app.file_pick.as_ref().unwrap().filtered.len();
    assert!(total >= 2, "workspace should have >= 2 indexable files");

    assert!(super::at_file::handle_file_pick_key(
        &mut app,
        KeyCode::Char('n'),
        KeyModifiers::CONTROL,
    ));
    assert_eq!(app.file_pick.as_ref().unwrap().selected, 1, "Ctrl+N moves down");
    assert!(super::at_file::handle_file_pick_key(
        &mut app,
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    ));
    assert_eq!(app.file_pick.as_ref().unwrap().selected, 0, "Ctrl+P moves up");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn at_no_match_char_dismisses_picker_and_keeps_at_in_draft() {
    let root = temp_workspace("dismiss");
    let mut app = create_test_app();
    app.session.working_dir = Some(root.to_string_lossy().to_string());

    super::input::handle_text_input(&mut app, "@");
    assert!(app.file_pick.is_some());
    assert_eq!(app.input, "@");

    // The zero-match char falls through the full local dispatch and lands in
    // the draft after the opening `@`.
    app.handle_key(KeyCode::Char('文'), KeyModifiers::NONE)
        .unwrap();
    assert!(app.file_pick.is_none(), "picker closes when the filter empties");
    assert_eq!(app.input, "@文", "draft keeps @ and receives the char");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn at_picker_preview_reads_selected_file_content() {
    let root = temp_workspace("preview");
    let mut app = create_test_app();
    app.session.working_dir = Some(root.to_string_lossy().to_string());

    super::input::handle_text_input(&mut app, "@");
    assert!(app.file_pick.is_some());

    let view = super::at_file::file_pick_view(&app).unwrap();
    assert!(view.preview.is_some(), "selected entry carries a preview");
    assert!(
        view.preview.as_ref().unwrap().contains("readme"),
        "preview shows the selected file content, got {:?}",
        view.preview
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn file_pick_open_ctrl_chord_falls_through_to_draft_editing() {
    let root = temp_workspace("fallthrough");
    let mut app = create_test_app();
    app.session.working_dir = Some(root.to_string_lossy().to_string());

    super::input::handle_text_input(&mut app, "@");
    assert!(app.file_pick.is_some());
    assert_eq!(app.input, "@");

    // Ctrl+U is not a picker key; the modal layer must report not-consumed so
    // the caller routes it to the normal handler (clear-to-line-start) instead
    // of swallowing it or panicking.
    let consumed = super::input::handle_modal_key(
        &mut app,
        KeyCode::Char('u'),
        KeyModifiers::CONTROL,
    )
    .unwrap();
    assert!(!consumed, "Ctrl+U must fall through to the normal handler");
    assert!(app.file_pick.is_some(), "picker stays open on fall-through keys");

    // Re-open, then verify an unhandled editing key (Delete) also falls through.
    super::input::handle_text_input(&mut app, "@");
    assert!(app.file_pick.is_some());
    let consumed = super::input::handle_modal_key(
        &mut app,
        KeyCode::Delete,
        KeyModifiers::NONE,
    )
    .unwrap();
    assert!(!consumed, "Delete must fall through to the normal handler");
    assert!(app.file_pick.is_some(), "picker stays open on fall-through keys");
    let _ = std::fs::remove_dir_all(&root);
}
