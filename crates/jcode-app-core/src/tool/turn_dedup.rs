//! #12 工具回合效率：同回合重复只读调用的观测与可选去重
//! （变更 `agent-model-call-optimization` · P2）。
//!
//! 严格限界（方案 §P2「工具回合效率」）：
//! - **同回合**：`begin_turn` 清空该会话缓存，不跨回合复用；
//! - **同工具同参数**：按 (canonical 工具名, 参数指纹) 命中；
//! - **写后失效**：任何非只读调用（写/执行/未知）立即清空该会话缓存，
//!   保证「写后读新」不受影响；
//! - **只读分类保守**：内置名单 + 配置覆盖；未知工具一律视为非只读；
//! - **默认仅观测**：控制关闭时只统计重复率与潜在节省，不返回缓存结果、
//!   不改变任何行为；启用后才复用同回合结果；
//! - **保护性限幅**：条目数受 `max_entries_per_turn` 约束，超限整表清空；
//! - **fail-open**：锁失败/异常一律按「照常执行」处理，绝不影响调用链。

use crate::config::ToolDedupControl;
use crate::tool::{ToolContext, ToolExecutionMode, ToolOutput};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

/// 内置只读工具（canonical 名）：同回合内以相同参数重复调用时结果可安全复用。
/// **不含** `todo` / `skill_manage` / `batch` / `write` / `edit` / `bash` 等
/// 可能产生副作用或执行动作的工具。
const DEFAULT_READONLY_TOOLS: &[&str] = &[
    "read",
    "agentgrep",
    "glob",
    "list",
    "ls",
    "list_files",
    "search",
    "search_files",
    "tree",
    "stat",
    "websearch",
    "webfetch",
    "fetch",
];

/// 重复只读调用的观测统计。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TurnDedupStats {
    /// 同回合重复只读调用次数。
    pub duplicates: u64,
    /// 若复用结果可节省的字符数（观测口径；仅记录已知输出长度）。
    pub potential_saved_chars: u64,
}

#[derive(Default)]
struct SessionTurnCache {
    /// (canonical 工具名, 参数指纹) → 输出。
    /// 仅观测模式下值为 `None`（只留指纹，不克隆输出）。
    entries: HashMap<(String, u64), Option<ToolOutput>>,
    stats: TurnDedupStats,
}

static TURN_CACHES: LazyLock<Mutex<HashMap<String, SessionTurnCache>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// 去重裁决。
#[derive(Debug)]
pub enum DedupDecision {
    /// 照常执行（观测模式命中、未命中、非只读、非回合内调用均走这里）。
    Proceed,
    /// 复用同回合相同只读调用的结果（仅控制启用时返回）。
    Cached(Box<ToolOutput>),
}

/// 参数指纹（FNV-1a 64）；`serde_json::Value` 的对象按键有序输出，
/// 因此等价的参数（键序不同）得到相同指纹。
pub fn fingerprint(input: &serde_json::Value) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in input.to_string().bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// 是否为可复用的只读工具。配置给了名单则以配置为准；未知工具返回 false。
pub fn is_readonly(cfg: &ToolDedupControl, resolved: &str) -> bool {
    if !cfg.readonly_tools.is_empty() {
        return cfg.readonly_tools.iter().any(|configured| {
            let canonical = jcode_tool_types::resolve_tool_name(configured.trim());
            canonical.eq_ignore_ascii_case(resolved)
        });
    }
    DEFAULT_READONLY_TOOLS.contains(&resolved)
}

/// 回合开始：清空该会话缓存（不跨回合）。
pub fn begin_turn(session_id: &str) {
    if let Ok(mut caches) = TURN_CACHES.lock() {
        caches.remove(session_id);
    }
}

/// 回合结束：清空缓存并返回本回合统计（供日志 / 账本观察）。
pub fn end_turn(session_id: &str) -> TurnDedupStats {
    let mut stats = TurnDedupStats::default();
    if let Ok(mut caches) = TURN_CACHES.lock()
        && let Some(cache) = caches.remove(session_id)
    {
        stats = cache.stats;
    }
    stats
}

/// 只读调用前裁决（读取当前配置；非回合内调用直接放行）。
pub fn before_call(ctx: &ToolContext, resolved: &str, input: &serde_json::Value) -> DedupDecision {
    if !matches!(ctx.execution_mode, ToolExecutionMode::AgentTurn) {
        return DedupDecision::Proceed;
    }
    let cfg = crate::call_control::control_config();
    let enabled =
        crate::call_control::is_enabled(&cfg, crate::call_control::ControlName::ToolDedup);
    before_call_with(&cfg.tool_dedup, enabled, &ctx.session_id, resolved, input)
}

/// 与 [`before_call`] 相同，但使用显式配置（便于单测）。
pub fn before_call_with(
    cfg: &ToolDedupControl,
    enabled: bool,
    session_id: &str,
    resolved: &str,
    input: &serde_json::Value,
) -> DedupDecision {
    if !is_readonly(cfg, resolved) {
        // 写 / 执行 / 未知工具：立即失效缓存，保证「写后读新」。
        invalidate(session_id);
        return DedupDecision::Proceed;
    }

    let key = (resolved.to_string(), fingerprint(input));
    let cap = cfg.max_entries_per_turn.max(1);
    let mut hit: Option<Option<ToolOutput>> = None;
    let mut stats = TurnDedupStats::default();
    if let Ok(mut caches) = TURN_CACHES.lock() {
        let cache = caches.entry(session_id.to_string()).or_default();
        if cache.entries.len() >= cap && !cache.entries.contains_key(&key) {
            // 保护性限幅：超限整表清空（严格限界优于淘汰策略的复杂度）。
            cache.entries.clear();
        }
        match cache.entries.get(&key) {
            Some(existing) => {
                cache.stats.duplicates = cache.stats.duplicates.saturating_add(1);
                let saved = existing
                    .as_ref()
                    .map(|output| output.output.chars().count() as u64)
                    .unwrap_or(0);
                cache.stats.potential_saved_chars =
                    cache.stats.potential_saved_chars.saturating_add(saved);
                stats = cache.stats;
                hit = Some(existing.clone());
            }
            None => {
                cache.entries.insert(key, None);
            }
        }
    }

    let Some(existing) = hit else {
        return DedupDecision::Proceed;
    };

    crate::logging::info(&format!(
        "TOOL_TURN_DEDUP duplicate_readonly_call tool={resolved} mode={} duplicates={} potential_saved_chars={}",
        if enabled { "dedup" } else { "observe" },
        stats.duplicates,
        stats.potential_saved_chars
    ));
    let action = if enabled {
        crate::call_control::ControlAction::Hit
    } else {
        crate::call_control::ControlAction::Observe
    };
    crate::call_control::control_event(
        crate::call_control::ControlName::ToolDedup,
        action,
        format!(
            "tool={resolved} duplicates={} potential_saved_chars={}",
            stats.duplicates, stats.potential_saved_chars
        ),
    );

    if enabled && let Some(output) = existing {
        return DedupDecision::Cached(Box::new(output));
    }
    DedupDecision::Proceed
}

/// 只读调用成功后的记录（仅控制启用时保存输出；观测模式只保留指纹）。
pub fn after_call(
    ctx: &ToolContext,
    resolved: &str,
    input: &serde_json::Value,
    output: &ToolOutput,
) {
    if !matches!(ctx.execution_mode, ToolExecutionMode::AgentTurn) {
        return;
    }
    let cfg = crate::call_control::control_config();
    if !crate::call_control::is_enabled(&cfg, crate::call_control::ControlName::ToolDedup) {
        return;
    }
    after_call_with(&cfg.tool_dedup, &ctx.session_id, resolved, input, output);
}

/// 与 [`after_call`] 相同，但使用显式配置（便于单测）。
pub fn after_call_with(
    cfg: &ToolDedupControl,
    session_id: &str,
    resolved: &str,
    input: &serde_json::Value,
    output: &ToolOutput,
) {
    if !is_readonly(cfg, resolved) {
        return;
    }
    let key = (resolved.to_string(), fingerprint(input));
    let cap = cfg.max_entries_per_turn.max(1);
    if let Ok(mut caches) = TURN_CACHES.lock() {
        let cache = caches.entry(session_id.to_string()).or_default();
        if let Some(slot) = cache.entries.get_mut(&key) {
            *slot = Some(output.clone());
        } else if cache.entries.len() < cap {
            cache.entries.insert(key, Some(output.clone()));
        }
    }
}

/// 当前会话的统计（诊断用；测试与内部诊断使用）。
#[cfg(test)]
pub fn stats(session_id: &str) -> TurnDedupStats {
    match TURN_CACHES.lock() {
        Ok(caches) => match caches.get(session_id) {
            Some(cache) => cache.stats,
            None => TurnDedupStats::default(),
        },
        // 锁被污染（仅观测路径）：返回空统计，不影响调用链。
        Err(_) => TurnDedupStats::default(),
    }
}

fn invalidate(session_id: &str) {
    if let Ok(mut caches) = TURN_CACHES.lock()
        && let Some(cache) = caches.get_mut(session_id)
    {
        cache.entries.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn readonly_cfg() -> ToolDedupControl {
        ToolDedupControl::default()
    }

    fn output(text: &str) -> ToolOutput {
        ToolOutput::new(text)
    }

    #[test]
    fn readonly_classification_is_conservative() {
        let cfg = readonly_cfg();
        for name in ["read", "agentgrep", "glob", "list", "websearch"] {
            assert!(is_readonly(&cfg, name), "{name} should be readonly");
        }
        for name in [
            "bash",
            "write",
            "edit",
            "todo",
            "skill_manage",
            "batch",
            "mystery",
        ] {
            assert!(!is_readonly(&cfg, name), "{name} must not be readonly");
        }

        // 配置覆盖：空名单用内置；非空名单则以配置为准（别名会被 canonical 化）。
        let mut custom = readonly_cfg();
        custom.readonly_tools = vec!["read_file".to_string()];
        assert!(is_readonly(&custom, "read"));
        assert!(!is_readonly(&custom, "agentgrep"));
    }

    #[test]
    fn fingerprint_ignores_key_order_and_tracks_values() {
        let a = json!({"path": "a.rs", "limit": 10});
        let b = json!({"limit": 10, "path": "a.rs"});
        assert_eq!(fingerprint(&a), fingerprint(&b));
        assert_ne!(fingerprint(&a), fingerprint(&json!({"path": "a.rs"})));
    }

    #[test]
    fn observe_mode_counts_without_reusing_output() {
        let cfg = readonly_cfg();
        let session = "session-observe";
        begin_turn(session);
        let args = json!({"path": "x.rs"});
        assert!(matches!(
            before_call_with(&cfg, false, session, "read", &args),
            DedupDecision::Proceed
        ));
        assert!(matches!(
            before_call_with(&cfg, false, session, "read", &args),
            DedupDecision::Proceed
        ));
        assert_eq!(stats(session).duplicates, 1);
        // 观测模式不保存输出 → 无「潜在节省」计数。
        assert_eq!(stats(session).potential_saved_chars, 0);
        begin_turn(session);
        assert_eq!(stats(session).duplicates, 0);
    }

    #[test]
    fn dedup_mode_reuses_same_turn_output_and_write_invalidates() {
        let cfg = readonly_cfg();
        let session = "session-dedup";
        begin_turn(session);
        let args = json!({"path": "y.rs"});
        assert!(matches!(
            before_call_with(&cfg, true, session, "read", &args),
            DedupDecision::Proceed
        ));
        after_call_with(&cfg, session, "read", &args, &output("file body"));

        match before_call_with(&cfg, true, session, "read", &args) {
            DedupDecision::Cached(cached) => assert_eq!(cached.output, "file body"),
            other => panic!("expected cached output, got {other:?}"),
        }
        assert_eq!(stats(session).duplicates, 1);
        assert!(stats(session).potential_saved_chars > 0);

        // 写/执行类调用 → 缓存失效（写后读新不受影响）。
        assert!(matches!(
            before_call_with(&cfg, true, session, "write", &args),
            DedupDecision::Proceed
        ));
        assert!(matches!(
            before_call_with(&cfg, true, session, "read", &args),
            DedupDecision::Proceed
        ));
    }

    #[test]
    fn non_readonly_tools_are_never_cached() {
        let cfg = readonly_cfg();
        let session = "session-nonreadonly";
        begin_turn(session);
        let args = json!({"command": "ls"});
        before_call_with(&cfg, true, session, "bash", &args);
        after_call_with(&cfg, session, "bash", &args, &output("bash result"));
        assert!(matches!(
            before_call_with(&cfg, true, session, "bash", &args),
            DedupDecision::Proceed
        ));
        assert_eq!(stats(session).duplicates, 0);
    }

    #[test]
    fn entry_cap_is_bounded() {
        let mut cfg = readonly_cfg();
        cfg.max_entries_per_turn = 2;
        let session = "session-cap";
        begin_turn(session);
        for index in 0..6 {
            before_call_with(&cfg, false, session, "read", &json!({ "path": index }));
        }
        let entries = TURN_CACHES
            .lock()
            .expect("lock")
            .get(session)
            .map(|cache| cache.entries.len())
            .unwrap_or(0);
        assert!(entries <= cfg.max_entries_per_turn.max(1));
        begin_turn(session);
    }
}
