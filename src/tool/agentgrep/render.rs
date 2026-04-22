use super::*;

pub(super) fn render_grep_output(result: &GrepResult, args: &GrepArgs) -> String {
    if args.paths_only {
        return result
            .files
            .iter()
            .map(|file| file.path.clone())
            .collect::<Vec<_>>()
            .join("\n");
    }

    let mut lines = vec![
        format!("query: {}", result.query),
        format!(
            "matches: {} in {} files",
            result.total_matches, result.total_files
        ),
    ];

    for file in &result.files {
        render_grep_file(file, &mut lines);
    }

    lines.join("\n")
}

fn render_grep_file(file: &FileMatches, lines: &mut Vec<String>) {
    lines.push(String::new());
    lines.push(file.path.clone());
    if file.total_symbols > 0 {
        lines.push(format!(
            "  symbols: {} total, {} matched, {} other",
            file.total_symbols,
            file.matched_symbol_count,
            file.total_symbols.saturating_sub(file.matched_symbol_count)
        ));
    } else {
        lines.push("  symbols: no structural items detected".to_string());
    }
    for group in &file.groups {
        match (group.start_line, group.end_line) {
            (Some(start_line), Some(end_line)) => lines.push(format!(
                "    - {} {} @ {}-{}",
                group.kind, group.label, start_line, end_line
            )),
            _ => lines.push(format!("    - {}", group.label)),
        }
        for line_match in group.resolved_matches(&file.matches) {
            lines.push(format!(
                "      - @ {} {}",
                line_match.line_number, line_match.line_text
            ));
        }
    }
    if !file.other_symbols.is_empty() {
        let mut summary = file
            .other_symbols
            .iter()
            .map(|item| {
                format!(
                    "{} {} @ {}-{}",
                    item.kind, item.label, item.start_line, item.end_line
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        if file.other_symbols_omitted_count > 0 {
            if !summary.is_empty() {
                summary.push_str("; ");
            }
            summary.push_str(&format!("... {} more", file.other_symbols_omitted_count));
        }
        lines.push(format!("    - other: {summary}"));
    }
}

pub(super) fn render_find_output(result: &FindResult, args: &FindArgs) -> String {
    if args.paths_only {
        return result
            .files
            .iter()
            .map(|file| file.path.clone())
            .collect::<Vec<_>>()
            .join("\n");
    }

    let mut lines = vec![
        format!("query: {}", result.query),
        format!("top files: {}", result.files.len()),
    ];

    for (idx, file) in result.files.iter().enumerate() {
        render_find_file(idx, file, args, &mut lines);
    }

    lines.join("\n")
}

fn render_find_file(idx: usize, file: &FindFile, args: &FindArgs, lines: &mut Vec<String>) {
    lines.push(String::new());
    lines.push(format!("{}. {}", idx + 1, file.path));
    lines.push(format!("   role: {}", file.role));
    lines.push("   why:".to_string());
    for reason in &file.why {
        lines.push(format!("     - {reason}"));
    }
    if args.debug_score {
        lines.push(format!("   score: {}", file.score));
    }
    lines.push("   structure:".to_string());
    for item in &file.structure.items {
        lines.push(format!(
            "     - {} {} @ {}-{} ({} lines)",
            item.kind, item.label, item.start_line, item.end_line, item.line_count
        ));
    }
    if file.structure.omitted_count > 0 {
        lines.push(format!(
            "     ... {} more symbols",
            file.structure.omitted_count
        ));
    }
}

pub(super) fn render_outline_output(result: &OutlineResult) -> String {
    let mut lines = vec![
        format!("file: {}", result.path),
        format!("language: {}", result.language),
        format!("role: {}", result.role),
        format!("lines: {}", result.total_lines),
        format!(
            "symbols: {}",
            result.structure.items.len() + result.structure.omitted_count
        ),
        String::new(),
        "structure:".to_string(),
    ];

    if result.structure.items.is_empty() {
        lines.push("  (no structural items detected)".to_string());
    } else {
        for item in &result.structure.items {
            lines.push(format!(
                "  - {} {} @ {}-{} ({} lines)",
                item.kind, item.label, item.start_line, item.end_line, item.line_count
            ));
        }
        if result.structure.omitted_count > 0 {
            lines.push(format!(
                "  ... {} more symbols",
                result.structure.omitted_count
            ));
        }
    }
    if let Some(note) = &result.context_applied {
        lines.push(String::new());
        lines.push(format!("context: {note}"));
    }

    lines.join("\n")
}

pub(super) fn render_smart_output(result: &SmartResult, args: &SmartArgs) -> String {
    if args.paths_only {
        return result
            .files
            .iter()
            .map(|file| file.path.clone())
            .collect::<Vec<_>>()
            .join("\n");
    }

    let mut lines = Vec::new();
    if args.debug_plan {
        lines.extend(render_debug_plan(result));
        lines.push(String::new());
    }
    lines.push("query parameters:".to_string());
    lines.push(format!("  subject: {}", result.query.subject));
    lines.push(format!("  relation: {}", result.query.relation.as_str()));
    if !result.query.support.is_empty() {
        lines.push(format!("  support: {}", result.query.support.join(", ")));
    }
    if let Some(kind) = &result.query.kind {
        lines.push(format!("  kind: {kind}"));
    }
    if let Some(path_hint) = &result.query.path_hint {
        lines.push(format!("  path_hint: {path_hint}"));
    }
    lines.push(String::new());
    lines.push(format!(
        "top results: {} files, {} regions",
        result.summary.total_files, result.summary.total_regions
    ));
    if result.files.is_empty() {
        lines.push("no results found for the current trace query and scope".to_string());
    }
    if let Some(best_file) = &result.summary.best_file {
        lines.push(format!("best answer likely in {best_file}"));
    }
    for (idx, file) in result.files.iter().enumerate() {
        render_smart_file(idx, file, args, &mut lines);
    }

    lines.join("\n")
}

fn render_debug_plan(result: &SmartResult) -> Vec<String> {
    let relation_terms = match result.query.relation {
        Relation::Rendered => "render, draw, ui, widget, view",
        Relation::CalledFrom => "call, invoke, dispatch",
        Relation::TriggeredFrom => "trigger, dispatch, schedule",
        Relation::Populated => "set, assign, insert, push, build",
        Relation::ComesFrom => "source, load, parse, read, fetch",
        Relation::Handled => "handle, handler, event, dispatch",
        Relation::Defined => "fn, struct, enum, class, def",
        Relation::Implementation => "impl, register, wire, tool",
        _ => result.query.relation.as_str(),
    };
    let mut lines = vec![
        "debug plan:".to_string(),
        "  mode: trace".to_string(),
        format!("  subject: {}", result.query.subject),
        format!("  relation: {}", result.query.relation.as_str()),
        format!("  relation_terms: {relation_terms}"),
    ];
    if let Some(kind) = &result.query.kind {
        lines.push(format!("  kind filter: {kind}"));
    }
    if let Some(path_hint) = &result.query.path_hint {
        lines.push(format!("  path hint: {path_hint}"));
    }
    if !result.query.support.is_empty() {
        lines.push(format!(
            "  support terms: {}",
            result.query.support.join(", ")
        ));
    }
    lines
}

fn render_smart_file(idx: usize, file: &SmartFile, args: &SmartArgs, lines: &mut Vec<String>) {
    lines.push(String::new());
    lines.push(format!("{}. {}", idx + 1, file.path));
    lines.push(format!("   role: {}", file.role));
    lines.push("   why:".to_string());
    for reason in &file.why {
        lines.push(format!("     - {reason}"));
    }
    if args.debug_score {
        lines.push(format!("   score: {}", file.score));
    }
    lines.push("   structure:".to_string());
    for item in &file.structure.items {
        lines.push(format!(
            "     - {} {} @ {}-{} ({} lines)",
            item.kind, item.label, item.start_line, item.end_line, item.line_count
        ));
    }
    if file.structure.omitted_count > 0 {
        lines.push(format!(
            "     ... {} more symbols",
            file.structure.omitted_count
        ));
    }
    if let Some(note) = &file.context_applied {
        lines.push(format!("   context: {note}"));
    }
    lines.push("   regions:".to_string());
    for region in &file.regions {
        render_smart_region(region, args.debug_score, lines);
    }
}

fn render_smart_region(region: &SmartRegion, debug_score: bool, lines: &mut Vec<String>) {
    lines.push(format!(
        "     - {} @ {}-{} ({} lines)",
        region.label, region.start_line, region.end_line, region.line_count
    ));
    lines.push(format!("       kind: {}", region.kind));
    if debug_score {
        lines.push(format!("       score: {}", region.score));
    }
    if region.full_region {
        lines.push("       full region:".to_string());
    } else {
        lines.push("       snippet:".to_string());
    }
    for line in region.body.lines() {
        lines.push(format!("         {line}"));
    }
    lines.push("       why:".to_string());
    for reason in &region.why {
        lines.push(format!("         - {reason}"));
    }
    if let Some(note) = &region.context_applied {
        lines.push(format!("       context: {note}"));
    }
}
