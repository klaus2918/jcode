use super::*;
use std::fs;
use std::time::Duration;

const CHANGE_YAML: &str = r#"name: sidebar-info-panel-redesign
title: 侧栏信息面板重构（交互能力 + 展示效果）
type: complex
phase: build
has_ui: true
summary: >
  把 TUI 侧栏从「静态展示、只读」
  提升为「关键辅助信息区」。
decisions:
  - id: D1
    question: Alt+M 语义
blockers:
  - id: B1
    desc: D1-D5 未裁决前，T4/T6 的键位语义无法开工
    action: plan.md 暂停点向用户裁决
  - id: B4
    desc: 执行约束 allow_commit 仍为 false
"#;

const TASKS_MD: &str = r#"---
tasks:
  - id: 1
    title: "页签模型"
    status: done
    verify: auto
  - id: 2
    title: "比例配置化"
    status: done
  - id: 3
    title: "页签栏渲染"
    status: in_progress
  - id: 4
    title: "页列表浮层"
    status: pending
  - id: 5
    title: "已跳过"
    status: skipped
---

# 任务清单

正文里写一句 status: pending 不应被统计。
"#;

#[test]
fn parse_change_meta_reads_top_level_scalars() {
    let meta = parse_change_meta(CHANGE_YAML);
    assert_eq!(meta.name.as_deref(), Some("sidebar-info-panel-redesign"));
    assert_eq!(
        meta.title.as_deref(),
        Some("侧栏信息面板重构（交互能力 + 展示效果）")
    );
    assert_eq!(meta.phase.as_deref(), Some("build"));
}

#[test]
fn parse_change_meta_collects_blocker_descriptions() {
    let meta = parse_change_meta(CHANGE_YAML);
    assert_eq!(meta.blockers.len(), 2, "only the blockers list counts");
    assert!(meta.blockers[0].contains("D1-D5"), "{:?}", meta.blockers);
    assert!(
        meta.blockers[1].contains("allow_commit"),
        "{:?}",
        meta.blockers
    );
}

#[test]
fn parse_change_meta_treats_an_inline_empty_list_as_no_blockers() {
    let text = "name: x\nphase: plan\nblockers: []\ndecisions: []\n";
    let meta = parse_change_meta(text);
    assert!(meta.blockers.is_empty());
    assert_eq!(meta.phase.as_deref(), Some("plan"));
}

#[test]
fn parse_change_meta_ignores_block_scalar_bodies() {
    // `summary: >` bodies are indented; they must never be read as top-level
    // keys even when they contain a colon.
    let text = "name: real\nsummary: >\n  phase: not-a-phase\n  name: not-a-name\n";
    let meta = parse_change_meta(text);
    assert_eq!(meta.name.as_deref(), Some("real"));
    assert_eq!(meta.phase, None);
    assert_eq!(meta.title, None);
}

#[test]
fn parse_tasks_counts_statuses_and_finds_the_in_flight_one() {
    let (counts, in_flight) = parse_tasks(TASKS_MD);
    assert_eq!(
        counts,
        TaskCounts {
            total: 5,
            done: 2,
            in_progress: 1,
            pending: 1,
            skipped: 1,
        },
        "prose below the front matter must not be tallied"
    );
    assert_eq!(in_flight.as_deref(), Some("页签栏渲染"));
}

#[test]
fn parse_tasks_without_front_matter_is_empty() {
    let (counts, in_flight) = parse_tasks("# Just a document\n\n- status: done\n");
    assert_eq!(counts, TaskCounts::default());
    assert_eq!(in_flight, None);
}

#[test]
fn task_counts_render_percent_and_bar() {
    let counts = TaskCounts {
        total: 5,
        done: 2,
        in_progress: 1,
        pending: 1,
        skipped: 1,
    };
    assert_eq!(counts.percent_done(), 40);
    let bar = counts.bar(10);
    assert_eq!(bar.chars().count(), 10);
    assert_eq!(bar.chars().filter(|ch| *ch == '█').count(), 4);
    assert_eq!(TaskCounts::default().percent_done(), 0);
    assert_eq!(
        TaskCounts::default()
            .bar(10)
            .chars()
            .filter(|c| *c == '░')
            .count(),
        10
    );
}

#[test]
fn render_progress_markdown_covers_phase_tasks_in_flight_and_blockers() {
    let meta = parse_change_meta(CHANGE_YAML);
    let (tasks, in_flight) = parse_tasks(TASKS_MD);
    let change = ChangeProgress {
        name: meta.name.clone().unwrap_or_default(),
        title: meta.title.clone(),
        phase: meta.phase.clone(),
        tasks,
        in_flight,
        blockers: meta.blockers.clone(),
    };

    let markdown = render_progress_markdown(&change);
    assert!(markdown.starts_with("# Progress\n"));
    assert!(markdown.contains("**Phase:** `build`"), "{markdown}");
    assert!(markdown.contains("2/5 done (40%)"), "{markdown}");
    assert!(markdown.contains("页签栏渲染"), "{markdown}");
    assert!(markdown.contains("Blockers (2)"), "{markdown}");
    assert!(markdown.contains("allow_commit"), "{markdown}");
    assert!(
        markdown.contains("`.op/changes/sidebar-info-panel-redesign/`"),
        "{markdown}"
    );
}

#[test]
fn render_progress_markdown_says_none_when_there_are_no_blockers() {
    let change = ChangeProgress {
        name: "x".to_string(),
        tasks: TaskCounts::default(),
        ..Default::default()
    };
    let markdown = render_progress_markdown(&change);
    assert!(markdown.contains("## Blockers\n- none"), "{markdown}");
    assert!(
        markdown.contains("no `tasks.md` front matter found"),
        "{markdown}"
    );
}

fn write_change(dir: &Path, name: &str, phase: &str, with_tasks: bool) {
    let change_dir = dir.join(".op").join("changes").join(name);
    fs::create_dir_all(&change_dir).expect("mkdir");
    fs::write(
        change_dir.join("change.yaml"),
        format!("name: {name}\ntitle: {name} title\nphase: {phase}\nblockers: []\n"),
    )
    .expect("write change.yaml");
    if with_tasks {
        fs::write(change_dir.join("tasks.md"), TASKS_MD).expect("write tasks.md");
    }
}

#[test]
fn load_workspace_progress_reads_the_newest_change() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    write_change(root, "older-change", "plan", false);
    std::thread::sleep(Duration::from_millis(25));
    write_change(root, "newer-change", "build", true);

    let change = load_workspace_progress(Some(root)).expect("a change directory");
    assert_eq!(change.name, "newer-change");
    assert_eq!(change.phase.as_deref(), Some("build"));
    assert_eq!(change.tasks.total, 5);
    assert_eq!(change.in_flight.as_deref(), Some("页签栏渲染"));
}

#[test]
fn load_workspace_progress_tolerates_a_missing_tasks_file() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    write_change(root, "solo", "plan", false);

    let change = load_workspace_progress(Some(root)).expect("a change directory");
    assert_eq!(change.name, "solo");
    assert_eq!(change.tasks, TaskCounts::default());
    assert_eq!(change.in_flight, None);
}

#[test]
fn load_workspace_progress_without_an_op_directory_is_none() {
    let temp = tempfile::tempdir().expect("tempdir");
    assert!(load_workspace_progress(Some(temp.path())).is_none());
    assert!(load_workspace_progress(None).is_none());
}

#[test]
fn load_workspace_progress_skips_directories_without_a_change_yaml() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    fs::create_dir_all(root.join(".op").join("changes").join("not-a-change")).expect("mkdir");
    assert!(
        load_workspace_progress(Some(root)).is_none(),
        "a change directory without change.yaml is not a change"
    );
}
