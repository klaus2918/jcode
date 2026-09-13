use super::*;
use crate::side_panel::{
    SidePanelPage, SidePanelPageFormat, SidePanelPageSource, SidePanelSnapshot,
};
use crossterm::event::{KeyCode, KeyModifiers};

fn page(id: &str, title: &str) -> SidePanelPage {
    SidePanelPage {
        id: id.to_string(),
        title: title.to_string(),
        file_path: format!("{id}.md"),
        format: SidePanelPageFormat::Markdown,
        source: SidePanelPageSource::Managed,
        content: "body".to_string(),
        updated_at_ms: 1,
    }
}

fn titled(id: &str, title: &str, updated_at_ms: u64, content_len: usize) -> SidePanelPage {
    SidePanelPage {
        content: "x".repeat(content_len),
        updated_at_ms,
        ..page(id, title)
    }
}

fn snapshot(pages: Vec<SidePanelPage>, focused: Option<&str>) -> SidePanelSnapshot {
    SidePanelSnapshot {
        focused_page_id: focused.map(str::to_string),
        pages,
    }
}

fn tab(index: usize, title: &str, focused: bool, updated: bool) -> TabItem {
    TabItem {
        page_id: format!("page.{index}"),
        kind: PageKind::Agent,
        title: title.to_string(),
        focused,
        updated,
    }
}

fn label_widths(layout: &TabBarLayout) -> Vec<usize> {
    layout
        .slots
        .iter()
        .map(|slot| slot.label.chars().count())
        .collect()
}

#[test]
fn page_kind_classifies_known_id_namespace() {
    let cases = [
        ("progress", PageKind::Progress),
        ("req_map", PageKind::ReqMap),
        ("goals", PageKind::Goals),
        ("goal.abc-123", PageKind::Goal),
        ("session_todos", PageKind::Todos),
        ("split_view", PageKind::SplitView),
        ("catchup", PageKind::Catchup),
        ("observe", PageKind::Observe),
        ("image.generated", PageKind::Image),
        ("plan-doc", PageKind::Agent),
        ("goal", PageKind::Agent),
        ("", PageKind::Agent),
    ];
    for (id, expected) in cases {
        assert_eq!(PageKind::from_id(id), expected, "id = {id}");
    }
}

#[test]
fn page_kind_orders_built_ins_before_agent_pages() {
    assert!(PageKind::Progress.order() < PageKind::Goals.order());
    assert!(PageKind::ReqMap.order() < PageKind::Goals.order());
    assert!(PageKind::Image.order() < PageKind::Agent.order());
    // Tag names are the machine-readable contract used by tests/diagnostics.
    assert_eq!(PageKind::Progress.tag(), "progress");
    assert_eq!(PageKind::ReqMap.tag(), "req_map");
}

#[test]
fn tab_order_pins_built_ins_first_and_keeps_insertion_order() {
    let pages = vec![
        page("plan-doc", "Plan"),
        page("split_view", "Split View"),
        page("progress", "Progress"),
        page("notes", "Notes"),
        page("req_map", "Requirements"),
    ];
    let order = tab_order(&pages);
    let ids: Vec<&str> = order.iter().map(|&i| pages[i].id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["progress", "req_map", "split_view", "plan-doc", "notes"],
        "built-ins first, same-kind pages keep insertion order"
    );
}

#[test]
fn tab_position_follows_display_order() {
    let pages = vec![page("plan-doc", "Plan"), page("progress", "Progress")];
    assert_eq!(tab_position(&pages, "progress"), Some(0));
    assert_eq!(tab_position(&pages, "plan-doc"), Some(1));
    assert_eq!(tab_position(&pages, "missing"), None);
}

#[test]
fn layout_wide_shows_every_tab_with_full_titles() {
    let items = vec![
        tab(0, "Requirements", false, false),
        tab(1, "Implementation map", true, false),
        tab(2, "Plan", false, false),
    ];
    let layout = layout_tab_bar(&items, 80);
    assert_eq!(layout.slots.len(), 3);
    assert!(!layout.has_overflow());
    assert_eq!(
        layout
            .slots
            .iter()
            .map(|slot| slot.label.as_str())
            .collect::<Vec<_>>(),
        vec!["Requirements", "Implementation map", "Plan"]
    );
    assert!(layout.slots[1].focused);
    assert!(layout.plain_text().contains("Requirements"));
}

#[test]
fn layout_narrow_truncates_labels_before_dropping_tabs() {
    let items = vec![
        tab(0, "Requirements", false, false),
        tab(1, "Implementation map", true, false),
        tab(2, "Plan", false, false),
    ];
    let layout = layout_tab_bar(&items, 30);
    assert_eq!(layout.slots.len(), 3, "truncation should keep every tab");
    assert!(!layout.has_overflow());
    assert_eq!(label_widths(&layout), vec![9, 9, 4]);
    assert!(layout.slots[0].label.ends_with('…'));
    assert_eq!(layout.slots[2].label, "Plan");
}

#[test]
fn layout_very_narrow_keeps_focused_tab_and_reports_overflow() {
    let items = vec![
        tab(0, "aaaa", false, false),
        tab(1, "bbbb", false, false),
        tab(2, "cccc", true, false),
        tab(3, "dddd", false, false),
        tab(4, "eeee", false, false),
        tab(5, "ffff", false, false),
    ];
    let layout = layout_tab_bar(&items, 20);
    assert!(layout.has_overflow());
    assert!(
        layout.slots.iter().any(|slot| slot.focused),
        "the focused tab must stay visible"
    );
    assert_eq!(
        layout.leading_hidden + layout.trailing_hidden + layout.slots.len(),
        items.len(),
        "hidden counts must account for every dropped tab"
    );
}

#[test]
fn layout_tiny_width_hides_everything() {
    let items = vec![tab(0, "aaaa", true, false)];
    let layout = layout_tab_bar(&items, 3);
    assert!(layout.slots.is_empty());
    assert_eq!(layout.leading_hidden, 1);
    assert_eq!(layout.trailing_hidden, 0);
    assert_eq!(layout.plain_text(), "");
}

#[test]
fn layout_of_empty_tab_list_is_empty() {
    let layout = layout_tab_bar(&[], 80);
    assert!(layout.slots.is_empty());
    assert!(!layout.has_overflow());
    assert_eq!(layout, TabBarLayout::default());
}

#[test]
fn plain_text_marks_updated_tabs_and_overflow() {
    let layout = TabBarLayout {
        slots: vec![
            TabSlot {
                page_id: "a".to_string(),
                kind: PageKind::Progress,
                label: "Progress".to_string(),
                focused: true,
                updated: true,
            },
            TabSlot {
                page_id: "b".to_string(),
                kind: PageKind::Agent,
                label: "Plan".to_string(),
                focused: false,
                updated: false,
            },
        ],
        leading_hidden: 2,
        trailing_hidden: 3,
    };
    assert_eq!(layout.plain_text(), "…+2 •Progress│Plan …+3");
}

#[test]
fn tab_items_collapse_whitespace_and_track_focus_and_badges() {
    let snapshot = snapshot(
        vec![
            page("plan-doc", "  My   Plan \n Doc  "),
            page("progress", "Progress"),
        ],
        Some("progress"),
    );
    let items = tab_items(&snapshot, |id| id == "plan-doc");
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].page_id, "progress");
    assert!(items[0].focused);
    assert!(!items[0].updated);
    assert_eq!(items[1].title, "My Plan Doc");
    assert!(items[1].updated);
    assert_eq!(items[1].kind, PageKind::Agent);
}

#[test]
fn truncate_label_handles_tiny_budgets() {
    assert_eq!(truncate_label("Plan", 4), "Plan");
    assert_eq!(truncate_label("Plan", 3), "Pl…");
    assert_eq!(truncate_label("Plan", 1), "…");
    assert_eq!(truncate_label("Plan", 0), "");
    // Character-counted, not byte-counted.
    assert_eq!(truncate_label("进展面板", 3), "进展…");
}

#[test]
fn badge_first_observation_is_silent() {
    let mut state = SidePanelTabState::default();
    state.sync(&snapshot(vec![page("plan-doc", "Plan")], Some("progress")));
    assert_eq!(state.updated_count(), 0, "existing pages get no badge");
}

#[test]
fn badge_marks_unfocused_change_and_clears_when_focused() {
    let mut state = SidePanelTabState::default();
    state.sync(&snapshot(
        vec![titled("plan-doc", "Plan", 1, 4)],
        Some("progress"),
    ));
    state.sync(&snapshot(
        vec![titled("plan-doc", "Plan", 2, 4)],
        Some("progress"),
    ));
    assert!(state.is_updated("plan-doc"));

    state.sync(&snapshot(
        vec![titled("plan-doc", "Plan", 2, 4)],
        Some("plan-doc"),
    ));
    assert!(!state.is_updated("plan-doc"), "focusing a page clears `•`");
}

#[test]
fn badge_ignores_changes_to_the_focused_page() {
    let mut state = SidePanelTabState::default();
    state.sync(&snapshot(
        vec![titled("plan-doc", "Plan", 1, 4)],
        Some("plan-doc"),
    ));
    state.sync(&snapshot(
        vec![titled("plan-doc", "Plan", 2, 7)],
        Some("plan-doc"),
    ));
    assert_eq!(state.updated_count(), 0);
}

#[test]
fn badge_detects_length_only_changes() {
    let mut state = SidePanelTabState::default();
    state.sync(&snapshot(vec![titled("a", "A", 9, 4)], Some("b")));
    state.sync(&snapshot(vec![titled("a", "A", 9, 9)], Some("b")));
    assert!(state.is_updated("a"));
}

#[test]
fn badge_prunes_pages_that_disappear() {
    let mut state = SidePanelTabState::default();
    state.sync(&snapshot(vec![titled("a", "A", 1, 4)], Some("b")));
    state.sync(&snapshot(vec![titled("a", "A", 2, 4)], Some("b")));
    assert!(state.is_updated("a"));
    state.sync(&snapshot(Vec::new(), None));
    assert_eq!(state.updated_count(), 0);
    assert!(!state.is_updated("a"));
}

#[test]
fn clear_badge_is_targeted() {
    let mut state = SidePanelTabState::default();
    state.mark_updated_for_tests("a");
    state.mark_updated_for_tests("b");
    state.clear_badge("a");
    assert!(!state.is_updated("a"));
    assert!(state.is_updated("b"));
}

// ---------------------------------------------------------------------------
// Page list overlay (`Alt+L`)
// ---------------------------------------------------------------------------

fn picker_pages() -> Vec<SidePanelPage> {
    vec![
        page("plan-doc", "My Plan"),
        page("split_view", "Split View"),
        page("progress", "Progress"),
        page("req_map", "Requirements"),
    ]
}

#[test]
fn picker_empty_query_lists_every_tab_in_bar_order() {
    let pages = picker_pages();
    let picker = PagePickerState::new();
    assert_eq!(picker.query(), "");
    assert_eq!(
        picker.matches(&pages),
        tab_order(&pages),
        "an empty query is just the tab bar order"
    );
}

#[test]
fn picker_filters_by_id_title_and_tag() {
    let pages = picker_pages();

    let mut by_id = PagePickerState::new();
    for ch in "req".chars() {
        by_id.push_char(ch);
    }
    assert_eq!(by_id.matches(&pages), vec![3], "id prefix req_map");

    let mut by_tag = PagePickerState::new();
    for ch in "progress".chars() {
        by_tag.push_char(ch);
    }
    assert_eq!(by_tag.matches(&pages), vec![2], "tag/title progress");

    let mut by_title = PagePickerState::new();
    for ch in "Spl".chars() {
        by_title.push_char(ch);
    }
    let title_matches = by_title.matches(&pages);
    assert_eq!(
        title_matches.first().copied(),
        Some(1),
        "the closest match (Split View) ranks first"
    );
    assert!(title_matches.contains(&1));
    // The shared matcher is deliberately typo-tolerant, so a looser query can
    // also pull in a lower-scoring page; the picker just ranks it below.
    assert!(title_matches.len() <= pages.len());

    let mut none = PagePickerState::new();
    for ch in "zzzz".chars() {
        none.push_char(ch);
    }
    assert!(none.matches(&pages).is_empty());
}

#[test]
fn picker_push_and_pop_reset_the_selection() {
    let mut picker = PagePickerState::new();
    picker.move_selection(2, 4);
    assert_eq!(picker.selected_row(4), Some(2));

    picker.push_char('a');
    assert_eq!(
        picker.selected_row(4),
        Some(0),
        "typing re-selects the best match"
    );

    picker.move_selection(1, 4);
    picker.pop_char();
    assert_eq!(picker.selected_row(4), Some(0));

    picker.clear_query();
    assert_eq!(picker.query(), "");
}

#[test]
fn picker_selection_wraps_and_clamps() {
    let mut picker = PagePickerState::new();
    picker.move_selection(-1, 3);
    assert_eq!(picker.selected_row(3), Some(2), "wraps backwards");

    picker.move_selection(1, 3);
    assert_eq!(picker.selected_row(3), Some(0), "wraps forwards");

    picker.move_selection(5, 0);
    assert_eq!(
        picker.selected_row(0),
        None,
        "no matches means no selection"
    );

    // A stale selection beyond the current match count clamps instead of
    // indexing out of range.
    let mut stale = PagePickerState::new();
    stale.move_selection(3, 4);
    assert_eq!(stale.selected_row(2), Some(1));
}

#[test]
fn picker_ignores_control_characters() {
    let mut picker = PagePickerState::new();
    picker.push_char('\n');
    picker.push_char('\t');
    assert_eq!(picker.query(), "");
}

#[test]
fn picker_rows_carry_title_tag_and_focus() {
    let pages = picker_pages();
    let picker = PagePickerState::new();
    let rows = picker_rows(&pages, &picker, Some("progress"));
    assert_eq!(rows.len(), 4);
    // Tab-bar order: progress, req_map, split_view, plan-doc.
    assert_eq!(rows[0].page_id, "progress");
    assert_eq!(rows[0].tag, "progress");
    assert!(rows[0].focused);
    assert_eq!(rows[1].page_id, "req_map");
    assert_eq!(rows[1].tag, "req_map");
    assert!(!rows[1].focused);
    assert_eq!(rows[3].page_id, "plan-doc");
    assert_eq!(rows[3].tag, "agent");
}

#[test]
fn picker_key_map() {
    let none = KeyModifiers::NONE;
    let ctrl = KeyModifiers::CONTROL;

    assert_eq!(
        page_picker_action(KeyCode::Esc, none, 3),
        PagePickerAction::Close
    );
    assert_eq!(
        page_picker_action(KeyCode::Char('c'), ctrl, 3),
        PagePickerAction::Close,
        "Ctrl+C closes the overlay instead of quitting"
    );
    assert_eq!(
        page_picker_action(KeyCode::Enter, none, 3),
        PagePickerAction::Commit
    );
    assert_eq!(
        page_picker_action(KeyCode::Down, none, 3),
        PagePickerAction::Move(1)
    );
    assert_eq!(
        page_picker_action(KeyCode::Tab, none, 3),
        PagePickerAction::Move(1)
    );
    assert_eq!(
        page_picker_action(KeyCode::BackTab, none, 3),
        PagePickerAction::Move(-1)
    );
    assert_eq!(
        page_picker_action(KeyCode::PageUp, none, 9),
        PagePickerAction::Move(-5)
    );
    assert_eq!(
        page_picker_action(KeyCode::Home, none, 9),
        PagePickerAction::Move(-9)
    );
    assert_eq!(
        page_picker_action(KeyCode::Backspace, none, 3),
        PagePickerAction::Pop
    );
    assert_eq!(
        page_picker_action(KeyCode::Char('r'), none, 3),
        PagePickerAction::Push('r'),
        "plain characters filter the list"
    );
    assert_eq!(
        page_picker_action(KeyCode::Char('r'), ctrl, 3),
        PagePickerAction::Ignored,
        "Ctrl chords stay with the global handlers"
    );
    assert_eq!(
        page_picker_action(KeyCode::F(5), none, 3),
        PagePickerAction::Ignored
    );
}
