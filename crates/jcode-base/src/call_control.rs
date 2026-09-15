//! 模型调用收敛控制基建（变更 `agent-model-call-optimization` · P2 #6）。
//!
//! 提供三件事：
//! 1. **开关矩阵**：集中读取 `config.model_call_control`；总开关 `enabled=false`
//!    是一键回退入口，任一项关闭即回到现状；
//! 2. **命中/降级观测**：统一事件 `MODEL_CALL_CONTROL`（只增不改），
//!    记录 hit / skip / degrade / observe，供灰度观察与诊断；
//! 3. **fail-open 兜底**：控制逻辑自身异常时降级回现状并告警，
//!    绝不阻断调用链（方案不变式 I6）。
//!
//! 约定：本模块只做「开关读取 + 观测 + 兜底」，不含任何控制行为本身；
//! 具体控制（busy-requeue 合并/退避、续写预算、轮询退化、工具去重）
//! 在各调用点按 `control_enabled` / 参数实现。

use crate::config::{ModelCallControlConfig, config};

/// 控制项标识（稳定字符串，用于事件与诊断；只增不改）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlName {
    /// busy-requeue 合并 / 冷却退避 / 尝试上限。
    BusyRequeue,
    /// 单回合续写预算统一 + 内容去重。
    ContinuationBudget,
    /// auto-poke 软阈值提醒 / 硬阈值停止。
    AutoPokeStop,
    /// 后台监控与 presence 轮询退化。
    PollBackoff,
    /// 工具回合效率（重复只读观测/去重）。
    ToolDedup,
}

impl ControlName {
    /// 稳定标识。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BusyRequeue => "busy_requeue",
            Self::ContinuationBudget => "continuation_budget",
            Self::AutoPokeStop => "auto_poke_stop",
            Self::PollBackoff => "poll_backoff",
            Self::ToolDedup => "tool_dedup",
        }
    }

    /// 全部控制项（矩阵遍历用）。
    pub const fn all() -> [ControlName; 5] {
        [
            Self::BusyRequeue,
            Self::ContinuationBudget,
            Self::AutoPokeStop,
            Self::PollBackoff,
            Self::ToolDedup,
        ]
    }
}

/// 控制观测动作（只增不改）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlAction {
    /// 控制生效（拦截/合并/去重/退化命中）。
    Hit,
    /// 控制本可生效但被配置关闭 → 回到现状。
    Skip,
    /// 控制逻辑自身异常 → 降级回现状（fail-open）。
    Degrade,
    /// 仅观测（如重复率统计），不改变行为。
    Observe,
}

impl ControlAction {
    /// 稳定标识。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Hit => "hit",
            Self::Skip => "skip",
            Self::Degrade => "degrade",
            Self::Observe => "observe",
        }
    }
}

/// 当前控制配置（克隆一份；`config()` 为进程级缓存）。
pub fn control_config() -> ModelCallControlConfig {
    config().model_call_control.clone()
}

/// 总开关是否打开（`false` = 一键回退到现状）。
pub fn controls_enabled() -> bool {
    control_config().enabled
}

/// 单项是否启用；总开关关闭时一律 `false`（回到现状）。
pub fn is_enabled(cfg: &ModelCallControlConfig, name: ControlName) -> bool {
    if !cfg.enabled {
        return false;
    }
    match name {
        ControlName::BusyRequeue => cfg.busy_requeue.enabled,
        ControlName::ContinuationBudget => cfg.continuation_budget.enabled,
        ControlName::AutoPokeStop => cfg.auto_poke_stop.enabled,
        ControlName::PollBackoff => cfg.poll_backoff.enabled,
        ControlName::ToolDedup => cfg.tool_dedup.enabled,
    }
}

/// 按当前配置判断单项是否启用。
pub fn control_enabled(name: ControlName) -> bool {
    is_enabled(&control_config(), name)
}

/// 观测事件（只增不改）：`MODEL_CALL_CONTROL control=… action=… detail=…`。
pub fn control_event(name: ControlName, action: ControlAction, detail: impl AsRef<str>) {
    crate::logging::event_info(
        "MODEL_CALL_CONTROL",
        vec![
            ("control", name.as_str().to_string()),
            ("action", action.as_str().to_string()),
            ("detail", detail.as_ref().to_string()),
        ],
    );
}

/// fail-open 兜底：控制逻辑异常时降级回现状（返回 `None`）并记降级事件。
///
/// 用法：把可能失败的准备工作包在闭包/表达式里，`None` 表示「按现状执行」。
pub fn fail_open<T, E: std::fmt::Display>(name: ControlName, result: Result<T, E>) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            control_event(
                name,
                ControlAction::Degrade,
                format!("fallback to status quo: {error}"),
            );
            None
        }
    }
}

/// 开关矩阵快照（诊断 / 信息面板）：(控制项, 是否生效, 参数摘要)。
pub fn control_matrix() -> Vec<(&'static str, bool, String)> {
    let cfg = control_config();
    vec![
        (
            ControlName::BusyRequeue.as_str(),
            is_enabled(&cfg, ControlName::BusyRequeue),
            format!(
                "merge_same_content={} backoff_base_ms={} backoff_cap_ms={} max_attempts={} idle_wait_timeout_ms={}",
                cfg.busy_requeue.merge_same_content,
                cfg.busy_requeue.backoff_base_ms,
                cfg.busy_requeue.backoff_cap_ms,
                cfg.busy_requeue.max_attempts,
                cfg.busy_requeue.idle_wait_timeout_ms,
            ),
        ),
        (
            ControlName::ContinuationBudget.as_str(),
            is_enabled(&cfg, ControlName::ContinuationBudget),
            format!(
                "max_per_turn={} dedup_same_content={}",
                cfg.continuation_budget.max_per_turn, cfg.continuation_budget.dedup_same_content,
            ),
        ),
        (
            ControlName::AutoPokeStop.as_str(),
            is_enabled(&cfg, ControlName::AutoPokeStop),
            format!(
                "soft_ratio={} non_user_only={}",
                cfg.auto_poke_stop.soft_ratio, cfg.auto_poke_stop.non_user_only,
            ),
        ),
        (
            ControlName::PollBackoff.as_str(),
            is_enabled(&cfg, ControlName::PollBackoff),
            format!(
                "active_interval_ms={} idle_interval_ms={}",
                cfg.poll_backoff.active_interval_ms, cfg.poll_backoff.idle_interval_ms,
            ),
        ),
        (
            ControlName::ToolDedup.as_str(),
            is_enabled(&cfg, ControlName::ToolDedup),
            format!(
                "readonly_tools={} max_entries_per_turn={}",
                cfg.tool_dedup.readonly_tools.len(),
                cfg.tool_dedup.max_entries_per_turn,
            ),
        ),
    ]
}

/// 轮询间隔（P2 #11）：后台监控与 presence 的自适应间隔。
///
/// 控制关闭时恒返回 `active_interval_ms`（默认等于现状 2s），即行为不变；
/// 启用后空闲态使用 `idle_interval_ms`。事件到达时调用点应主动即时刷新
/// （本函数只决定「下一次轮询」的等待时长）。
pub fn poll_interval(active: bool) -> std::time::Duration {
    let cfg = control_config();
    let control = &cfg.poll_backoff;
    let ms = if !active && is_enabled(&cfg, ControlName::PollBackoff) {
        control.idle_interval_ms
    } else {
        control.active_interval_ms
    };
    std::time::Duration::from_millis(ms.max(1))
}

/// 单回合续写预算 + 内容去重门（P2 #9）。
///
/// 约定：既有分层上限（如 context-limit 重试上限、gate 熔断）保持不变，
/// 本门只**叠加**一层「单回合续写总预算」与「相同续写不重发」；
/// 关闭时不影响任何现有语义（只计数、始终放行）。
#[derive(Debug, Default, Clone)]
pub struct ContinuationGate {
    used: u32,
    last_fingerprint: Option<u64>,
}

/// 续写裁决。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContinuationVerdict {
    /// 允许本次续写。
    Allow,
    /// 已达单回合续写上限。
    BudgetExhausted,
    /// 与上一次续写内容（结构指纹）相同 → 不重复发送。
    DuplicateContent,
}

impl ContinuationGate {
    pub fn new() -> Self {
        Self::default()
    }

    /// 本回合已消耗的续写次数。
    pub fn used(&self) -> u32 {
        self.used
    }

    /// 记录一次续写请求并给出裁决（读取当前配置）。
    pub fn check(&mut self, fingerprint: u64) -> ContinuationVerdict {
        let cfg = control_config();
        self.check_with(fingerprint, &cfg)
    }

    /// 便捷：按「续写种类 + 细节」计算指纹后裁决（读取当前配置）。
    pub fn check_kind(&mut self, kind: &str, detail: &str) -> ContinuationVerdict {
        let cfg = control_config();
        self.check_with(fingerprint_of(kind, detail), &cfg)
    }

    /// 与 [`Self::check`] 相同，但使用显式配置（便于单测与灰度对照）。
    pub fn check_with(
        &mut self,
        fingerprint: u64,
        cfg: &ModelCallControlConfig,
    ) -> ContinuationVerdict {
        let control = &cfg.continuation_budget;
        if !is_enabled(cfg, ControlName::ContinuationBudget) {
            // 只观测：继续沿用现有分层上限。
            self.used = self.used.saturating_add(1);
            self.last_fingerprint = Some(fingerprint);
            return ContinuationVerdict::Allow;
        }
        if control.dedup_same_content && self.last_fingerprint == Some(fingerprint) {
            return ContinuationVerdict::DuplicateContent;
        }
        if control.max_per_turn > 0 && self.used >= control.max_per_turn {
            return ContinuationVerdict::BudgetExhausted;
        }
        self.used = self.used.saturating_add(1);
        self.last_fingerprint = Some(fingerprint);
        ContinuationVerdict::Allow
    }
}

/// 稳定指纹（FNV-1a 64）：用于「相同续写」判定（不引入额外依赖）。
pub fn fingerprint_of(kind: &str, detail: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in kind
        .bytes()
        .chain(std::iter::once(b':'))
        .chain(detail.bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_conservative() {
        let cfg = ModelCallControlConfig::default();
        // 总开关默认打开（保证「读取到的开关」有意义），但改变行为的控制项默认全部关闭。
        assert!(cfg.enabled);
        for name in ControlName::all() {
            assert!(
                !is_enabled(&cfg, name),
                "{} must default to off (status quo)",
                name.as_str()
            );
        }
    }

    #[test]
    fn master_switch_forces_status_quo() {
        let mut cfg = ModelCallControlConfig::default();
        cfg.busy_requeue.enabled = true;
        cfg.poll_backoff.enabled = true;
        assert!(is_enabled(&cfg, ControlName::BusyRequeue));
        assert!(is_enabled(&cfg, ControlName::PollBackoff));

        cfg.enabled = false;
        for name in ControlName::all() {
            assert!(
                !is_enabled(&cfg, name),
                "master off must disable {}",
                name.as_str()
            );
        }
    }

    #[test]
    fn fail_open_never_propagates_errors() {
        let ok: Result<u32, String> = Ok(7);
        assert_eq!(fail_open(ControlName::ToolDedup, ok), Some(7));

        let err: Result<u32, String> = Err("boom".to_string());
        assert_eq!(fail_open(ControlName::ToolDedup, err), None);
    }

    #[test]
    fn control_identifiers_are_unique_and_stable() {
        let mut seen = std::collections::HashSet::new();
        for name in ControlName::all() {
            assert!(seen.insert(name.as_str()), "duplicate control id");
        }
        let actions = [
            ControlAction::Hit,
            ControlAction::Skip,
            ControlAction::Degrade,
            ControlAction::Observe,
        ];
        let mut seen = std::collections::HashSet::new();
        for action in actions {
            assert!(seen.insert(action.as_str()), "duplicate action id");
        }
    }

    #[test]
    fn continuation_gate_is_transparent_when_disabled() {
        let cfg = ModelCallControlConfig::default();
        let mut gate = ContinuationGate::new();
        for _ in 0..50 {
            assert_eq!(gate.check_with(7, &cfg), ContinuationVerdict::Allow);
        }
        // 关闭时只计数、从不拦截（现状语义）。
        assert_eq!(gate.used(), 50);
    }

    #[test]
    fn continuation_gate_dedups_and_caps_when_enabled() {
        let mut cfg = ModelCallControlConfig::default();
        cfg.continuation_budget.enabled = true;
        cfg.continuation_budget.max_per_turn = 2;
        cfg.continuation_budget.dedup_same_content = true;

        let mut gate = ContinuationGate::new();
        assert_eq!(gate.check_with(1, &cfg), ContinuationVerdict::Allow);
        // 同指纹 = 请求内容未变化的同一续写 → 不重发。
        assert_eq!(
            gate.check_with(1, &cfg),
            ContinuationVerdict::DuplicateContent
        );
        assert_eq!(gate.check_with(2, &cfg), ContinuationVerdict::Allow);
        assert_eq!(
            gate.check_with(3, &cfg),
            ContinuationVerdict::BudgetExhausted
        );
        assert_eq!(gate.used(), 2);
    }

    #[test]
    fn continuation_gate_respects_master_switch() {
        let mut cfg = ModelCallControlConfig::default();
        cfg.continuation_budget.enabled = true;
        cfg.continuation_budget.max_per_turn = 1;
        cfg.enabled = false;
        let mut gate = ContinuationGate::new();
        assert_eq!(gate.check_with(1, &cfg), ContinuationVerdict::Allow);
        assert_eq!(gate.check_with(1, &cfg), ContinuationVerdict::Allow);
    }
}
