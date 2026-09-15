//! #14 context-remaining 工具（变更 `agent-model-call-optimization` · P3）。
//!
//! 目标：让代理**查询**而不是**试探**——直接给出剩余上下文预算、本会话的调用
//! 账本摘要（调用次数 / tokens / 来源分布）与当前生效的收敛控制项，替代反复
//! 探测式验证（方案 §P3「价值导向」）。
//!
//! 数据来源：
//! - 上下文预算/占用：`Registry::guard_context_overflow` 每次工具调用后记录的
//!   会话快照（进程内，fail-open）；
//! - 调用账本：#3 的 sidecar 账本（`load_ledger_rollup`，含 swarm 子树归集）。

use super::{Tool, ToolContext, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

/// 会话上下文快照。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextSnapshot {
    pub budget_tokens: usize,
    pub used_tokens: usize,
}

static SNAPSHOTS: LazyLock<Mutex<HashMap<String, ContextSnapshot>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// 记录会话上下文快照（fail-open；调用点在工具执行完成处）。
pub fn record(session_id: &str, budget_tokens: usize, used_tokens: usize) {
    if let Ok(mut snapshots) = SNAPSHOTS.lock() {
        snapshots.insert(
            session_id.to_string(),
            ContextSnapshot {
                budget_tokens,
                used_tokens,
            },
        );
    }
}

/// 读取会话上下文快照。
pub fn snapshot(session_id: &str) -> Option<ContextSnapshot> {
    SNAPSHOTS
        .lock()
        .ok()
        .and_then(|snapshots| snapshots.get(session_id).copied())
}

/// 剩余上下文：(tokens, 剩余比例 0..=1)。
pub fn remaining(budget_tokens: usize, used_tokens: usize) -> (usize, f32) {
    let remaining = budget_tokens.saturating_sub(used_tokens);
    let percent = if budget_tokens == 0 {
        0.0
    } else {
        (remaining as f32 / budget_tokens as f32).clamp(0.0, 1.0)
    };
    (remaining, percent)
}

/// 剩余上下文 / 账本查询工具。
pub struct ContextRemainingTool;

impl ContextRemainingTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for ContextRemainingTool {
    fn name(&self) -> &str {
        "context_remaining"
    }

    fn description(&self) -> &str {
        "Report remaining context budget plus this session's model-call ledger \
         (call counts, tokens, origins) and the currently active convergence controls. \
         Use this instead of probing when deciding whether to compact or to keep working."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "intent": super::intent_schema_property()
            }
        })
    }

    async fn execute(&self, _input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let mut lines: Vec<String> = Vec::new();

        match snapshot(&ctx.session_id) {
            Some(snapshot) => {
                let (remaining_tokens, percent) =
                    remaining(snapshot.budget_tokens, snapshot.used_tokens);
                lines.push(format!(
                    "context: used {}/{} tokens · remaining {} ({:.0}%)",
                    snapshot.used_tokens,
                    snapshot.budget_tokens,
                    remaining_tokens,
                    percent * 100.0
                ));
            }
            None => lines.push(
                "context: no snapshot yet (no tool/provider call recorded in this session)"
                    .to_string(),
            ),
        }

        let ledger = crate::call_ledger::load_ledger_rollup(&ctx.session_id);
        if ledger.total_calls > 0 {
            let (input_tokens, output_tokens, cache_read, _) = ledger.token_totals();
            lines.push(format!(
                "calls: {} total · {} in / {} out / {} cached-in",
                ledger.total_calls, input_tokens, output_tokens, cache_read
            ));
            for entry in ledger.top_entries(3) {
                lines.push(format!(
                    "  {} / {} ×{} · {} in",
                    entry.origin.as_str(),
                    entry.reason.as_str(),
                    entry.count,
                    entry.input_tokens
                ));
            }
            if ledger.unknown_origin_calls > 0 || ledger.unknown_reason_calls > 0 {
                lines.push(format!(
                    "  (unattributed: origin {} · reason {})",
                    ledger.unknown_origin_calls, ledger.unknown_reason_calls
                ));
            }
        }

        let active: Vec<&str> = crate::call_control::control_matrix()
            .into_iter()
            .filter(|(_, enabled, _)| *enabled)
            .map(|(name, _, _)| name)
            .collect();
        lines.push(if active.is_empty() {
            "controls: observation-only (status quo; nothing converges calls yet)".to_string()
        } else {
            format!("controls active: {}", active.join(", "))
        });

        Ok(ToolOutput::new(lines.join("\n")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remaining_is_clamped() {
        assert_eq!(remaining(1000, 250), (750, 0.75));
        // 占用超过预算 → 剩余 0（不出现负值或 NaN）。
        assert_eq!(remaining(1000, 5000), (0, 0.0));
        // 未知预算 → 0%（调用点应提示「无快照」）。
        assert_eq!(remaining(0, 0), (0, 0.0));
    }

    #[test]
    fn snapshot_round_trip() {
        record("session-ctx", 200_000, 50_000);
        let current = snapshot("session-ctx").expect("snapshot");
        assert_eq!(current.budget_tokens, 200_000);
        assert_eq!(current.used_tokens, 50_000);
        assert!(snapshot("session-ctx-missing").is_none());
    }
}
