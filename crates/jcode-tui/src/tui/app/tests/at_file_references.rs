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
fn at_reference_not_triggered_mid_word_or_remote() {
    let root = temp_workspace("midword");
    let mut app = create_test_app();
    app.session.working_dir = Some(root.to_string_lossy().to_string());

    // Email-ish context: typing @ after a word char must not open the picker.
    app.input = "foo".to_string();
    app.cursor_pos = 3;
    super::input::handle_text_input(&mut app, "@");
    assert!(app.file_pick.is_none(), "mid-word @ must not open the picker");
    assert_eq!(app.input, "foo@");

    // Remote mode never opens the picker (paths live on the server).
    app.is_remote = true;
    app.input = String::new();
    app.cursor_pos = 0;
    super::input::handle_text_input(&mut app, "@");
    assert!(app.file_pick.is_none(), "remote @ must not open the picker");
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

    // Display keeps the reference as typed; the model receives the file content.
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
            assert_eq!(text, "<@README.md>\n# readme");
        }
        _ => panic!("Expected Text content block"),
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn at_reference_missing_file_stays_as_typed() {
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
