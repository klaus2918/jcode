use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SidePanelPageFormat {
    #[default]
    Markdown,
}

impl SidePanelPageFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Markdown => "markdown",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SidePanelPage {
    pub id: String,
    pub title: String,
    pub file_path: String,
    #[serde(default)]
    pub format: SidePanelPageFormat,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SidePanelSnapshot {
    #[serde(default)]
    pub focused_page_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pages: Vec<SidePanelPage>,
}

impl SidePanelSnapshot {
    pub fn has_pages(&self) -> bool {
        !self.pages.is_empty()
    }

    pub fn focused_page(&self) -> Option<&SidePanelPage> {
        let focused_id = self.focused_page_id.as_deref()?;
        self.pages.iter().find(|page| page.id == focused_id)
    }
}

pub fn snapshot_is_empty(snapshot: &SidePanelSnapshot) -> bool {
    !snapshot.has_pages()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PersistedSidePanelState {
    #[serde(default)]
    focused_page_id: Option<String>,
    #[serde(default)]
    pages: Vec<PersistedSidePanelPage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedSidePanelPage {
    id: String,
    title: String,
    file_path: String,
    #[serde(default)]
    format: SidePanelPageFormat,
    updated_at_ms: u64,
}

pub fn snapshot_for_session(session_id: &str) -> Result<SidePanelSnapshot> {
    let state = load_state(session_id)?;
    hydrate_snapshot(state)
}

pub fn write_markdown_page(
    session_id: &str,
    page_id: &str,
    title: Option<&str>,
    content: &str,
    focus: bool,
) -> Result<SidePanelSnapshot> {
    write_page(session_id, page_id, title, content, focus, false)
}

pub fn append_markdown_page(
    session_id: &str,
    page_id: &str,
    title: Option<&str>,
    content: &str,
    focus: bool,
) -> Result<SidePanelSnapshot> {
    write_page(session_id, page_id, title, content, focus, true)
}

pub fn focus_page(session_id: &str, page_id: &str) -> Result<SidePanelSnapshot> {
    validate_page_id(page_id)?;
    let mut state = load_state(session_id)?;
    if state.pages.iter().any(|page| page.id == page_id) {
        state.focused_page_id = Some(page_id.to_string());
        save_state(session_id, &state)?;
        hydrate_snapshot(state)
    } else {
        anyhow::bail!("Side panel page not found: {}", page_id);
    }
}

pub fn delete_page(session_id: &str, page_id: &str) -> Result<SidePanelSnapshot> {
    validate_page_id(page_id)?;
    let mut state = load_state(session_id)?;
    let before = state.pages.len();
    state.pages.retain(|page| page.id != page_id);
    if state.pages.len() == before {
        anyhow::bail!("Side panel page not found: {}", page_id);
    }

    let page_path = session_dir(session_id)?.join(format!("{}.md", page_id));
    let _ = std::fs::remove_file(page_path);

    if state.focused_page_id.as_deref() == Some(page_id) {
        state.focused_page_id = state
            .pages
            .iter()
            .max_by_key(|page| page.updated_at_ms)
            .map(|page| page.id.clone());
    }

    save_state(session_id, &state)?;
    hydrate_snapshot(state)
}

pub fn status_output(snapshot: &SidePanelSnapshot) -> String {
    if snapshot.pages.is_empty() {
        return "Side panel: empty".to_string();
    }

    let focused = snapshot
        .focused_page()
        .map(|page| page.id.as_str())
        .unwrap_or("none");
    let mut out = format!(
        "Side panel: {} page{}\nFocused: {}\n",
        snapshot.pages.len(),
        if snapshot.pages.len() == 1 { "" } else { "s" },
        focused
    );

    for page in &snapshot.pages {
        let focus_marker = if snapshot.focused_page_id.as_deref() == Some(page.id.as_str()) {
            "*"
        } else {
            " "
        };
        out.push_str(&format!(
            "{} {} ({})\n  title: {}\n  file: {}\n",
            focus_marker,
            page.id,
            page.format.as_str(),
            page.title,
            page.file_path
        ));
    }

    out.trim_end().to_string()
}

fn write_page(
    session_id: &str,
    page_id: &str,
    title: Option<&str>,
    content: &str,
    focus: bool,
    append: bool,
) -> Result<SidePanelSnapshot> {
    validate_page_id(page_id)?;
    let dir = session_dir(session_id)?;
    crate::storage::ensure_dir(&dir)?;

    let page_path = dir.join(format!("{}.md", page_id));
    let mut state = load_state(session_id)?;
    let now = now_ms();

    let combined_content = if append && page_path.exists() {
        let mut existing = std::fs::read_to_string(&page_path)
            .with_context(|| format!("failed to read {}", page_path.display()))?;
        if !existing.is_empty() && !existing.ends_with('\n') {
            existing.push('\n');
        }
        existing.push_str(content);
        existing
    } else {
        content.to_string()
    };

    std::fs::write(&page_path, &combined_content)
        .with_context(|| format!("failed to write {}", page_path.display()))?;

    if let Some(existing) = state.pages.iter_mut().find(|page| page.id == page_id) {
        existing.title = title
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .unwrap_or(existing.title.as_str())
            .to_string();
        existing.file_path = page_path.display().to_string();
        existing.format = SidePanelPageFormat::Markdown;
        existing.updated_at_ms = now;
    } else {
        state.pages.push(PersistedSidePanelPage {
            id: page_id.to_string(),
            title: title
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .unwrap_or(page_id)
                .to_string(),
            file_path: page_path.display().to_string(),
            format: SidePanelPageFormat::Markdown,
            updated_at_ms: now,
        });
    }

    state.pages.sort_by(|a, b| {
        b.updated_at_ms
            .cmp(&a.updated_at_ms)
            .then_with(|| a.id.cmp(&b.id))
    });

    if focus || state.focused_page_id.is_none() {
        state.focused_page_id = Some(page_id.to_string());
    }

    save_state(session_id, &state)?;
    hydrate_snapshot(state)
}

fn hydrate_snapshot(state: PersistedSidePanelState) -> Result<SidePanelSnapshot> {
    let pages = state
        .pages
        .into_iter()
        .map(|page| {
            let content = std::fs::read_to_string(&page.file_path).unwrap_or_default();
            SidePanelPage {
                id: page.id,
                title: page.title,
                file_path: page.file_path,
                format: page.format,
                content,
                updated_at_ms: page.updated_at_ms,
            }
        })
        .collect();

    Ok(SidePanelSnapshot {
        focused_page_id: state.focused_page_id,
        pages,
    })
}

fn load_state(session_id: &str) -> Result<PersistedSidePanelState> {
    let path = state_file(session_id)?;
    if !path.exists() {
        return Ok(PersistedSidePanelState::default());
    }
    crate::storage::read_json(&path)
}

fn save_state(session_id: &str, state: &PersistedSidePanelState) -> Result<()> {
    let path = state_file(session_id)?;
    crate::storage::write_json_fast(&path, state)
}

fn session_dir(session_id: &str) -> Result<PathBuf> {
    let base = crate::storage::jcode_dir()?.join("side_panel");
    Ok(base.join(session_id))
}

fn state_file(session_id: &str) -> Result<PathBuf> {
    Ok(session_dir(session_id)?.join("index.json"))
}

fn validate_page_id(page_id: &str) -> Result<()> {
    let page_id = page_id.trim();
    if page_id.is_empty() {
        anyhow::bail!("page_id cannot be empty");
    }
    if page_id.len() > 80 {
        anyhow::bail!("page_id is too long (max 80 characters)");
    }
    if !page_id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
    {
        anyhow::bail!("page_id must use only ASCII letters, digits, underscore, dash, or dot");
    }
    if page_id.contains("..") {
        anyhow::bail!("page_id cannot contain '..'");
    }
    if Path::new(page_id).components().count() != 1 {
        anyhow::bail!("page_id cannot contain path separators");
    }
    Ok(())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|dur| dur.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn side_panel_pages_persist_and_focus_latest() {
        let _guard = crate::storage::lock_test_env();
        let temp = tempfile::tempdir().expect("tempdir");
        let prev_home = std::env::var_os("JCODE_HOME");
        crate::env::set_var("JCODE_HOME", temp.path());

        let session_id = "ses_side_panel_test";
        let first = write_markdown_page(session_id, "notes", Some("Notes"), "# Notes", true)
            .expect("write notes");
        assert_eq!(first.focused_page_id.as_deref(), Some("notes"));
        assert_eq!(first.pages.len(), 1);

        let second = write_markdown_page(session_id, "plan", Some("Plan"), "# Plan", true)
            .expect("write plan");
        assert_eq!(second.focused_page_id.as_deref(), Some("plan"));
        assert_eq!(second.pages.len(), 2);
        assert_eq!(
            second.focused_page().map(|p| p.title.as_str()),
            Some("Plan")
        );

        let appended =
            append_markdown_page(session_id, "notes", None, "- item", false).expect("append notes");
        let notes = appended
            .pages
            .iter()
            .find(|page| page.id == "notes")
            .expect("notes page");
        assert!(notes.content.contains("- item"));
        assert_eq!(appended.focused_page_id.as_deref(), Some("plan"));

        let focused = focus_page(session_id, "notes").expect("focus notes");
        assert_eq!(focused.focused_page_id.as_deref(), Some("notes"));

        let reloaded = snapshot_for_session(session_id).expect("reload snapshot");
        assert_eq!(reloaded.focused_page_id.as_deref(), Some("notes"));
        assert_eq!(reloaded.pages.len(), 2);

        if let Some(prev_home) = prev_home {
            crate::env::set_var("JCODE_HOME", prev_home);
        } else {
            crate::env::remove_var("JCODE_HOME");
        }
    }

    #[test]
    fn side_panel_delete_falls_back_to_most_recent_page() {
        let _guard = crate::storage::lock_test_env();
        let temp = tempfile::tempdir().expect("tempdir");
        let prev_home = std::env::var_os("JCODE_HOME");
        crate::env::set_var("JCODE_HOME", temp.path());

        let session_id = "ses_side_panel_delete";
        write_markdown_page(session_id, "one", Some("One"), "# One", true).expect("page one");
        write_markdown_page(session_id, "two", Some("Two"), "# Two", true).expect("page two");

        let after_delete = delete_page(session_id, "two").expect("delete page two");
        assert_eq!(after_delete.pages.len(), 1);
        assert_eq!(after_delete.focused_page_id.as_deref(), Some("one"));

        if let Some(prev_home) = prev_home {
            crate::env::set_var("JCODE_HOME", prev_home);
        } else {
            crate::env::remove_var("JCODE_HOME");
        }
    }
}
