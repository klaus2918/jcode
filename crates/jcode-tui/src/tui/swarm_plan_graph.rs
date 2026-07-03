//! Mermaid source generation for the swarm plan graph.
//!
//! When a swarm plan is created or updated, the TUI renders a flowchart of the
//! plan's task DAG (dependencies from `blocked_by`) through the normal mermaid
//! pipeline, so it appears in the pinned diagram pane or margin widget like
//! any transcript diagram. This module only builds the mermaid source; the
//! caller decides when to render/register it.

use crate::plan::PlanItem;

/// Max tasks drawn before the graph is truncated with a summary node.
/// Beyond this the diagram stops being readable at terminal cell sizes.
const MAX_GRAPH_NODES: usize = 30;
/// Max characters of task content shown per node label.
const MAX_LABEL_CHARS: usize = 42;

/// Build mermaid flowchart source for a swarm plan, or `None` when the plan
/// is empty. Node styling encodes status (done/active/failed/blocked/pending)
/// and edges follow `blocked_by` dependencies.
pub(crate) fn swarm_plan_mermaid(items: &[PlanItem]) -> Option<String> {
    if items.is_empty() {
        return None;
    }
    let shown = &items[..items.len().min(MAX_GRAPH_NODES)];
    let mut out = String::from("flowchart TD\n");

    for item in shown {
        let id = node_id(&item.id);
        let label = node_label(item);
        let class = status_class(&item.status);
        out.push_str(&format!("    {id}[\"{label}\"]:::{class}\n"));
    }

    // Dependency edges, only between nodes that are actually drawn.
    for item in shown {
        let to = node_id(&item.id);
        for dep in &item.blocked_by {
            if shown.iter().any(|other| &other.id == dep) {
                let from = node_id(dep);
                out.push_str(&format!("    {from} --> {to}\n"));
            }
        }
    }

    let hidden = items.len().saturating_sub(shown.len());
    if hidden > 0 {
        out.push_str(&format!(
            "    more[\"…and {hidden} more tasks\"]:::pending\n"
        ));
    }

    // Palette mirrors the swarm gallery status accents.
    out.push_str("    classDef done fill:#1d3a1d,stroke:#64c864,color:#a8e0a8\n");
    out.push_str("    classDef active fill:#3a321d,stroke:#ffc864,color:#ffe0a8\n");
    out.push_str("    classDef failed fill:#3a1d1d,stroke:#ff6464,color:#ffa8a8\n");
    out.push_str("    classDef blocked fill:#3a2a1d,stroke:#ffaa50,color:#ffd0a0\n");
    out.push_str("    classDef pending fill:#26262e,stroke:#8c8c96,color:#b4b4be\n");
    Some(out)
}

/// A mermaid-safe node id derived from a plan item id.
fn node_id(raw: &str) -> String {
    let mut id: String = raw
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if id.is_empty() {
        id.push('x');
    }
    // Mermaid ids must not start with a digit for some directives; prefix
    // uniformly so ids stay predictable.
    format!("t_{id}")
}

/// Node label: status glyph + truncated content + optional assignee.
fn node_label(item: &PlanItem) -> String {
    let glyph = match normalized_status(&item.status) {
        "done" => "✓",
        "active" => "▶",
        "failed" => "✗",
        "blocked" => "⏸",
        _ => "·",
    };
    let mut content = sanitize_label(&item.content);
    if content.chars().count() > MAX_LABEL_CHARS {
        content = content.chars().take(MAX_LABEL_CHARS - 1).collect();
        content.push('…');
    }
    // Keep labels single-line plain text: HTML-ish line breaks (<br/>) are
    // not reliably supported by the Rust mermaid renderer's SVG output.
    match &item.assigned_to {
        Some(who) if !who.is_empty() => {
            format!("{glyph} {content} · @{}", sanitize_label(who))
        }
        _ => format!("{glyph} {content}"),
    }
}

/// Collapse the scheduler's status vocabulary onto graph style classes.
fn normalized_status(status: &str) -> &'static str {
    match status {
        "completed" | "done" => "done",
        "running" | "running_stale" | "in_progress" | "active" => "active",
        "failed" | "cancelled" | "crashed" => "failed",
        "blocked" => "blocked",
        _ => "pending",
    }
}

fn status_class(status: &str) -> &'static str {
    normalized_status(status)
}

/// Strip characters that would break out of a mermaid quoted label.
fn sanitize_label(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '"' => '\'',
            '\n' | '\r' | '\t' => ' ',
            '[' | ']' | '{' | '}' => '(',
            _ => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, content: &str, status: &str, blocked_by: &[&str]) -> PlanItem {
        PlanItem {
            content: content.to_string(),
            status: status.to_string(),
            priority: "normal".to_string(),
            id: id.to_string(),
            subsystem: None,
            file_scope: Vec::new(),
            blocked_by: blocked_by.iter().map(|s| s.to_string()).collect(),
            assigned_to: None,
        }
    }

    #[test]
    fn empty_plan_yields_no_graph() {
        assert!(swarm_plan_mermaid(&[]).is_none());
    }

    #[test]
    fn graph_has_nodes_edges_and_status_classes() {
        let mut assigned = item("b-2", "carve the gallery band", "running", &["a-1"]);
        assigned.assigned_to = Some("worker-fox".to_string());
        let items = vec![
            item("a-1", "wire the bus tap", "completed", &[]),
            assigned,
            item("c-3", "run the ui tests", "pending", &["b-2"]),
        ];
        let graph = swarm_plan_mermaid(&items).expect("graph");
        assert!(graph.starts_with("flowchart TD"), "got: {graph}");
        assert!(
            graph.contains("t_a_1[\"✓ wire the bus tap\"]:::done"),
            "got: {graph}"
        );
        assert!(graph.contains(":::active"), "got: {graph}");
        assert!(graph.contains("@worker-fox"), "got: {graph}");
        assert!(
            !graph.contains("<br"),
            "labels must stay single-line: {graph}"
        );
        assert!(graph.contains("t_a_1 --> t_b_2"), "got: {graph}");
        assert!(graph.contains("t_b_2 --> t_c_3"), "got: {graph}");
        assert!(graph.contains("classDef done"), "got: {graph}");
    }

    #[test]
    fn labels_are_sanitized_and_truncated() {
        let items = vec![item(
            "x!y",
            "a \"quoted\" [bracketed]\nmultiline label that is much longer than the cap allows here",
            "weird-status",
            &["missing-dep"],
        )];
        let graph = swarm_plan_mermaid(&items).expect("graph");
        // Quotes/brackets/newlines neutralized, unknown status -> pending.
        assert!(
            graph.contains("t_x_y[\"· a 'quoted' (bracketed( multiline"),
            "got: {graph}"
        );
        assert!(graph.contains(":::pending"), "got: {graph}");
        assert!(graph.contains('…'), "expected truncation: {graph}");
        // Edge to an undrawn/missing dependency is dropped.
        assert!(!graph.contains("-->"), "got: {graph}");
    }

    #[test]
    fn oversized_plans_truncate_with_summary_node() {
        let items: Vec<PlanItem> = (0..40)
            .map(|i| item(&format!("t{i}"), &format!("task {i}"), "pending", &[]))
            .collect();
        let graph = swarm_plan_mermaid(&items).expect("graph");
        assert!(graph.contains("…and 10 more tasks"), "got: {graph}");
        assert!(!graph.contains("task 35"), "got: {graph}");
    }
}
