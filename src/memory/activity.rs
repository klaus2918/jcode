use super::*;

/// Global memory activity state - updated by sidecar, read by info widget
static MEMORY_ACTIVITY: Mutex<Option<MemoryActivity>> = Mutex::new(None);

/// Maximum number of recent events to keep
const MAX_RECENT_EVENTS: usize = 10;

/// Staleness timeout: auto-reset to Idle if state has been non-Idle for this long
const STALENESS_TIMEOUT_SECS: u64 = 10;

/// Get current memory activity state
pub fn get_activity() -> Option<MemoryActivity> {
    MEMORY_ACTIVITY.lock().ok().and_then(|guard| guard.clone())
}

/// Update the memory activity state
pub fn set_state(state: MemoryState) {
    if let Ok(mut guard) = MEMORY_ACTIVITY.lock() {
        if let Some(activity) = guard.as_mut() {
            activity.state = state;
            activity.state_since = Instant::now();
        } else {
            *guard = Some(MemoryActivity {
                state,
                state_since: Instant::now(),
                pipeline: None,
                recent_events: Vec::new(),
            });
        }
    }
}

/// Add an event to the activity log
pub fn add_event(kind: MemoryEventKind) {
    crate::memory_log::log_event(&kind);

    if let Ok(mut guard) = MEMORY_ACTIVITY.lock() {
        let event = MemoryEvent {
            kind,
            timestamp: Instant::now(),
            detail: None,
        };

        if let Some(activity) = guard.as_mut() {
            activity.recent_events.insert(0, event);
            activity.recent_events.truncate(MAX_RECENT_EVENTS);
        } else {
            *guard = Some(MemoryActivity {
                state: MemoryState::Idle,
                state_since: Instant::now(),
                pipeline: None,
                recent_events: vec![event],
            });
        }
    }
}

/// Start a new pipeline run (called at the beginning of each memory check)
pub fn pipeline_start() {
    use crate::tui::info_widget::PipelineState;
    if let Ok(mut guard) = MEMORY_ACTIVITY.lock() {
        if let Some(activity) = guard.as_mut() {
            activity.pipeline = Some(PipelineState::new());
        } else {
            *guard = Some(MemoryActivity {
                state: MemoryState::Idle,
                state_since: Instant::now(),
                pipeline: Some(PipelineState::new()),
                recent_events: Vec::new(),
            });
        }
    }
}

/// Update pipeline step status
pub fn pipeline_update(f: impl FnOnce(&mut crate::tui::info_widget::PipelineState)) {
    if let Ok(mut guard) = MEMORY_ACTIVITY.lock() {
        if let Some(activity) = guard.as_mut() {
            if let Some(pipeline) = activity.pipeline.as_mut() {
                f(pipeline);
            }
        }
    }
}

/// Check for staleness and auto-reset if needed.
/// Returns true if state was reset due to staleness.
pub fn check_staleness() -> bool {
    if let Ok(mut guard) = MEMORY_ACTIVITY.lock() {
        if let Some(activity) = guard.as_mut() {
            if !matches!(activity.state, MemoryState::Idle)
                && activity.state_since.elapsed().as_secs() >= STALENESS_TIMEOUT_SECS
            {
                crate::logging::info(&format!(
                    "Memory state stale ({:?} for {}s), auto-resetting to Idle",
                    activity.state,
                    activity.state_since.elapsed().as_secs()
                ));
                activity.state = MemoryState::Idle;
                activity.state_since = Instant::now();
                return true;
            }
        }
    }
    false
}

/// Clear activity (reset to idle with no events)
pub fn clear_activity() {
    if let Ok(mut guard) = MEMORY_ACTIVITY.lock() {
        *guard = None;
    }
}

/// Record that a memory payload was injected into model context.
/// This feeds the memory info widget with injected content + metadata.
pub fn record_injected_prompt(prompt: &str, count: usize, age_ms: u64) {
    crate::telemetry::record_memory_injected(count, age_ms);
    let items = parse_injected_items(prompt, 8);
    let preview = prompt_preview(prompt, 72);
    add_event(MemoryEventKind::MemoryInjected {
        count,
        prompt_chars: prompt.chars().count(),
        age_ms,
        preview: preview.clone(),
        items,
    });
    add_event(MemoryEventKind::MemorySurfaced {
        memory_preview: preview,
    });
}

fn parse_injected_items(prompt: &str, max_items: usize) -> Vec<InjectedMemoryItem> {
    let mut items: Vec<InjectedMemoryItem> = Vec::new();
    let mut section = String::from("Memory");

    for raw_line in prompt.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line == "# Memory" {
            continue;
        }
        if let Some(header) = line.strip_prefix("## ") {
            let header = header.trim();
            if !header.is_empty() {
                section = header.to_string();
            }
            continue;
        }

        let content = if let Some(rest) = line.strip_prefix("- ") {
            Some(rest.trim())
        } else if let Some((prefix, rest)) = line.split_once(". ") {
            if !prefix.is_empty() && prefix.chars().all(|c| c.is_ascii_digit()) {
                Some(rest.trim())
            } else {
                None
            }
        } else {
            None
        };

        if let Some(content) = content {
            if content.is_empty() {
                continue;
            }
            items.push(InjectedMemoryItem {
                section: section.clone(),
                content: content.to_string(),
            });
            if items.len() >= max_items {
                return items;
            }
        }
    }

    if items.is_empty() {
        let fallback = prompt
            .lines()
            .map(str::trim)
            .filter(|line| {
                !line.is_empty()
                    && !line.starts_with('#')
                    && !line.starts_with("## ")
                    && !line.starts_with("- ")
            })
            .collect::<Vec<_>>()
            .join(" ");
        if !fallback.is_empty() {
            items.push(InjectedMemoryItem {
                section,
                content: fallback,
            });
        }
    }

    items
}

fn prompt_preview(prompt: &str, max_chars: usize) -> String {
    let bullet = prompt
        .lines()
        .map(str::trim)
        .find_map(|line| {
            if line.starts_with("- ") {
                Some(line.trim_start_matches("- ").trim())
            } else if let Some((prefix, rest)) = line.split_once(". ") {
                if !prefix.is_empty() && prefix.chars().all(|c| c.is_ascii_digit()) {
                    Some(rest.trim())
                } else {
                    None
                }
            } else {
                None
            }
        })
        .unwrap_or_else(|| prompt.trim());

    if bullet.chars().count() <= max_chars {
        bullet.to_string()
    } else {
        let mut out = String::new();
        for (i, ch) in bullet.chars().enumerate() {
            if i >= max_chars.saturating_sub(3) {
                break;
            }
            out.push(ch);
        }
        out.push_str("...");
        out
    }
}
