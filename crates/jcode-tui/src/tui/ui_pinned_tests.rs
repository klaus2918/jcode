use super::*;

fn clear_side_panel_render_caches() {
    super::clear_side_panel_render_caches();
}

fn render_side_panel_markdown_wraps_long_text_lines() {
    let page = crate::side_panel::SidePanelPage {
            id: "wrap_demo".to_string(),
            title: "Wrap Demo".to_string(),
            file_path: "wrap_demo.md".to_string(),
            format: crate::side_panel::SidePanelPageFormat::Markdown,
            source: crate::side_panel::SidePanelPageSource::Managed,
            content: "This is a deliberately long side panel line that should wrap instead of overflowing the pane.".to_string(),
            updated_at_ms: 1,
        };

    let rendered = render_side_panel_markdown_cached(&page, Rect::new(0, 0, 18, 30), false);

    let non_empty: Vec<&Line<'_>> = rendered
        .lines
        .iter()
        .filter(|line| line.width() > 0)
        .collect();

    assert!(
        non_empty.len() >= 2,
        "expected long side panel text to wrap: {:?}",
        rendered.lines
    );
    assert!(
        non_empty.iter().all(|line| line.width() <= 18),
        "expected wrapped side panel lines to fit width 18: {:?}",
        rendered.lines
    );
}

#[test]
fn render_side_panel_markdown_keeps_table_rows_intact() {
    let page = crate::side_panel::SidePanelPage {
        id: "table_demo".to_string(),
        title: "Table Demo".to_string(),
        file_path: "table_demo.md".to_string(),
        format: crate::side_panel::SidePanelPageFormat::Markdown,
        source: crate::side_panel::SidePanelPageSource::Managed,
        content:
            "| # | Principle | Story Ready |\n| - | - | - |\n| 1 | Customer Obsession | unchecked |"
                .to_string(),
        updated_at_ms: 1,
    };

    let rendered = render_side_panel_markdown_cached(&page, Rect::new(0, 0, 24, 20), false);
    let text: Vec<String> = rendered
        .lines
        .iter()
        .map(|line| line.spans.iter().map(|s| s.content.as_ref()).collect())
        .collect();

    assert!(
        text.iter().any(|line| line.contains("─┼─")),
        "expected separator line to remain intact: {:?}",
        text
    );
    assert!(
        text.iter()
            .any(|line| line.matches('│').count() == 2 && line.contains("Cust")),
        "expected a single intact table row line: {:?}",
        text
    );
}

#[test]
fn render_side_panel_markdown_live_syncs_file_content() {
    let temp = tempfile::tempdir().expect("tempdir");
    let file_path = temp.path().join("live.md");
    std::fs::write(&file_path, "# First").expect("write initial content");

    let mut snapshot = crate::side_panel::SidePanelSnapshot {
        focused_page_id: Some("live_demo".to_string()),
        pages: vec![crate::side_panel::SidePanelPage {
            id: "live_demo".to_string(),
            title: "Live Demo".to_string(),
            file_path: file_path.display().to_string(),
            format: crate::side_panel::SidePanelPageFormat::Markdown,
            source: crate::side_panel::SidePanelPageSource::LinkedFile,
            content: "# Stale".to_string(),
            updated_at_ms: 1,
        }],
    };

    clear_side_panel_render_caches();
    assert!(crate::side_panel::refresh_linked_page_content(
        &mut snapshot,
        None
    ));
    let page = snapshot.focused_page().expect("focused page");

    let first = render_side_panel_markdown_cached(&page, Rect::new(0, 0, 24, 20), false);
    let first_text: Vec<String> = first
        .lines
        .iter()
        .map(|line| line.spans.iter().map(|s| s.content.as_ref()).collect())
        .collect();
    assert!(
        first_text.iter().any(|line| line.contains("First")),
        "expected first render to use file content: {:?}",
        first_text
    );

    std::fs::write(&file_path, "# Second").expect("write updated content");

    assert!(crate::side_panel::refresh_linked_page_content(
        &mut snapshot,
        None
    ));
    let page = snapshot.focused_page().expect("focused page");

    let second = render_side_panel_markdown_cached(&page, Rect::new(0, 0, 24, 20), false);
    let second_text: Vec<String> = second
        .lines
        .iter()
        .map(|line| line.spans.iter().map(|s| s.content.as_ref()).collect())
        .collect();
    assert!(
        second_text.iter().any(|line| line.contains("Second")),
        "expected second render to reflect updated file content: {:?}",
        second_text
    );
}

#[test]
fn render_side_panel_height_change_reuses_markdown_render_cache() {
    clear_side_panel_render_caches();
    // Use the thread-local render counter: the process-global
    // debug_stats().total_renders races markdown renders on other test
    // threads, making "no extra render" assertions order-dependent.
    let before = markdown::thread_render_count();
    let page = crate::side_panel::SidePanelPage {
        id: "height_cache_demo".to_string(),
        title: "Height Cache Demo".to_string(),
        file_path: "height_cache_demo.md".to_string(),
        format: crate::side_panel::SidePanelPageFormat::Markdown,
        source: crate::side_panel::SidePanelPageSource::Managed,
        content: "# Demo\n\nThis side panel should only parse markdown once for a stable width."
            .to_string(),
        updated_at_ms: 9,
    };

    let _first = render_side_panel_markdown_cached(&page, Rect::new(0, 0, 28, 18), false);
    let after_first = markdown::thread_render_count();
    let _second = render_side_panel_markdown_cached(&page, Rect::new(0, 0, 28, 26), false);
    let after_second = markdown::thread_render_count();

    assert!(
        after_first > before,
        "expected initial render to parse markdown"
    );
    assert_eq!(
        after_second, after_first,
        "height-only cache miss should not trigger another markdown render"
    );
}

#[test]
fn render_side_panel_content_change_with_same_revision_invalidates_cache() {
    clear_side_panel_render_caches();

    let first_page = crate::side_panel::SidePanelPage {
        id: "cache_invalidation_demo".to_string(),
        title: "Cache Invalidation Demo".to_string(),
        file_path: "cache_invalidation_demo.md".to_string(),
        format: crate::side_panel::SidePanelPageFormat::Markdown,
        source: crate::side_panel::SidePanelPageSource::Managed,
        content: "# First version".to_string(),
        updated_at_ms: 1,
    };
    let second_page = crate::side_panel::SidePanelPage {
        content: "# Second version".to_string(),
        ..first_page.clone()
    };

    let first = render_side_panel_markdown_cached(&first_page, Rect::new(0, 0, 28, 12), false);
    let second = render_side_panel_markdown_cached(&second_page, Rect::new(0, 0, 28, 12), false);

    let first_text: Vec<String> = first
        .lines
        .iter()
        .map(|line| line.spans.iter().map(|s| s.content.as_ref()).collect())
        .collect();
    let second_text: Vec<String> = second
        .lines
        .iter()
        .map(|line| line.spans.iter().map(|s| s.content.as_ref()).collect())
        .collect();

    assert!(
        first_text.iter().any(|line| line.contains("First version")),
        "expected first render to contain the original content: {:?}",
        first_text
    );
    assert!(
        second_text
            .iter()
            .any(|line| line.contains("Second version")),
        "expected second render to invalidate the stale cache entry: {:?}",
        second_text
    );
}

#[test]
fn prewarm_focused_side_panel_reuses_markdown_cache_on_first_draw() {
    clear_side_panel_render_caches();
    // Thread-local counter: see render_side_panel_height_change test.
    let before = markdown::thread_render_count();
    let snapshot = crate::side_panel::SidePanelSnapshot {
        focused_page_id: Some("prewarm_demo".to_string()),
        pages: vec![crate::side_panel::SidePanelPage {
            id: "prewarm_demo".to_string(),
            title: "Prewarm Demo".to_string(),
            file_path: "prewarm_demo.md".to_string(),
            format: crate::side_panel::SidePanelPageFormat::Markdown,
            source: crate::side_panel::SidePanelPageSource::Managed,
            content: "# Demo\n\nThis should be warm before first draw.".to_string(),
            updated_at_ms: 7,
        }],
    };

    assert!(prewarm_focused_side_panel(&snapshot, 120, 40, 40, false));
    let after_prewarm = markdown::thread_render_count();
    let page = snapshot.focused_page().expect("focused page");
    let pane_area = estimate_side_panel_pane_area(120, 40, 40).expect("side panel area");
    let inner = side_panel_content_area(pane_area).expect("side panel content area");
    let _ = render_side_panel_markdown_cached(&page, inner, false);
    let after_draw = markdown::thread_render_count();

    assert!(
        after_prewarm > before,
        "expected prewarm to render markdown once"
    );
    assert_eq!(
        after_draw, after_prewarm,
        "expected first draw to reuse prewarmed markdown cache"
    );
}

#[test]
fn render_side_panel_managed_pages_ignore_disk_file_content() {
    let temp = tempfile::tempdir().expect("tempdir");
    let file_path = temp.path().join("managed.md");
    std::fs::write(&file_path, "# Disk Version").expect("write disk content");

    let page = crate::side_panel::SidePanelPage {
        id: "managed_demo".to_string(),
        title: "Managed Demo".to_string(),
        file_path: file_path.display().to_string(),
        format: crate::side_panel::SidePanelPageFormat::Markdown,
        source: crate::side_panel::SidePanelPageSource::Managed,
        content: "# In Memory".to_string(),
        updated_at_ms: 42,
    };

    let rendered = render_side_panel_markdown_cached(&page, Rect::new(0, 0, 24, 20), false);
    let text: Vec<String> = rendered
        .lines
        .iter()
        .map(|line| line.spans.iter().map(|s| s.content.as_ref()).collect())
        .collect();

    assert!(
        text.iter().any(|line| line.contains("In Memory")),
        "expected managed side panel to render snapshot content: {:?}",
        text
    );
    assert!(
        !text.iter().any(|line| line.contains("Disk Version")),
        "managed side panel should not re-read disk content: {:?}",
        text
    );
}

#[test]
fn render_side_panel_linked_file_missing_file_falls_back_to_snapshot_content() {
    let temp = tempfile::tempdir().expect("tempdir");
    let file_path = temp.path().join("linked.md");

    let page = crate::side_panel::SidePanelPage {
        id: "linked_missing_demo".to_string(),
        title: "Linked Missing Demo".to_string(),
        file_path: file_path.display().to_string(),
        format: crate::side_panel::SidePanelPageFormat::Markdown,
        source: crate::side_panel::SidePanelPageSource::LinkedFile,
        content: "# Snapshot Fallback".to_string(),
        updated_at_ms: 7,
    };

    let rendered = render_side_panel_markdown_cached(&page, Rect::new(0, 0, 24, 20), false);
    let text: Vec<String> = rendered
        .lines
        .iter()
        .map(|line| line.spans.iter().map(|s| s.content.as_ref()).collect())
        .collect();

    assert!(
        text.iter().any(|line| line.contains("Snapshot Fallback")),
        "expected linked side panel to fall back to snapshot content when file is missing: {:?}",
        text
    );
}
