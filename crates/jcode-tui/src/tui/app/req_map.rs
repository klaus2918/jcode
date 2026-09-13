//! The side panel's `req_map` page: requirements on the left of every line, the
//! tasks and files that implement them on the right.
//!
//! The mapping is not guessed — it is read out of the op change workspace that
//! already encodes it:
//!
//! * `plan.md`「使用故事」numbers the requirements. 主线 stories are numbered
//!   list items; 支线 stories are bullets and continue the numbering after the
//!   last numbered one (the same convention `tasks.md` documents for its
//!   `usage-step` field).
//! * `tasks.md` maps each task back to those numbers (`usage-step: [1, 2]`) and
//!   names its landing spots in the task body (`落点：\`path\``).
//!
//! Tasks with no `usage-step` are reported under "not mapped to a story" rather
//! than being hidden: a gap in the mapping is exactly what makes the page worth
//! reading.
//!
//! Like the progress page, this is an optional adapter: no `.op/` (or no
//! `plan.md`/`tasks.md`) simply means no page.

use super::App;
use crate::side_panel::{
    SidePanelPage, SidePanelPageFormat, SidePanelPageSource, SidePanelSnapshot,
};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const REQ_MAP_PAGE_TITLE: &str = "Requirements";
const REQ_MAP_PAGE_FILE: &str = "op://requirement-map";
const REQ_MAP_RESCAN_INTERVAL: Duration = Duration::from_millis(1500);
/// Long task titles are truncated in the tree so the rows stay scannable.
const TASK_TITLE_CHARS: usize = 44;

/// One requirement, as numbered by `plan.md`「使用故事」.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct UsageStep {
    pub number: usize,
    pub text: String,
}

/// One task from `tasks.md`, with the story numbers and landing spots it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TaskRecord {
    pub id: u32,
    pub title: String,
    pub status: String,
    pub usage_steps: Vec<usize>,
    pub targets: Vec<String>,
}

/// The whole requirement → implementation picture.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct ReqMap {
    pub steps: Vec<UsageStep>,
    pub tasks: Vec<TaskRecord>,
}

impl ReqMap {
    fn tasks_for_step(&self, step: usize) -> Vec<&TaskRecord> {
        self.tasks
            .iter()
            .filter(|task| task.usage_steps.contains(&step))
            .collect()
    }

    fn unmapped_tasks(&self) -> Vec<&TaskRecord> {
        self.tasks
            .iter()
            .filter(|task| task.usage_steps.is_empty())
            .collect()
    }
}

/// Collect the 「使用故事」 items of a plan.
///
/// Numbered items keep their own number; bullet items continue after the
/// highest numbered item, which is how `tasks.md` `usage-step` numbers refer to
/// the 支线 stories.
pub(super) fn parse_usage_steps(plan_md: &str) -> Vec<UsageStep> {
    let mut steps: Vec<UsageStep> = Vec::new();
    let mut bullets: Vec<String> = Vec::new();
    let mut in_section = false;

    for raw in plan_md.lines() {
        let line = raw.trim_end();
        if line.starts_with('#') {
            let heading = line.trim_start_matches('#').trim();
            in_section =
                heading.contains("使用故事") || heading.to_lowercase().contains("usage story");
            continue;
        }
        if !in_section {
            continue;
        }
        if let Some(text) = numbered_item(line) {
            steps.push(UsageStep {
                number: steps.len() + 1,
                text,
            });
        } else if let Some(text) = line.trim().strip_prefix("- ") {
            bullets.push(text.trim().to_string());
        }
    }

    // Bullets carry no numbers of their own, so they continue the sequence.
    let next = steps.len() + 1;
    for (offset, text) in bullets.into_iter().enumerate() {
        steps.push(UsageStep {
            number: next + offset,
            text,
        });
    }
    steps
}

/// `1. text` → `text`. Only plain arabic numbered items count.
fn numbered_item(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let digits: String = trimmed.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    let rest = trimmed[digits.len()..].trim_start();
    let text = rest.strip_prefix(". ").or_else(|| rest.strip_prefix('.'))?;
    Some(text.trim().to_string())
}

/// Parse the task records of a `tasks.md`.
///
/// The front matter carries id/title/status/`usage-step`; the body's
/// `### T<id>` sections carry the landing spots (`落点：\`path\``).
pub(super) fn parse_task_records(tasks_md: &str) -> Vec<TaskRecord> {
    let (front, body) = split_front_matter(tasks_md);
    let mut records: Vec<TaskRecord> = Vec::new();
    let mut current: Option<TaskRecord> = None;

    for line in front.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("- id:") {
            if let Some(record) = current.take() {
                records.push(record);
            }
            current = Some(TaskRecord {
                id: parse_task_id(trimmed),
                title: String::new(),
                status: String::new(),
                usage_steps: Vec::new(),
                targets: Vec::new(),
            });
            continue;
        }
        let Some(record) = current.as_mut() else {
            continue;
        };
        if let Some(value) = trimmed.strip_prefix("title:") {
            record.title = clean_scalar(value);
        } else if let Some(value) = trimmed.strip_prefix("status:") {
            record.status = clean_scalar(value);
        } else if let Some(value) = trimmed
            .strip_prefix("usage-step:")
            .or_else(|| trimmed.strip_prefix("usage_step:"))
        {
            record.usage_steps = parse_number_list(value);
        }
    }
    if let Some(record) = current {
        records.push(record);
    }

    attach_targets(&mut records, body);
    records
}

fn split_front_matter(text: &str) -> (&str, &str) {
    let Some(rest) = text.strip_prefix("---") else {
        return ("", text);
    };
    let rest = rest.strip_prefix('\n').unwrap_or(rest);
    match rest.find("\n---") {
        Some(end) => {
            let front = &rest[..end];
            let body_start = end + "\n---".len();
            (front, rest.get(body_start..).unwrap_or(""))
        }
        None => (rest, ""),
    }
}

/// `- id: 3` → `3`. An unparsable id becomes 0, which simply never matches a
/// task reference.
fn parse_task_id(line: &str) -> u32 {
    line.trim_start_matches("- id:").trim().parse().unwrap_or(0)
}

/// `[1, 2]` / `1` / `[]` → the numbers inside.
fn parse_number_list(value: &str) -> Vec<usize> {
    let mut numbers = Vec::new();
    for part in value.split(|ch: char| !ch.is_ascii_digit()) {
        if part.is_empty() {
            continue;
        }
        if let Ok(number) = part.parse::<usize>() {
            numbers.push(number);
        }
    }
    numbers
}

/// `### T3 <title>` sections own the `落点：...` lines that follow them.
fn attach_targets(records: &mut [TaskRecord], body: &str) {
    let mut current: Option<usize> = None;
    for line in body.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed
            .strip_prefix("### T")
            .or_else(|| trimmed.strip_prefix("###  T"))
        {
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            current = None;
            if let Ok(id) = digits.parse::<u32>() {
                current = records.iter().position(|record| record.id == id);
            }
            continue;
        }
        if trimmed.starts_with("## ") {
            current = None;
            continue;
        }
        let Some(index) = current else {
            continue;
        };
        let Some(targets) = target_line(trimmed) else {
            continue;
        };
        for target in targets {
            if !records[index].targets.contains(&target) {
                records[index].targets.push(target);
            }
        }
    }
}

/// `` 落点：`a.rs`、`b.rs` `` → `["a.rs", "b.rs"]`.
fn target_line(line: &str) -> Option<Vec<String>> {
    let rest = line
        .strip_prefix("- 落点：")
        .or_else(|| line.strip_prefix("落点："))
        .or_else(|| line.strip_prefix("- 落点:"))
        .or_else(|| line.strip_prefix("落点:"))?;
    let targets: Vec<String> = rest
        .split(['、', ',', '，'])
        .filter_map(|part| {
            let trimmed = part.trim();
            let stripped = trimmed.trim_matches('`').trim();
            (!stripped.is_empty()).then(|| stripped.to_string())
        })
        .collect();
    (!targets.is_empty()).then_some(targets)
}

fn clean_scalar(value: &str) -> String {
    let trimmed = value.trim();
    trimmed
        .strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .unwrap_or(trimmed)
        .trim()
        .to_string()
}

fn truncate_chars(text: &str, max: usize) -> String {
    let mut out: String = text.chars().take(max).collect();
    if text.chars().count() > max {
        out.push('…');
    }
    out
}

fn status_label(status: &str) -> &'static str {
    match status {
        "done" => "done",
        "in_progress" => "in progress",
        "pending" => "pending",
        "skipped" => "skipped",
        _ => "unknown",
    }
}

/// Render the tree. Box-drawing markers keep the nesting readable in the pane's
/// narrow width, and every row stays on one line so nothing reflows oddly.
pub(super) fn render_req_map_markdown(map: &ReqMap) -> String {
    let mut out = String::from("# Requirements → implementation\n\n");
    let mapped = map
        .tasks
        .iter()
        .filter(|task| !task.usage_steps.is_empty())
        .count();
    out.push_str(&format!(
        "**{} stories · {} tasks ({} mapped)**  \n",
        map.steps.len(),
        map.tasks.len(),
        mapped
    ));
    out.push_str("Source: `.op/changes/` plan + tasks  \n\n");

    if map.steps.is_empty() {
        out.push_str("No「使用故事」section found in `plan.md`.\n\n");
    }

    for step in &map.steps {
        out.push_str(&format!(
            "**{}.** {}\n",
            step.number,
            truncate_chars(&step.text, 120)
        ));
        let tasks = map.tasks_for_step(step.number);
        if tasks.is_empty() {
            out.push_str("- _no task references this story_\n\n");
            continue;
        }
        let last = tasks.len() - 1;
        for (index, task) in tasks.iter().enumerate() {
            let branch = if index == last { "└─" } else { "├─" };
            out.push_str(&format!(
                "{} T{} {} · {}\n",
                branch,
                task.id,
                truncate_chars(&task.title, TASK_TITLE_CHARS),
                status_label(&task.status)
            ));
            if !task.targets.is_empty() {
                let indent = if index == last { "   " } else { "│  " };
                out.push_str(&format!("{indent}   ↳ {}\n", task.targets.join(", ")));
            }
        }
        out.push('\n');
    }

    let unmapped = map.unmapped_tasks();
    if !unmapped.is_empty() {
        out.push_str(&format!("## Not mapped to a story ({})\n", unmapped.len()));
        for task in unmapped {
            out.push_str(&format!(
                "- T{} {} · {}\n",
                task.id,
                truncate_chars(&task.title, TASK_TITLE_CHARS),
                status_label(&task.status)
            ));
        }
        out.push('\n');
    }

    out
}

/// Read and pair the plan and tasks of the newest change under
/// `<working_dir>/.op/changes`, if there is one.
pub(super) fn load_workspace_req_map(working_dir: Option<&Path>) -> Option<ReqMap> {
    let change_dir = super::op_workspace::active_change_dir(working_dir)?;
    let mut plan = String::new();
    if let Some(text) = super::op_workspace::read_text_optional(&change_dir.join("plan.md")) {
        plan = text;
    }
    let mut tasks = String::new();
    if let Some(text) = super::op_workspace::read_text_optional(&change_dir.join("tasks.md")) {
        tasks = text;
    }
    if plan.trim().is_empty() && tasks.trim().is_empty() {
        return None;
    }
    let map = ReqMap {
        steps: parse_usage_steps(&plan),
        tasks: parse_task_records(&tasks),
    };
    (!map.steps.is_empty() || !map.tasks.is_empty()).then_some(map)
}

impl App {
    /// Re-scan the plan/tasks at most once per [`REQ_MAP_RESCAN_INTERVAL`].
    pub(super) fn refresh_req_map_view_if_needed(&mut self) -> bool {
        let now = Instant::now();
        if let Some(last) = self.req_map_checked_at
            && now.duration_since(last) < REQ_MAP_RESCAN_INTERVAL
        {
            return false;
        }
        self.req_map_checked_at = Some(now);
        self.refresh_req_map_view_now()
    }

    /// Load and re-render unconditionally. Returns true when the page changed.
    pub(super) fn refresh_req_map_view_now(&mut self) -> bool {
        let working_dir = self.session.working_dir.clone().map(PathBuf::from);
        let mut markdown = String::new();
        if let Some(map) = load_workspace_req_map(working_dir.as_deref()) {
            markdown = render_req_map_markdown(&map);
        }
        if markdown == self.req_map_markdown {
            return false;
        }
        self.req_map_markdown = markdown;
        self.req_map_updated_at_ms = now_ms();
        self.refresh_req_map_page();
        true
    }

    fn req_map_page(&self) -> SidePanelPage {
        SidePanelPage {
            id: crate::tui::ui::REQ_MAP_PAGE_ID.to_string(),
            title: REQ_MAP_PAGE_TITLE.to_string(),
            file_path: REQ_MAP_PAGE_FILE.to_string(),
            format: SidePanelPageFormat::Markdown,
            source: SidePanelPageSource::Ephemeral,
            content: self.req_map_markdown.clone(),
            updated_at_ms: self.req_map_updated_at_ms.max(1),
        }
    }

    /// Add (or drop) the requirement map page. Never steals focus, for the same
    /// reason the progress page does not: the panel is read while working.
    pub(super) fn decorate_side_panel_with_req_map(
        &self,
        mut snapshot: SidePanelSnapshot,
    ) -> SidePanelSnapshot {
        snapshot
            .pages
            .retain(|page| page.id != crate::tui::ui::REQ_MAP_PAGE_ID);
        if self.req_map_markdown.trim().is_empty() {
            if snapshot.focused_page_id.as_deref() == Some(crate::tui::ui::REQ_MAP_PAGE_ID) {
                snapshot.focused_page_id = self
                    .last_side_panel_focus_id
                    .clone()
                    .filter(|id| snapshot.pages.iter().any(|page| page.id == *id))
                    .or_else(|| snapshot.pages.first().map(|page| page.id.clone()));
            }
            return snapshot;
        }

        snapshot.pages.push(self.req_map_page());
        snapshot.pages.sort_by(|a, b| {
            b.updated_at_ms
                .cmp(&a.updated_at_ms)
                .then_with(|| a.id.cmp(&b.id))
        });
        snapshot
    }

    pub(super) fn snapshot_without_req_map(&self) -> SidePanelSnapshot {
        let mut snapshot = self.side_panel.clone();
        snapshot
            .pages
            .retain(|page| page.id != crate::tui::ui::REQ_MAP_PAGE_ID);
        if snapshot.focused_page_id.as_deref() == Some(crate::tui::ui::REQ_MAP_PAGE_ID) {
            snapshot.focused_page_id = None;
        }
        snapshot
    }

    fn refresh_req_map_page(&mut self) {
        let snapshot = self.snapshot_without_req_map();
        let snapshot = self.decorate_side_panel_with_req_map(snapshot);
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
#[path = "req_map_tests.rs"]
mod tests;
