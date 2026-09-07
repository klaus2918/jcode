pub use jcode_tui_markdown::{
    CopyTargetKind, IncrementalMarkdownRenderer, MarkdownDebugStats, MarkdownMemoryProfile,
    RawCopyTarget, center_code_blocks, debug_memory_profile, debug_stats, debug_stats_json,
    encode_handterm_latex_apc, extract_copy_targets_from_rendered_lines,
    handterm_native_latex_for_hash, highlight_file_lines, highlight_line, progress_bar,
    progress_line, recenter_structured_blocks_for_display, render_markdown, render_markdown_lazy,
    render_markdown_with_width, render_table_with_width, reset_debug_stats, set_center_code_blocks,
    thread_render_count, wrap_line, wrap_lines,
};

fn to_markdown_spacing_mode(
    mode: crate::config::MarkdownSpacingMode,
) -> jcode_tui_markdown::MarkdownSpacingMode {
    match mode {
        crate::config::MarkdownSpacingMode::Compact => {
            jcode_tui_markdown::MarkdownSpacingMode::Compact
        }
        crate::config::MarkdownSpacingMode::Document => {
            jcode_tui_markdown::MarkdownSpacingMode::Document
        }
    }
}

fn to_markdown_latex_mode(
    mode: crate::config::LatexRenderingMode,
) -> jcode_tui_markdown::LatexRenderingMode {
    match mode {
        crate::config::LatexRenderingMode::None => jcode_tui_markdown::LatexRenderingMode::None,
        crate::config::LatexRenderingMode::Unicode => {
            jcode_tui_markdown::LatexRenderingMode::Unicode
        }
        crate::config::LatexRenderingMode::Image => jcode_tui_markdown::LatexRenderingMode::Image,
    }
}

pub fn install_jcode_markdown_hooks() {
    jcode_tui_markdown::set_latex_log_hook(|error| {
        crate::logging::warn(&format!(
            "LaTeX image rendering fell back to Unicode: {error}"
        ));
    });
    jcode_tui_markdown::set_config_snapshot_hook(|| {
        let cfg = crate::config::config();
        jcode_tui_markdown::MarkdownConfigSnapshot {
            diagram_mode: jcode_tui_markdown::DiagramDisplayMode::None,
            markdown_spacing: to_markdown_spacing_mode(cfg.display.markdown_spacing),
            mermaid_enabled: false,
            latex_rendering: to_markdown_latex_mode(cfg.display.latex_rendering),
        }
    });
    jcode_tui_markdown::set_memory_snapshot_hook(|| {
        let snapshot = crate::process_memory::snapshot_with_source("client:markdown:memory");
        jcode_tui_markdown::ProcessMemorySnapshot {
            rss_bytes: snapshot.rss_bytes,
            peak_rss_bytes: snapshot.peak_rss_bytes,
            virtual_bytes: snapshot.virtual_bytes,
        }
    });
}
