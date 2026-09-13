//! The side panel's `progress` page: what the current piece of work is doing.
//!
//! Source of truth is the workspace's `op` change directory
//! (`<working_dir>/.op/changes/<name>/`): its `change.yaml` phase/blockers and
//! its `tasks.md` task counts. The adapter is strictly optional — a workspace
//! without `.op/` simply gets no progress page, which is the agreed boundary
//! (`.op` is the op skill's private workspace and must not become a hard
//! dependency of the TUI).
//!
//! Todos and goals deliberately do **not** appear here: the `session_todos`
//! page already renders those (`todos_view.rs`), and duplicating them would give
//! the side panel two sources of truth for the same list.
//!
//! Everything except `load_workspace_progress` is pure so the parsing and the
//! rendered markdown can be unit-tested without a filesystem.

use super::App;
use crate::side_panel::{
    SidePanelPage, SidePanelPageFormat, SidePanelPageSource, SidePanelSnapshot,
};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const PROGRESS_PAGE_TITLE: &str = "Progress";
const PROGRESS_PAGE_FILE: &str = "op://workspace-progress";
/// How often the op workspace is re-scanned while the page is alive. The scan is
/// two small file reads, but a tick-driven repaint should not do it every frame.
const PROGRESS_RESCAN_INTERVAL: Duration = Duration::from_millis(1500);

/// Task status counts parsed out of a `tasks.md` front matter.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct TaskCounts {
    pub total: usize,
    pub done: usize,
    pub in_progress: usize,
    pub pending: usize,
    pub skipped: usize,
}

impl TaskCounts {
    pub(super) fn percent_done(self) -> u64 {
        if self.total == 0 {
            return 0;
        }
        ((self.done as f64 / self.total as f64) * 100.0).round() as u64
    }

    /// `████░░░░` style bar, `width` cells wide.
    fn bar(self, width: usize) -> String {
        if width == 0 {
            return String::new();
        }
        let filled = ((self.done * width) + self.total / 2)
            .checked_div(self.total.max(1))
            .unwrap_or(0)
            .min(width);
        let mut bar = String::with_capacity(width);
        for cell in 0..width {
            bar.push(if cell < filled { '█' } else { '░' });
        }
        bar
    }
}

/// Top-level scalars of a `change.yaml` we care about.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct ChangeMeta {
    pub name: Option<String>,
    pub title: Option<String>,
    pub phase: Option<String>,
    pub blockers: Vec<String>,
}

/// Everything the progress page renders.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct ChangeProgress {
    pub name: String,
    pub title: Option<String>,
    pub phase: Option<String>,
    pub tasks: TaskCounts,
    pub in_flight: Option<String>,
    pub blockers: Vec<String>,
}

/// Parse the scalars this page needs out of a `change.yaml`.
///
/// Only column-0 keys are read, which is what makes this safe without a YAML
/// parser: the file's block scalars (`summary: >`) and nested lists are all
/// indented, so they can never be mistaken for a top-level key.
pub(super) fn parse_change_meta(text: &str) -> ChangeMeta {
    let lines: Vec<&str> = text.lines().collect();
    let mut meta = ChangeMeta::default();
    for line in &lines {
        if line.starts_with(char::is_whitespace) {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        match key.trim() {
            "name" => meta.name = Some(clean_scalar(value)),
            "title" => meta.title = Some(clean_scalar(value)),
            "phase" => meta.phase = Some(clean_scalar(value)),
            _ => {}
        }
    }
    meta.blockers = parse_blockers(&lines);
    meta
}

/// Blocker entries under a top-level `blockers:` key. Each item shows its
/// `desc:` when present, otherwise the raw `- ` text (`- id: B1` and friends).
fn parse_blockers(lines: &[&str]) -> Vec<String> {
    let Some(start) = lines.iter().position(|line| line.starts_with("blockers:")) else {
        return Vec::new();
    };
    if !lines[start]
        .trim_start_matches("blockers:")
        .trim()
        .is_empty()
    {
        // `blockers: []` (or an inline list): nothing to itemize.
        return Vec::new();
    }

    let mut out: Vec<String> = Vec::new();
    let mut current: Option<String> = None;
    for line in &lines[start + 1..] {
        if !line.starts_with(char::is_whitespace) {
            break;
        }
        let trimmed = line.trim();
        if let Some(item) = trimmed.strip_prefix("- ") {
            if let Some(previous) = current.take() {
                out.push(previous);
            }
            current = Some(clean_scalar(item));
        } else if let Some(desc) = trimmed.strip_prefix("desc:")
            && current.is_some()
        {
            // A `desc:` line is better copy than the `- id: X` placeholder.
            current = Some(clean_scalar(desc));
        }
    }
    if let Some(last) = current {
        out.push(last);
    }
    out
}

/// Task counts plus the first `in_progress` task title, from a `tasks.md`.
///
/// Only the YAML front matter counts; the prose below it may mention statuses
/// and must not be tallied.
pub(super) fn parse_tasks(text: &str) -> (TaskCounts, Option<String>) {
    let Some(front) = front_matter(text) else {
        return (TaskCounts::default(), None);
    };

    let mut counts = TaskCounts::default();
    let mut in_flight: Option<String> = None;
    let mut title: Option<String> = None;
    let mut status: Option<String> = None;
    let mut saw_record = false;

    for raw in front.lines() {
        let trimmed = raw.trim();
        if trimmed.starts_with("- id:") {
            if saw_record {
                tally(&title, &status, &mut counts, &mut in_flight);
            }
            saw_record = true;
            title = None;
            status = None;
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("title:") {
            title = Some(clean_scalar(value));
        } else if let Some(value) = trimmed.strip_prefix("status:") {
            status = Some(clean_scalar(value));
        }
    }
    if saw_record {
        tally(&title, &status, &mut counts, &mut in_flight);
    }
    (counts, in_flight)
}

fn tally(
    title: &Option<String>,
    status: &Option<String>,
    counts: &mut TaskCounts,
    in_flight: &mut Option<String>,
) {
    let Some(status) = status.as_deref() else {
        return;
    };
    match status {
        "done" => counts.done += 1,
        "pending" => counts.pending += 1,
        "skipped" => counts.skipped += 1,
        "in_progress" => {
            counts.in_progress += 1;
            if in_flight.is_none() {
                *in_flight = title.clone();
            }
        }
        _ => return,
    }
    counts.total += 1;
}

/// The `---`-delimited front matter of a markdown file.
fn front_matter(text: &str) -> Option<&str> {
    let rest = text.strip_prefix("---")?;
    let rest = rest.strip_prefix('\n').unwrap_or(rest);
    let end = rest.find("\n---")?;
    Some(&rest[..end])
}

fn clean_scalar(value: &str) -> String {
    let trimmed = value.trim();
    let unquoted = trimmed
        .strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .or_else(|| {
            trimmed
                .strip_prefix('\'')
                .and_then(|inner| inner.strip_suffix('\''))
        })
        .unwrap_or(trimmed);
    unquoted.trim().to_string()
}

/// Render the page body.
pub(super) fn render_progress_markdown(change: &ChangeProgress) -> String {
    let mut out = String::from("# Progress\n\n");
    match change.title.as_deref() {
        Some(title) if !title.is_empty() => {
            out.push_str(&format!("**Change:** {title}  \n"));
            out.push_str(&format!("**Id:** `{}`  \n", change.name));
        }
        _ => out.push_str(&format!("**Change:** `{}`  \n", change.name)),
    }
    if let Some(phase) = change.phase.as_deref() {
        out.push_str(&format!("**Phase:** `{phase}`  \n"));
    }
    out.push('\n');

    if change.tasks.total > 0 {
        out.push_str(&format!(
            "**Tasks:** {}/{} done ({}%)  \n",
            change.tasks.done,
            change.tasks.total,
            change.tasks.percent_done()
        ));
        out.push_str(&format!("`{}`\n\n", change.tasks.bar(24)));
        let mut parts: Vec<String> = Vec::new();
        if change.tasks.in_progress > 0 {
            parts.push(format!("{} in progress", change.tasks.in_progress));
        }
        if change.tasks.pending > 0 {
            parts.push(format!("{} pending", change.tasks.pending));
        }
        if change.tasks.skipped > 0 {
            parts.push(format!("{} skipped", change.tasks.skipped));
        }
        if !parts.is_empty() {
            out.push_str(&format!("{}\n\n", parts.join(" · ")));
        }
    } else {
        out.push_str("**Tasks:** no `tasks.md` front matter found  \n\n");
    }

    if let Some(in_flight) = change.in_flight.as_deref() {
        out.push_str("## In flight\n");
        out.push_str(&format!("- {in_flight}\n\n"));
    }

    if change.blockers.is_empty() {
        out.push_str("## Blockers\n- none\n\n");
    } else {
        out.push_str(&format!("## Blockers ({})\n", change.blockers.len()));
        for blocker in &change.blockers {
            out.push_str(&format!("- {blocker}\n"));
        }
        out.push('\n');
    }

    out.push_str(&format!(
        "---\nSource: `.op/changes/{}/` in the session working directory.\n",
        change.name
    ));
    out
}

/// Read the op change workspace under `working_dir`, if there is one.
pub(super) fn load_workspace_progress(working_dir: Option<&Path>) -> Option<ChangeProgress> {
    let change_dir = super::op_workspace::active_change_dir(working_dir)?;
    let name = change_dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "change".to_string());

    let mut meta = ChangeMeta::default();
    if let Some(text) = super::op_workspace::read_text_optional(&change_dir.join("change.yaml")) {
        meta = parse_change_meta(&text);
    }
    let mut tasks = TaskCounts::default();
    let mut in_flight = None;
    if let Some(text) = super::op_workspace::read_text_optional(&change_dir.join("tasks.md")) {
        let (parsed_tasks, parsed_in_flight) = parse_tasks(&text);
        tasks = parsed_tasks;
        in_flight = parsed_in_flight;
    }

    Some(ChangeProgress {
        name: meta.name.unwrap_or(name),
        title: meta.title,
        phase: meta.phase,
        tasks,
        in_flight,
        blockers: meta.blockers,
    })
}

impl App {
    /// Re-scan the op workspace at most once per [`PROGRESS_RESCAN_INTERVAL`].
    pub(super) fn refresh_progress_view_if_needed(&mut self) -> bool {
        let now = Instant::now();
        if let Some(last) = self.progress_checked_at
            && now.duration_since(last) < PROGRESS_RESCAN_INTERVAL
        {
            return false;
        }
        self.progress_checked_at = Some(now);
        self.refresh_progress_view_now()
    }

    /// Load and re-render unconditionally. Returns true when the page changed.
    pub(super) fn refresh_progress_view_now(&mut self) -> bool {
        let working_dir = self.session.working_dir.clone().map(PathBuf::from);
        let mut markdown = String::new();
        if let Some(change) = load_workspace_progress(working_dir.as_deref()) {
            markdown = render_progress_markdown(&change);
        }
        if markdown == self.progress_markdown {
            return false;
        }
        self.progress_markdown = markdown;
        self.progress_updated_at_ms = now_ms();
        self.refresh_progress_page();
        true
    }

    fn progress_page(&self) -> SidePanelPage {
        SidePanelPage {
            id: crate::tui::ui::PROGRESS_PAGE_ID.to_string(),
            title: PROGRESS_PAGE_TITLE.to_string(),
            file_path: PROGRESS_PAGE_FILE.to_string(),
            format: SidePanelPageFormat::Markdown,
            source: SidePanelPageSource::Ephemeral,
            content: self.progress_markdown.clone(),
            updated_at_ms: self.progress_updated_at_ms.max(1),
        }
    }

    /// Add (or drop) the progress page on a snapshot. Never steals focus: the
    /// panel is a read-while-working surface, so a page appearing in it must not
    /// move the user's keyboard.
    pub(super) fn decorate_side_panel_with_progress(
        &self,
        mut snapshot: SidePanelSnapshot,
    ) -> SidePanelSnapshot {
        snapshot
            .pages
            .retain(|page| page.id != crate::tui::ui::PROGRESS_PAGE_ID);
        if self.progress_markdown.trim().is_empty() {
            if snapshot.focused_page_id.as_deref() == Some(crate::tui::ui::PROGRESS_PAGE_ID) {
                snapshot.focused_page_id = self
                    .last_side_panel_focus_id
                    .clone()
                    .filter(|id| snapshot.pages.iter().any(|page| page.id == *id))
                    .or_else(|| snapshot.pages.first().map(|page| page.id.clone()));
            }
            return snapshot;
        }

        snapshot.pages.push(self.progress_page());
        snapshot.pages.sort_by(|a, b| {
            b.updated_at_ms
                .cmp(&a.updated_at_ms)
                .then_with(|| a.id.cmp(&b.id))
        });
        snapshot
    }

    pub(super) fn snapshot_without_progress(&self) -> SidePanelSnapshot {
        let mut snapshot = self.side_panel.clone();
        snapshot
            .pages
            .retain(|page| page.id != crate::tui::ui::PROGRESS_PAGE_ID);
        if snapshot.focused_page_id.as_deref() == Some(crate::tui::ui::PROGRESS_PAGE_ID) {
            snapshot.focused_page_id = None;
        }
        snapshot
    }

    fn refresh_progress_page(&mut self) {
        let snapshot = self.snapshot_without_progress();
        let snapshot = self.decorate_side_panel_with_progress(snapshot);
        self.apply_side_panel_snapshot(snapshot);
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "progress_view_tests.rs"]
mod tests;
