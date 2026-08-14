//! `@` workspace-file references and multiline-paste temp files.
//!
//! Two features share one expansion path so pasted content and explicit file
//! references both flow into the message as file content on submit:
//!
//! 1. Typing `@` at a word boundary opens a workspace file picker with fuzzy
//!    filtering. Confirming inserts `@<relative-path>` into the composer, and
//!    on submit the reference is expanded to the file content.
//! 2. Pasting multi-line text (>= 2 lines) is written to a temp file and the
//!    composer gets a simplified `@[粘贴内容...]` marker instead of the raw
//!    text, so multiline pastes never fill the input box and never race a
//!    stray Enter. On submit the marker is replaced by the file content and
//!    the temp file is removed.

use std::path::{Path, PathBuf};

use super::App;
use super::input::insert_input_text;
use crate::tui::FilePickView;

/// Marker prefix for temp-file-backed pastes. The full marker is
/// `@[粘贴内容{N}]` with a monotonically increasing index so multiple pastes
/// in one draft can each be expanded exactly once.
pub(super) const PASTE_MARKER_PREFIX: &str = "@[粘贴内容";

/// A multiline paste stored in a temp file and referenced by a marker.
#[derive(Debug, Clone)]
pub(super) struct PasteFileRef {
    pub marker: String,
    pub path: PathBuf,
}

/// State for the `@` workspace-file picker overlay.
#[derive(Debug, Clone, Default)]
pub(super) struct FilePickState {
    /// Fuzzy filter typed after `@`.
    pub filter: String,
    /// All indexed workspace files (relative paths), in scan order.
    pub entries: Vec<String>,
    /// Indices into `entries` that match `filter`, in score order.
    pub filtered: Vec<usize>,
    /// Selected row in `filtered`.
    pub selected: usize,
    /// Cached preview text of the selected file (first lines), refreshed when
    /// the selection or filter changes. `None` when the file is unreadable,
    /// binary, or oversized.
    pub preview: Option<String>,
    /// Whether the opening `@` was inserted by this picker (so Esc can undo it).
    opened_at_end: bool,
}

/// Maximum size of a file whose content is expanded inline on submit. Larger
/// files stay as `@path` text so the model can read them with tools instead
/// of blowing up the context window.
const MAX_EXPAND_BYTES: u64 = 128 * 1024;
/// Maximum number of files in the workspace index, bounding scan cost.
const MAX_INDEX_FILES: usize = 3000;
/// Maximum number of preview lines and bytes shown in the picker preview pane.
const PREVIEW_MAX_LINES: usize = 15;
const PREVIEW_MAX_BYTES: u64 = 2000;
/// Directory names never indexed (build outputs, VCS, dependencies).
const SKIP_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    ".jcode",
    ".claude",
    ".codex",
    "target",
    "node_modules",
    "bower_components",
    ".venv",
    "venv",
    ".tox",
    ".cache",
    "__pycache__",
    "dist",
    "build",
    ".next",
    ".nuxt",
    "vendor",
    ".idea",
    ".vscode",
    "coverage",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
    ".terraform",
    "Pods",
    ".gradle",
];
/// Maximum directory depth scanned for `@` candidates.
const MAX_SCAN_DEPTH: usize = 10;

/// Root directory the `@` picker searches: the session working directory, or
/// the process CWD when the session has none.
pub(super) fn workspace_root(app: &App) -> Option<PathBuf> {
    app.session
        .working_dir
        .clone()
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .or_else(|| std::env::current_dir().ok().filter(|path| path.is_dir()))
}

/// Whether `@` at the cursor starts a reference. ASCII word/path characters
/// before it (`foo@bar` email, `a/b@c`) do not trigger, but Unicode text does:
/// CJK ideographs are `is_alphabetic`, so `请参考@` opens the picker while
/// `foo@bar` stays plain text.
fn at_starts_reference(input: &str, cursor_pos: usize) -> bool {
    if cursor_pos == 0 {
        return true;
    }
    let Some(prev) = input[..cursor_pos].chars().next_back() else {
        return true;
    };
    !(prev.is_ascii_alphanumeric() || matches!(prev, '_' | '-' | '/' | '.' | '\\'))
}

/// Open the `@` file picker after inserting the opening `@`.
pub(super) fn open_file_pick(app: &mut App) {
    let Some(root) = workspace_root(app) else {
        return;
    };
    let entries = match app.file_index.clone() {
        Some(cached) => cached,
        None => {
            let scanned = scan_workspace_files(&root);
            app.file_index = Some(scanned.clone());
            scanned
        }
    };
    let opened_at_end = app.cursor_pos == app.input.len();
    let mut state = FilePickState {
        filter: String::new(),
        filtered: (0..entries.len()).collect(),
        selected: 0,
        entries,
        preview: None,
        opened_at_end,
    };
    refresh_file_pick_preview(app, &mut state);
    app.file_pick = Some(state);
}

/// Try to trigger the picker for a freshly typed `@`. Returns true when the
/// picker opened (the caller should not also insert the character).
pub(super) fn try_trigger_file_pick(app: &mut App, text: &str) -> bool {
    if text != "@" {
        return false;
    }
    if !at_starts_reference(&app.input, app.cursor_pos) {
        return false;
    }
    app.remember_input_undo_state();
    insert_input_text(app, "@");
    open_file_pick(app);
    app.file_pick.as_ref().is_some()
}

/// Recompute `filtered` from the current filter, scoring with the prepared
/// token query. Basename matches rank above whole-path-only matches (fzf
/// style), with shorter paths winning ties. Bounded to the visible candidate
/// set for responsiveness.
fn refresh_file_pick_filter(state: &mut FilePickState) {
    if state.filter.trim().is_empty() {
        state.filtered = (0..state.entries.len()).collect();
    } else {
        let query = jcode_fuzzy::PreparedTokenQuery::new(&state.filter);
        let mut scored: Vec<(i32, usize)> = Vec::new();
        for (index, entry) in state.entries.iter().enumerate() {
            let Some(base) = query.score(entry) else {
                continue;
            };
            let basename = entry.rsplit('/').next().unwrap_or(entry);
            let score = if query.score(basename).is_some() {
                base.saturating_mul(2)
            } else {
                base
            };
            scored.push((score, index));
        }
        scored.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| state.entries[a.1].len().cmp(&state.entries[b.1].len()))
                .then_with(|| a.1.cmp(&b.1))
        });
        state.filtered = scored.into_iter().map(|(_, index)| index).collect();
    }
    if state.selected >= state.filtered.len() {
        state.selected = 0;
    }
}

/// Refresh the cached preview text for the currently selected candidate. The
/// preview is the file's first `PREVIEW_MAX_LINES` lines; binary, oversized,
/// or unreadable files get no preview.
fn refresh_file_pick_preview(app: &App, state: &mut FilePickState) {
    let Some(root) = workspace_root(app) else {
        state.preview = None;
        return;
    };
    let Some(&index) = state.filtered.get(state.selected) else {
        state.preview = None;
        return;
    };
    let Some(rel) = state.entries.get(index) else {
        state.preview = None;
        return;
    };
    let path = root.join(rel);
    let Ok(metadata) = std::fs::metadata(&path) else {
        state.preview = None;
        return;
    };
    if !metadata.is_file() || metadata.len() > PREVIEW_MAX_BYTES {
        state.preview = None;
        return;
    }
    let Ok(content) = std::fs::read(&path) else {
        state.preview = None;
        return;
    };
    if content.contains(&0) {
        state.preview = None;
        return;
    }
    let text = String::from_utf8_lossy(&content);
    let preview: Vec<&str> = text.lines().take(PREVIEW_MAX_LINES).collect();
    state.preview = Some(preview.join("\n"));
}

/// Handle keys while the `@` picker is active. Returns true when consumed.
///
/// Typing a character that narrows the list to zero matches is treated as the
/// reference ending: the picker closes (keeping the opening `@`) and the
/// character falls through to the normal draft handler, so Chinese text typed
/// right after `@` is never swallowed or lost.
pub(super) fn handle_file_pick_key(
    app: &mut App,
    code: crossterm::event::KeyCode,
    modifiers: crossterm::event::KeyModifiers,
) -> bool {
    use crossterm::event::{KeyCode, KeyModifiers};
    let Some(mut state) = app.file_pick.take() else {
        return false;
    };

    // fzf-style navigation chords, routed before the Ctrl fall-through.
    match (code, modifiers) {
        (KeyCode::Char('n'), KeyModifiers::CONTROL) => {
            if !state.filtered.is_empty() && state.selected + 1 < state.filtered.len() {
                state.selected += 1;
                refresh_file_pick_preview(app, &mut state);
            }
            app.file_pick = Some(state);
            return true;
        }
        (KeyCode::Char('p'), KeyModifiers::CONTROL) => {
            if !state.filtered.is_empty() {
                state.selected = state.selected.saturating_sub(1);
                refresh_file_pick_preview(app, &mut state);
            }
            app.file_pick = Some(state);
            return true;
        }
        _ => {}
    }

    if modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::SUPER) {
        app.file_pick = Some(state);
        return false;
    }

    match code {
        KeyCode::Char(ch) => {
            state.filter.push(ch);
            refresh_file_pick_filter(&mut state);
            if state.filtered.is_empty() {
                // Hybrid dismiss: the reference ended. Keep the opening `@`,
                // drop the picker, and let the caller insert this character
                // into the draft normally.
                return false;
            }
            refresh_file_pick_preview(app, &mut state);
        }
        KeyCode::Backspace => {
            if !state.filter.is_empty() {
                state.filter.pop();
                refresh_file_pick_filter(&mut state);
            }
            refresh_file_pick_preview(app, &mut state);
        }
        KeyCode::Up => {
            if !state.filtered.is_empty() {
                state.selected = state.selected.saturating_sub(1);
            }
            refresh_file_pick_preview(app, &mut state);
        }
        KeyCode::Down => {
            if !state.filtered.is_empty() && state.selected + 1 < state.filtered.len() {
                state.selected += 1;
            }
            refresh_file_pick_preview(app, &mut state);
        }
        KeyCode::Tab | KeyCode::Enter => {
            accept_file_pick_selection(app, state);
            return true;
        }
        KeyCode::Esc => {
            cancel_file_pick(app, state);
            return true;
        }
        _ => {
            app.file_pick = Some(state);
            return false;
        }
    }
    app.file_pick = Some(state);
    true
}

/// Insert the selected path after the opening `@` and close the picker.
fn accept_file_pick_selection(app: &mut App, state: FilePickState) {
    let Some(&index) = state.filtered.get(state.selected) else {
        return;
    };
    let Some(path) = state.entries.get(index) else {
        return;
    };
    insert_input_text(app, path);
}

/// Close the picker. If the opening `@` was auto-inserted at the end of the
/// draft and nothing else was typed around it, undo it so Esc leaves the
/// composer exactly as it was.
fn cancel_file_pick(app: &mut App, state: FilePickState) {
    if state.opened_at_end
        && app.cursor_pos == app.input.len()
        && app.input.ends_with('@')
        && state.filter.is_empty()
    {
        app.input.pop();
        app.cursor_pos = app.input.len();
    }
}

/// Snapshot for the `@` picker overlay.
pub(super) fn file_pick_view(app: &App) -> Option<FilePickView> {
    let state = app.file_pick.as_ref()?;
    const VISIBLE: usize = 8;
    let total = state.filtered.len();
    let selected = state.selected.min(total.saturating_sub(1));
    let window_start = selected.saturating_sub(VISIBLE.saturating_sub(1));
    let mut visible = Vec::new();
    for &index in state.filtered.iter().skip(window_start).take(VISIBLE) {
        if let Some(path) = state.entries.get(index) {
            visible.push(path.clone());
        }
    }
    let selected_in_window = selected.saturating_sub(window_start);
    Some(FilePickView {
        query: state.filter.clone(),
        matches: visible,
        selected: selected_in_window,
        total,
        preview: state.preview.clone(),
    })
}

/// Scan the workspace for indexable files, returning relative POSIX-style
/// paths. Bounded by `MAX_INDEX_FILES` and `MAX_SCAN_DEPTH`.
fn scan_workspace_files(root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    scan_dir(root, root, 0, &mut files);
    files.sort();
    files
}

fn scan_dir(root: &Path, dir: &Path, depth: usize, files: &mut Vec<String>) {
    if depth > MAX_SCAN_DEPTH || files.len() >= MAX_INDEX_FILES {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if files.len() >= MAX_INDEX_FILES {
            return;
        }
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') && SKIP_DIRS.contains(&name.as_ref()) {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            if !SKIP_DIRS.contains(&name.as_ref()) {
                scan_dir(root, &path, depth + 1, files);
            }
        } else if file_type.is_file()
            && let Ok(rel) = path.strip_prefix(root)
        {
            let rel = rel.to_string_lossy().replace('\\', "/");
            files.push(rel);
        }
    }
}

/// Write paste text to a temp file under `~/.jcode/tmp/`.
fn write_paste_temp_file(app: &mut App, text: &str) -> Option<PathBuf> {
    let jcode_dir = crate::storage::jcode_dir().ok()?;
    let tmp_dir = jcode_dir.join("tmp");
    std::fs::create_dir_all(&tmp_dir).ok()?;
    app.paste_file_seq += 1;
    let path = tmp_dir.join(format!(
        "paste-{}-{}.txt",
        std::process::id(),
        app.paste_file_seq
    ));
    std::fs::write(&path, text).ok()?;
    Some(path)
}

/// Handle a text paste. Multi-line text (>= 2 lines) is moved to a temp file
/// and referenced by a marker; single-line text is inserted directly.
pub(super) fn handle_paste_text(app: &mut App, text: &str) {
    if text.lines().count() >= 2
        && let Some(path) = write_paste_temp_file(app, text)
    {
        let marker = paste_marker(app.paste_file_seq);
        app.paste_files.push(PasteFileRef {
            marker: marker.clone(),
            path,
        });
        app.remember_input_undo_state();
        insert_input_text(app, &marker);
        return;
    }
    insert_input_text(app, text);
}

fn paste_marker(seq: u64) -> String {
    format!("{PASTE_MARKER_PREFIX}{seq}]")
}

/// Replace paste markers and `@file` references in a draft with file content.
pub(super) fn expand_placeholders(app: &App, input: &str) -> String {
    let mut result = expand_paste_markers(app, input);
    result = expand_at_references(app, &result);
    result
}

fn expand_paste_markers(app: &App, input: &str) -> String {
    let mut result = input.to_string();
    for paste in app.paste_files.iter().rev() {
        if let Some(pos) = result.rfind(&paste.marker)
            && let Ok(content) = std::fs::read_to_string(&paste.path)
        {
            result.replace_range(pos..pos + paste.marker.len(), &content);
        }
    }
    result
}

/// Expand `@<path>` references to file content. Paths resolve relative to the
/// workspace root (or absolutely when they start with a root/`~`). Missing or
/// oversized files stay as typed text so tools can still read them.
fn expand_at_references(app: &App, input: &str) -> String {
    let Some(root) = workspace_root(app) else {
        return input.to_string();
    };
    expand_at_references_in_root(&root, input)
}

fn expand_at_references_in_root(root: &Path, input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(at) = rest.find('@') {
        result.push_str(&rest[..at]);
        rest = &rest[at..];
        // A reference token runs until whitespace or a closing bracket.
        let token_end = rest[1..]
            .find(|ch: char| ch.is_whitespace() || ch == ']')
            .map(|end| end + 1)
            .unwrap_or(rest.len());
        let token = &rest[1..token_end];
        if token.is_empty() {
            result.push('@');
            rest = &rest[1..];
            continue;
        }
        let expanded = resolve_and_read_reference(root, token);
        match expanded {
            Some(content) => {
                result.push_str(&content);
            }
            None => {
                result.push('@');
                result.push_str(token);
            }
        }
        rest = &rest[token_end..];
    }
    result.push_str(rest);
    result
}

fn resolve_and_read_reference(root: &Path, token: &str) -> Option<String> {
    let path = resolve_reference_path(root, token)?;
    let metadata = std::fs::metadata(&path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_EXPAND_BYTES {
        return None;
    }
    let content = std::fs::read_to_string(&path).ok()?;
    Some(format!("<@{}>\n{}", token, content.trim_end()))
}

fn resolve_reference_path(root: &Path, token: &str) -> Option<PathBuf> {
    let candidate = if let Some(stripped) = token.strip_prefix("~/") {
        dirs_home()?.join(stripped)
    } else if token.starts_with('/') || token.contains(':') {
        PathBuf::from(token)
    } else {
        root.join(token)
    };
    // Keep references inside the workspace (or an explicit absolute/`~` path).
    Some(candidate)
}

#[cfg(not(test))]
fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
}

#[cfg(test)]
fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

/// Remove temp files that backed submitted pastes.
pub(super) fn cleanup_paste_files(app: &mut App) {
    for paste in std::mem::take(&mut app.paste_files) {
        let _ = std::fs::remove_file(&paste.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_root() -> PathBuf {
        std::env::temp_dir().join(format!("jcode-at-file-test-{}", std::process::id()))
    }

    fn prepare_tree(root: &Path) {
        let _ = std::fs::remove_dir_all(root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("target")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join("README.md"), "# hi\n").unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn x() {}\n").unwrap();
        std::fs::write(root.join("target/out.bin"), "junk").unwrap();
        std::fs::write(root.join(".git/config"), "[core]").unwrap();
    }

    #[test]
    fn scan_skips_vcs_and_build_dirs() {
        let root = test_root().join("scan");
        prepare_tree(&root);
        let files = scan_workspace_files(&root);
        assert!(files.contains(&"README.md".to_string()));
        assert!(files.contains(&"src/main.rs".to_string()));
        assert!(!files.contains(&"target/out.bin".to_string()));
        assert!(!files.contains(&".git/config".to_string()));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn at_starts_reference_boundaries() {
        assert!(at_starts_reference("", 0));
        assert!(at_starts_reference("hello ", 6));
        assert!(at_starts_reference("(", 1));
        assert!(!at_starts_reference("foo", 3));
        assert!(!at_starts_reference("a/b", 3));
    }

    #[test]
    fn paste_marker_format_is_compact() {
        assert_eq!(paste_marker(1), "@[粘贴内容1]");
        assert_eq!(paste_marker(12), "@[粘贴内容12]");
    }

    #[test]
    fn expand_at_references_reads_workspace_files() {
        let root = test_root().join("expand");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("note.txt"), "hello world").unwrap();
        std::fs::write(root.join("big.bin"), vec![0u8; 200 * 1024]).unwrap();

        let expanded = expand_at_references_in_root(&root, "see @note.txt ok");
        assert_eq!(expanded, "see <@note.txt>\nhello world ok");

        // Oversized files stay as typed text.
        let big = expand_at_references_in_root(&root, "@big.bin");
        assert_eq!(big, "@big.bin");

        // Missing files stay as typed text.
        let missing = expand_at_references_in_root(&root, "@nope.txt");
        assert_eq!(missing, "@nope.txt");
        let _ = std::fs::remove_dir_all(&root);
    }
}
