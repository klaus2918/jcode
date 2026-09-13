use super::*;
use std::fs;

const PLAN_MD: &str = r#"# 方案：示例

## 使用故事

**主线故事**

1. 用户让 agent 开始一个较大任务；`Alt+M` 一下就能看进度
2. 一眼看到变更处于哪个阶段与任务进度
3. `Esc` 回到聊天输入框

**支线故事**

- 用户按 `Alt+P` 打开页列表浮层
- 用户看到页签上的 `•` 才知道有更新

## 方案概述

1. 这一条在别的章节里，不该被当成需求
"#;

const TASKS_MD: &str = r#"---
tasks:
  - id: 1
    title: "页签模型"
    status: done
    verify: auto
    usage-step: [1, 2]
    depends: []
  - id: 2
    title: "焦点与页签切换交互（Alt+M 打开即聚焦、Tab/←→/数字直选、Esc 退回输入框并保持滚动位置）"
    status: in_progress
    verify: manual
    usage-step: [1, 3]
    depends: [1]
  - id: 3
    title: "手册同步"
    status: pending
    verify: manual
---

### T1 页签模型

- 落点：`side_panel_tabs.rs`、`ui.rs`

### T2 焦点与切页

落点：`navigation.rs`

### T3 手册同步

没有落点。
"#;

#[test]
fn parse_usage_steps_numbers_main_stories_then_continues_with_bullets() {
    let steps = parse_usage_steps(PLAN_MD);
    let numbers: Vec<usize> = steps.iter().map(|step| step.number).collect();
    assert_eq!(numbers, vec![1, 2, 3, 4, 5]);
    assert!(steps[0].text.starts_with("用户让 agent"));
    assert_eq!(steps[3].text, "用户按 `Alt+P` 打开页列表浮层");
    assert_eq!(steps[4].text, "用户看到页签上的 `•` 才知道有更新");
}

#[test]
fn parse_usage_steps_ignores_numbered_lines_outside_the_section() {
    let steps = parse_usage_steps(PLAN_MD);
    assert!(
        !steps.iter().any(|step| step.text.contains("别的章节")),
        "only the 使用故事 section counts: {:?}",
        steps
    );
}

#[test]
fn parse_usage_steps_without_a_section_is_empty() {
    assert!(parse_usage_steps("# Plan\n\n## 概述\n\n1. nothing here\n").is_empty());
}

#[test]
fn parse_task_records_read_usage_steps_and_landing_spots() {
    let records = parse_task_records(TASKS_MD);
    assert_eq!(records.len(), 3);

    assert_eq!(records[0].id, 1);
    assert_eq!(records[0].title, "页签模型");
    assert_eq!(records[0].status, "done");
    assert_eq!(records[0].usage_steps, vec![1, 2]);
    assert_eq!(
        records[0].targets,
        vec!["side_panel_tabs.rs", "ui.rs"],
        "落点 lines are split on 、 and stripped of backticks"
    );

    assert_eq!(records[1].usage_steps, vec![1, 3]);
    assert_eq!(records[1].targets, vec!["navigation.rs"]);

    assert!(records[2].usage_steps.is_empty());
    assert!(records[2].targets.is_empty());
}

#[test]
fn parse_task_records_accept_a_bare_or_snake_case_usage_step() {
    let md = "---\ntasks:\n  - id: 7\n    title: \"bare\"\n    status: pending\n    usage-step: 3\n  - id: 8\n    title: \"snake\"\n    status: pending\n    usage_step: 4\n---\n\n";
    let records = parse_task_records(md);
    assert_eq!(records[0].usage_steps, vec![3]);
    assert_eq!(records[1].usage_steps, vec![4]);
}

#[test]
fn parse_task_records_without_front_matter_is_empty() {
    assert!(parse_task_records("# Tasks\n\n- id: 1\n").is_empty());
}

#[test]
fn target_line_accepts_the_common_spellings() {
    assert_eq!(
        target_line("- 落点：`a.rs`、`b.rs`"),
        Some(vec!["a.rs".to_string(), "b.rs".to_string()])
    );
    assert_eq!(target_line("落点：`c.rs`"), Some(vec!["c.rs".to_string()]));
    assert_eq!(
        target_line("- 落点: `d.rs`, `e.rs`"),
        Some(vec!["d.rs".to_string(), "e.rs".to_string()])
    );
    assert_eq!(target_line("- 没有落点"), None);
}

#[test]
fn truncate_chars_keeps_short_text_and_marks_clipped_text() {
    assert_eq!(truncate_chars("abc", 5), "abc");
    assert_eq!(truncate_chars("abcde", 5), "abcde");
    assert_eq!(truncate_chars("abcdef", 5), "abcde…");
    assert_eq!(truncate_chars("进展面板优化", 3), "进展面…");
}

#[test]
fn render_req_map_marks_the_tree_statuses_targets_and_gaps() {
    let map = ReqMap {
        steps: parse_usage_steps(PLAN_MD),
        tasks: parse_task_records(TASKS_MD),
    };
    let markdown = render_req_map_markdown(&map);

    assert!(markdown.starts_with("# Requirements → implementation\n"));
    assert!(
        markdown.contains("**5 stories · 3 tasks (2 mapped)**"),
        "{markdown}"
    );
    assert!(markdown.contains("**1.** 用户让 agent"), "{markdown}");
    assert!(
        markdown.contains("├─ T1 页签模型 · done"),
        "a story with several tasks uses branch markers: {markdown}"
    );
    assert!(markdown.contains("└─ T2 "), "{markdown}");
    assert!(
        markdown.contains("↳ side_panel_tabs.rs, ui.rs"),
        "{markdown}"
    );
    assert!(markdown.contains("· in progress"), "{markdown}");
    assert!(
        markdown.contains('…'),
        "long titles are clipped in the tree: {markdown}"
    );
    assert!(
        markdown.contains("no task references this story"),
        "stories without tasks are called out: {markdown}"
    );
    assert!(
        markdown.contains("## Not mapped to a story (1)"),
        "{markdown}"
    );
    assert!(markdown.contains("- T3 手册同步 · pending"), "{markdown}");
}

#[test]
fn render_req_map_without_stories_says_so() {
    let map = ReqMap {
        steps: Vec::new(),
        tasks: vec![TaskRecord {
            id: 9,
            title: "solo".to_string(),
            status: "done".to_string(),
            usage_steps: Vec::new(),
            targets: Vec::new(),
        }],
    };
    let markdown = render_req_map_markdown(&map);
    assert!(
        markdown.contains("No「使用故事」section found"),
        "{markdown}"
    );
    assert!(
        markdown.contains("## Not mapped to a story (1)"),
        "{markdown}"
    );
}

fn write_change(dir: &Path, name: &str, plan: &str, tasks: &str) {
    let change_dir = dir.join(".op").join("changes").join(name);
    fs::create_dir_all(&change_dir).expect("mkdir");
    fs::write(
        change_dir.join("change.yaml"),
        format!("name: {name}\nphase: build\n"),
    )
    .expect("write change.yaml");
    if !plan.is_empty() {
        fs::write(change_dir.join("plan.md"), plan).expect("write plan.md");
    }
    if !tasks.is_empty() {
        fs::write(change_dir.join("tasks.md"), tasks).expect("write tasks.md");
    }
}

#[test]
fn load_workspace_req_map_pairs_plan_and_tasks() {
    let temp = tempfile::tempdir().expect("tempdir");
    write_change(temp.path(), "demo-change", PLAN_MD, TASKS_MD);

    let map = load_workspace_req_map(Some(temp.path())).expect("a requirement map");
    assert_eq!(map.steps.len(), 5);
    assert_eq!(map.tasks.len(), 3);
    assert_eq!(map.tasks_for_step(1).len(), 2);
    assert_eq!(map.tasks_for_step(4).len(), 0);
    assert_eq!(map.unmapped_tasks().len(), 1);
}

#[test]
fn load_workspace_req_map_needs_op_and_content() {
    let temp = tempfile::tempdir().expect("tempdir");
    assert!(load_workspace_req_map(Some(temp.path())).is_none());
    assert!(load_workspace_req_map(None).is_none());

    // A change with neither plan.md nor tasks.md has nothing to map.
    write_change(temp.path(), "empty-change", "", "");
    assert!(load_workspace_req_map(Some(temp.path())).is_none());
}
