//! 调用归因：模型请求的「来源」标签（两维度模型）。
//!
//! 变更「agent-model-call-optimization」的 P1 基建：
//!
//! - [`CallOrigin`]：**发起方**（逐回合、粘性）——用户 / auto-poke / gate /
//!   swarm / reload 恢复 / ambient / unknown。用于「非用户触发」判定与
//!   预算/停用优先级（方案决策 2）。
//! - [`CallReason`]：**本次调用的原因**（逐调用）——回合首调 / 工具结果续轮 /
//!   上下文超限 / 不完整续写 / 空响应续写 / 恢复 / 重试 / 压缩 / unknown。
//!   用于频率与效率分析（调用账本聚合维度）。
//!
//! 用法：
//! - 调用方用 [`with_call_origin`] 包住「整个回合的执行」声明发起方；
//! - 用 [`with_call_reason`] 包住「单次 provider 调用」声明本次原因；
//! - 两者可嵌套组合：内层只更新对应维度、保留另一维度；
//! - provider 边界统一读取 [`current_call_source`]（两维度）记账。
//!
//! 约定：未声明时对应维度为 `Unknown`（账本侧据此对打标缺口告警；
//! 停用优先级策略应把 `Unknown` 当作受保护类，只有明确非用户才可停）；
//! 仅观测、fail-open；不跨任务传播（tokio task-local 语义，边界读取在进入时
//! 同步完成）。

use serde::{Deserialize, Serialize};
use std::cell::Cell;
use std::future::Future;

/// 发起方（逐回合、粘性）：谁促成了这条调用链路。
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CallOrigin {
    /// 用户直接发起（输入、steer、排队追发）。
    User,
    /// auto-poke 自动推进。
    AutoPoke,
    /// 完成门（completion gate）检查触发的推进。
    Gate,
    /// swarm / 子代理成员会话。
    Swarm,
    /// reload / 会话恢复后的推进。
    ReloadRecovery,
    /// ambient 自主巡检（周期任务）。
    Ambient,
    /// 未声明来源（账本告警项；策略上按受保护类处理）。
    #[default]
    Unknown,
}

/// 本次调用的原因（逐调用）：为什么会发生这一次请求。
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CallReason {
    /// 回合的首次调用。
    Initial,
    /// 工具结果之后的正常续轮（工具循环轮次）。
    ToolResults,
    /// 上下文超限后的压缩重试续写。
    ContextLimit,
    /// 模型输出不完整（incomplete / stranded tool_use）后的续写。
    Incomplete,
    /// 工具结果之后收到空响应后的续写。
    EmptyPostTool,
    /// 响应恢复路径（response_recovery）发起的请求（保留）。
    Recovery,
    /// provider 层整请求重试（runtime 内部标记；保留）。
    Retry,
    /// 上下文压缩（compaction）自身的模型调用。
    Compaction,
    /// 未声明原因（账本告警项）。
    #[default]
    Unknown,
}

/// 调用归因二元组：发起方 × 原因。
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct CallSource {
    /// 发起方（逐回合、粘性）。
    pub origin: CallOrigin,
    /// 本次调用原因（逐调用）。
    pub reason: CallReason,
}

impl CallOrigin {
    /// 稳定的字符串标识（用于日志与账本事件字段；只增不改）。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::AutoPoke => "auto_poke",
            Self::Gate => "gate",
            Self::Swarm => "swarm",
            Self::ReloadRecovery => "reload_recovery",
            Self::Ambient => "ambient",
            Self::Unknown => "unknown",
        }
    }

    /// 是否为用户发起。
    pub const fn is_user(self) -> bool {
        matches!(self, Self::User)
    }
}

impl CallReason {
    /// 稳定的字符串标识（用于日志与账本事件字段；只增不改）。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Initial => "initial",
            Self::ToolResults => "tool_results",
            Self::ContextLimit => "context_limit",
            Self::Incomplete => "incomplete",
            Self::EmptyPostTool => "empty_post_tool",
            Self::Recovery => "recovery",
            Self::Retry => "retry",
            Self::Compaction => "compaction",
            Self::Unknown => "unknown",
        }
    }
}

impl CallSource {
    /// 是否为「非用户触发」（预算/硬停策略的默认作用域，方案决策 2）。
    ///
    /// 注意：`Unknown` 视为受保护类（尚未打标完成），只有明确非用户
    /// （且非 Unknown）才允许硬停。
    pub const fn is_non_user_triggered(self) -> bool {
        !matches!(self.origin, CallOrigin::User | CallOrigin::Unknown)
    }
}

tokio::task_local! {
    static CURRENT_CALL_SOURCE: Cell<Option<CallSource>>;
}

/// 读取当前异步任务声明的调用来源；未声明或不可用时返回全 `Unknown`。
pub fn current_call_source() -> CallSource {
    CURRENT_CALL_SOURCE
        .try_with(|cell| cell.get())
        .ok()
        .flatten()
        .unwrap_or_default()
}

/// 在作用域内声明**发起方**（保留当前已声明的原因维度）。
///
/// 典型用法：包住整个回合执行（如 swarm 成员）。
pub async fn with_call_origin<F, T>(origin: CallOrigin, fut: F) -> T
where
    F: Future<Output = T>,
{
    let mut source = current_call_source();
    source.origin = origin;
    CURRENT_CALL_SOURCE
        .scope(Cell::new(Some(source)), fut)
        .await
}

/// 在作用域内声明**本次调用原因**（保留当前已声明的发起方维度）。
///
/// 典型用法：包住单次 provider 调用。
pub async fn with_call_reason<F, T>(reason: CallReason, fut: F) -> T
where
    F: Future<Output = T>,
{
    let mut source = current_call_source();
    source.reason = reason;
    CURRENT_CALL_SOURCE
        .scope(Cell::new(Some(source)), fut)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn default_is_unknown() {
        let source = current_call_source();
        assert_eq!(source.origin, CallOrigin::Unknown);
        assert_eq!(source.reason, CallReason::Unknown);
        // Unknown 视为受保护类：不得被当作「非用户触发」硬停对象。
        assert!(!source.is_non_user_triggered());
    }

    #[tokio::test]
    async fn reason_scope_preserves_origin() {
        with_call_origin(CallOrigin::Swarm, async {
            with_call_reason(CallReason::ToolResults, async {
                let source = current_call_source();
                assert_eq!(source.origin, CallOrigin::Swarm);
                assert_eq!(source.reason, CallReason::ToolResults);
                assert!(source.is_non_user_triggered());
            })
            .await;
            // 内层作用域结束后：原因恢复为 Unknown，发起方保持 Swarm。
            assert_eq!(current_call_source().reason, CallReason::Unknown);
            assert_eq!(current_call_source().origin, CallOrigin::Swarm);
        })
        .await;
        assert_eq!(current_call_source(), CallSource::default());
    }

    #[tokio::test]
    async fn origin_scope_preserves_reason() {
        with_call_reason(CallReason::Compaction, async {
            with_call_origin(CallOrigin::Ambient, async {
                let source = current_call_source();
                assert_eq!(source.origin, CallOrigin::Ambient);
                assert_eq!(source.reason, CallReason::Compaction);
            })
            .await;
            assert_eq!(current_call_source().origin, CallOrigin::Unknown);
            assert_eq!(current_call_source().reason, CallReason::Compaction);
        })
        .await;
    }

    #[tokio::test]
    async fn source_does_not_propagate_across_spawn() {
        let observed = with_call_origin(CallOrigin::AutoPoke, async {
            tokio::spawn(async { current_call_source() })
                .await
                .expect("join")
        })
        .await;
        assert_eq!(observed, CallSource::default());
    }

    #[test]
    fn as_str_is_stable_and_unique() {
        let origins = [
            CallOrigin::User,
            CallOrigin::AutoPoke,
            CallOrigin::Gate,
            CallOrigin::Swarm,
            CallOrigin::ReloadRecovery,
            CallOrigin::Ambient,
            CallOrigin::Unknown,
        ];
        let mut seen = std::collections::HashSet::new();
        for origin in origins {
            assert!(seen.insert(origin.as_str()), "duplicate origin {origin:?}");
        }

        let reasons = [
            CallReason::Initial,
            CallReason::ToolResults,
            CallReason::ContextLimit,
            CallReason::Incomplete,
            CallReason::EmptyPostTool,
            CallReason::Recovery,
            CallReason::Retry,
            CallReason::Compaction,
            CallReason::Unknown,
        ];
        let mut seen = std::collections::HashSet::new();
        for reason in reasons {
            assert!(seen.insert(reason.as_str()), "duplicate reason {reason:?}");
        }
    }
}
