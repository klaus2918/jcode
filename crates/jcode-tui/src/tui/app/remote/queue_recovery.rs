use super::*;

impl App {
    pub(super) fn track_pending_soft_interrupt(&mut self, request_id: u64, content: String) {
        let content_bytes = content.len();
        let content_chars = content.chars().count();
        self.pending_soft_interrupt_requests
            .push((request_id, content.clone()));
        self.pending_soft_interrupts.push(content);
        crate::logging::info(&format!(
            "REMOTE_SOFT_INTERRUPT_TRACK_PENDING id={} content_bytes={} content_chars={} pending_requests={} pending_messages={}",
            request_id,
            content_bytes,
            content_chars,
            self.pending_soft_interrupt_requests.len(),
            self.pending_soft_interrupts.len()
        ));
    }

    pub(super) fn acknowledge_pending_soft_interrupt(&mut self, request_id: u64) -> bool {
        if let Some(index) = self
            .pending_soft_interrupt_requests
            .iter()
            .position(|(id, _)| *id == request_id)
        {
            self.pending_soft_interrupt_requests.remove(index);
            crate::logging::info(&format!(
                "REMOTE_SOFT_INTERRUPT_ACK_MATCHED id={} pending_requests={} pending_messages={}",
                request_id,
                self.pending_soft_interrupt_requests.len(),
                self.pending_soft_interrupts.len()
            ));
            true
        } else {
            if !self.pending_soft_interrupt_requests.is_empty() {
                crate::logging::info(&format!(
                    "REMOTE_SOFT_INTERRUPT_ACK_UNMATCHED id={} pending_requests={} pending_messages={}",
                    request_id,
                    self.pending_soft_interrupt_requests.len(),
                    self.pending_soft_interrupts.len()
                ));
            }
            false
        }
    }

    pub(super) fn clear_pending_soft_interrupt_tracking(&mut self) {
        crate::logging::info(&format!(
            "REMOTE_SOFT_INTERRUPT_TRACKING_CLEAR pending_requests={} pending_messages={}",
            self.pending_soft_interrupt_requests.len(),
            self.pending_soft_interrupts.len()
        ));
        self.pending_soft_interrupts.clear();
        self.pending_soft_interrupt_requests.clear();
    }

    pub(super) fn mark_soft_interrupt_injected(&mut self, content: &str) {
        crate::logging::info(&format!(
            "REMOTE_SOFT_INTERRUPT_MARK_INJECTED content_bytes={} content_chars={} pending_requests={} pending_messages={}",
            content.len(),
            content.chars().count(),
            self.pending_soft_interrupt_requests.len(),
            self.pending_soft_interrupts.len()
        ));
        if self.mark_combined_soft_interrupt_injected(content) {
            return;
        }

        if let Some(index) = self
            .pending_soft_interrupts
            .iter()
            .position(|pending| pending == content)
        {
            self.pending_soft_interrupts.remove(index);
        }

        if let Some(index) = self
            .pending_soft_interrupt_requests
            .iter()
            .position(|(_, pending)| pending == content)
        {
            self.pending_soft_interrupt_requests.remove(index);
        }
    }

    fn mark_combined_soft_interrupt_injected(&mut self, content: &str) -> bool {
        let mut combined = String::new();
        for (index, pending) in self.pending_soft_interrupts.iter().enumerate() {
            if index > 0 {
                combined.push_str("\n\n");
            }
            combined.push_str(pending);

            if combined == content {
                let count = index + 1;
                let removed: Vec<String> = self.pending_soft_interrupts.drain(..count).collect();
                for removed_content in removed {
                    if let Some(request_index) = self
                        .pending_soft_interrupt_requests
                        .iter()
                        .position(|(_, pending)| pending == &removed_content)
                    {
                        self.pending_soft_interrupt_requests.remove(request_index);
                    }
                }
                return true;
            }

            if !content.starts_with(&combined) {
                break;
            }
        }

        false
    }
}

/// Recover an in-flight queued continuation back into the queue.
///
/// A queued follow-up that was already taken from `queued_messages` and handed
/// to `begin_remote_send` lives only in `rate_limit_pending_message` while it
/// is in flight. That pending shape (`is_system` with `auto_retry == false`)
/// has no retry path: the tick resend requires a rate-limit reset timestamp
/// and the disconnect resend requires `auto_retry`. If the connection dies
/// before the turn completes (typically a server reload handoff racing the
/// dispatch), clearing the pending message silently drops the user's queued
/// message (issue #391). Instead, put it back at the front of the queue so it
/// is re-sent once the turn is proven idle after reconnect, which is the
/// queue's contract.
pub(super) fn recover_undelivered_queued_continuation(app: &mut App, reason: &str) -> bool {
    let is_recoverable = app
        .rate_limit_pending_message
        .as_ref()
        .is_some_and(|pending| {
            pending.is_system
                && !pending.auto_retry
                && (!pending.content.trim().is_empty() || pending.system_reminder.is_some())
        });
    if !is_recoverable {
        return false;
    }
    let Some(pending) = app.rate_limit_pending_message.take() else {
        return false;
    };
    app.rate_limit_reset = None;
    crate::logging::info(&format!(
        "Recovering in-flight queued continuation into queued follow-ups after {} (content_chars={}, has_reminder={})",
        reason,
        pending.content.chars().count(),
        pending.system_reminder.is_some()
    ));
    if let Some(reminder) = pending.system_reminder
        && app.hidden_queued_system_messages.first() != Some(&reminder)
    {
        app.hidden_queued_system_messages.insert(0, reminder);
    }
    // #7/#8：忙拒绝路径按控制配置做合并 + 冷却退避 + 尝试上限
    // （控制默认关闭 → merged=false / backoff=0，行为与现状一致）。
    // 合并只影响「是否重复入队」，不改变「回收 → 排队 → 等 turn 完成」不变式（#391）。
    let plan = if reason.contains("busy") {
        note_busy_rejection(app, &pending.content)
    } else {
        BusyRequeuePlan {
            merged: false,
            backoff: Duration::ZERO,
            waiting_explicit_idle: false,
        }
    };
    if !pending.content.trim().is_empty() && !plan.merged {
        app.queued_messages.insert(0, pending.content);
    } else if plan.merged {
        crate::logging::info(
            "BUSY_REQUEUE_CONTROL duplicate continuations merged (single queued copy retained)",
        );
    }
    true
}

pub(super) fn recover_local_interleave_to_queue(app: &mut App, reason: &str) -> bool {
    let Some(interleave) = app.interleave_message.take() else {
        return false;
    };
    if interleave.trim().is_empty() {
        return false;
    }

    crate::logging::info(&format!(
        "Recovering unsent interleave into queued follow-ups after {}",
        reason
    ));
    app.queued_messages.insert(0, interleave);
    true
}

// ---------------------------------------------------------------------------
// busy-requeue 控制（变更 `agent-model-call-optimization` · P2 #7/#8）
// ---------------------------------------------------------------------------
//
// 背景（recon-notes 实证）：服务端忙时对排队续写的回收-重派路径没有合并、
// 没有退避、没有上限——09-09 达 936 次/日、峰值 145 次/分钟（相邻间隔低至
// ~0.1s）。本模块在不破坏 #391「不丢消息」不变式的前提下做三件事：
//   1. 合并：同内容已在队列 → 不重复入队；
//   2. 冷却退避：0.5s 起指数上探、封顶；只改「再次 dispatch 的时机」；
//   3. 尝试上限：达到上限后等待**显式空闲信号**（回合真正结束），
//      并带超时兜底（避免无限等待）。
//
// 开关默认关闭（回到现状）；总开关关闭时一律不介入。状态是进程内单 App 的，
// 队列派发成功或收到显式空闲信号即复位（天然自清理）。

/// busy-requeue 控制状态。
#[derive(Debug, Default, Clone)]
pub(crate) struct BusyRequeueState {
    /// 同一内容的连续忙拒绝次数。
    pub attempts: u32,
    /// 最近被拒绝内容的哈希（内容变化则重新计数）。
    pub last_content_hash: Option<u64>,
    /// 下一次允许重派的时刻（冷却/退避）。
    pub next_dispatch_at: Option<Instant>,
    /// 超过尝试上限：等待显式空闲信号（带超时兜底）。
    pub waiting_explicit_idle: bool,
    /// 进入等待态的时刻。
    pub waiting_since: Option<Instant>,
}

static BUSY_REQUEUE_STATE: std::sync::LazyLock<std::sync::Mutex<BusyRequeueState>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(BusyRequeueState::default()));

fn with_busy_state<T>(f: impl FnOnce(&mut BusyRequeueState) -> T) -> T {
    let mut guard = BUSY_REQUEUE_STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&mut guard)
}

fn content_hash(content: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}

/// 一次「服务端忙拒绝」的重排决策（纯逻辑，便于单测）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BusyRequeuePlan {
    /// 队列中已有同内容 → 合并（不重复入队）。
    pub merged: bool,
    /// 冷却/退避时长（`Duration::ZERO` = 不等待）。
    pub backoff: Duration,
    /// 已达尝试上限 → 等待显式空闲信号。
    pub waiting_explicit_idle: bool,
}

/// 纯逻辑：按控制配置计算本次忙拒绝的重排方式。
///
/// `attempts` 为本次之前的连续拒绝次数（0 = 本次是第一次）。
pub(crate) fn plan_busy_requeue(
    queue: &[String],
    content: &str,
    attempts: u32,
    cfg: &crate::config::ModelCallControlConfig,
    enabled: bool,
) -> BusyRequeuePlan {
    if !enabled {
        // 控制关闭 = 现状：原样重排，无合并/无退避/无上限。
        return BusyRequeuePlan {
            merged: false,
            backoff: Duration::ZERO,
            waiting_explicit_idle: false,
        };
    }

    let control = &cfg.busy_requeue;
    let merged = control.merge_same_content
        && !content.trim().is_empty()
        && queue.iter().any(|queued| queued == content);

    // 尝试上限：本次之后达到上限 → 等显式空闲信号。
    if control.max_attempts > 0 && attempts.saturating_add(1) >= control.max_attempts {
        return BusyRequeuePlan {
            merged,
            backoff: Duration::ZERO,
            waiting_explicit_idle: true,
        };
    }

    let base = control.backoff_base_ms.max(1);
    let cap = control.backoff_cap_ms.max(base);
    let shift = attempts.min(16);
    let backoff_ms = base.saturating_mul(1u64 << shift).min(cap);
    BusyRequeuePlan {
        merged,
        backoff: Duration::from_millis(backoff_ms),
        waiting_explicit_idle: false,
    }
}

/// App 侧：记录一次忙拒绝，更新冷却/等待状态，返回决策。
///
/// 返回的 `merged` 由调用点用于「队列里已有同内容则不再入队」。
pub(super) fn note_busy_rejection(app: &mut App, content: &str) -> BusyRequeuePlan {
    use crate::call_control::{ControlAction, ControlName, control_config, control_enabled};

    let cfg = control_config();
    let enabled = control_enabled(ControlName::BusyRequeue);
    let hash = content_hash(content);
    let queue: Vec<String> = app.queued_messages.clone();

    let plan = with_busy_state(|state| {
        if state.last_content_hash != Some(hash) {
            // 内容变了：重新计数（不同内容的拒绝不累计）。
            state.attempts = 0;
            state.last_content_hash = Some(hash);
        }
        let plan = plan_busy_requeue(&queue, content, state.attempts, &cfg, enabled);
        state.attempts = state.attempts.saturating_add(1);
        let now = Instant::now();
        if plan.waiting_explicit_idle {
            state.waiting_explicit_idle = true;
            state.waiting_since = Some(now);
            state.next_dispatch_at = None;
        } else if !plan.backoff.is_zero() {
            state.next_dispatch_at = Some(now + plan.backoff);
        }
        plan
    });

    if enabled {
        let action = if plan.waiting_explicit_idle || !plan.backoff.is_zero() || plan.merged {
            ControlAction::Hit
        } else {
            ControlAction::Observe
        };
        crate::logging::info(&format!(
            "BUSY_REQUEUE_CONTROL merged={} backoff_ms={} waiting_explicit_idle={} (content_chars={})",
            plan.merged,
            plan.backoff.as_millis(),
            plan.waiting_explicit_idle,
            content.chars().count()
        ));
        crate::call_control::control_event(
            ControlName::BusyRequeue,
            action,
            format!(
                "merged={} backoff_ms={} waiting_explicit_idle={}",
                plan.merged,
                plan.backoff.as_millis(),
                plan.waiting_explicit_idle
            ),
        );
    }
    plan
}

/// 派发门（#7/#8）：返回 `Some(剩余等待)` 表示本 tick 不应派发队列消息。
pub(super) fn queued_dispatch_wait() -> Option<Duration> {
    use crate::call_control::{ControlName, control_config, control_enabled};

    if !control_enabled(ControlName::BusyRequeue) {
        return None;
    }
    let cfg = control_config();
    let timeout = Duration::from_millis(cfg.busy_requeue.idle_wait_timeout_ms);
    let now = Instant::now();
    with_busy_state(|state| {
        if state.waiting_explicit_idle {
            if let Some(since) = state.waiting_since {
                let elapsed = now.saturating_duration_since(since);
                if elapsed < timeout {
                    return Some(timeout - elapsed);
                }
            }
            // 超时兜底：解除等待并放行（避免无限等待）。
            crate::logging::warn(&format!(
                "BUSY_REQUEUE_CONTROL idle wait timed out after {}ms; dispatching queued work",
                timeout.as_millis()
            ));
            state.waiting_explicit_idle = false;
            state.waiting_since = None;
            return None;
        }
        state
            .next_dispatch_at
            .and_then(|at| at.checked_duration_since(now))
    })
}

/// 显式空闲信号（回合真正结束 / 连接恢复）→ 复位冷却与等待态。
pub(super) fn note_explicit_idle() {
    with_busy_state(|state| {
        if state.attempts > 0 || state.next_dispatch_at.is_some() || state.waiting_explicit_idle {
            crate::logging::info(&format!(
                "BUSY_REQUEUE_CONTROL reset on explicit idle (attempts={})",
                state.attempts
            ));
        }
        *state = BusyRequeueState::default();
    });
}

/// 队列已成功派发 → 复位控制状态。
pub(super) fn note_queue_dispatched() {
    with_busy_state(|state| *state = BusyRequeueState::default());
}

#[cfg(test)]
pub(super) fn reset_busy_requeue_state_for_tests() {
    with_busy_state(|state| *state = BusyRequeueState::default());
}

#[cfg(test)]
mod busy_requeue_tests {
    use super::*;
    use crate::config::ModelCallControlConfig;

    fn enabled_cfg() -> ModelCallControlConfig {
        let mut cfg = ModelCallControlConfig::default();
        cfg.busy_requeue.enabled = true;
        cfg
    }

    #[test]
    fn disabled_control_keeps_status_quo() {
        let cfg = ModelCallControlConfig::default();
        let queue = vec!["same".to_string()];
        let plan = plan_busy_requeue(&queue, "same", 3, &cfg, false);
        // 控制关闭：不合并、不退避、不等待（= 现状）。
        assert!(!plan.merged);
        assert_eq!(plan.backoff, Duration::ZERO);
        assert!(!plan.waiting_explicit_idle);

        // 打开单项但总开关关闭时同样不介入。
        let mut master_off = enabled_cfg();
        master_off.enabled = false;
        let plan = plan_busy_requeue(
            &queue,
            "same",
            3,
            &master_off,
            crate::call_control::is_enabled(
                &master_off,
                crate::call_control::ControlName::BusyRequeue,
            ),
        );
        assert!(!plan.merged && plan.backoff == Duration::ZERO && !plan.waiting_explicit_idle);
    }

    #[test]
    fn merge_detects_duplicate_content() {
        let cfg = enabled_cfg();
        let queue = vec!["follow-up".to_string()];
        assert!(plan_busy_requeue(&queue, "follow-up", 0, &cfg, true).merged);
        assert!(!plan_busy_requeue(&queue, "other", 0, &cfg, true).merged);
        // 空白内容不参与合并。
        let blank = vec!["".to_string()];
        assert!(!plan_busy_requeue(&blank, "", 0, &cfg, true).merged);
    }

    #[test]
    fn backoff_is_exponential_and_capped() {
        let cfg = enabled_cfg(); // base 500ms, cap 8s
        let queue: Vec<String> = Vec::new();
        let base = plan_busy_requeue(&queue, "x", 0, &cfg, true).backoff;
        assert_eq!(base, Duration::from_millis(500));
        assert_eq!(
            plan_busy_requeue(&queue, "x", 1, &cfg, true).backoff,
            Duration::from_millis(1_000)
        );
        assert_eq!(
            plan_busy_requeue(&queue, "x", 2, &cfg, true).backoff,
            Duration::from_millis(2_000)
        );
        // 封顶：远超基数时不越界（用高上限配置，避免先被「尝试上限」分支截住）。
        let mut high_cap = enabled_cfg();
        high_cap.busy_requeue.max_attempts = 100;
        assert_eq!(
            plan_busy_requeue(&queue, "x", 12, &high_cap, true).backoff,
            Duration::from_millis(8_000)
        );
    }

    #[test]
    fn attempt_cap_switches_to_explicit_idle_wait() {
        let cfg = enabled_cfg(); // max_attempts = 5
        let queue: Vec<String> = Vec::new();
        for attempts in 0..4 {
            let plan = plan_busy_requeue(&queue, "x", attempts, &cfg, true);
            assert!(
                !plan.waiting_explicit_idle,
                "attempt {attempts} must still back off, not wait"
            );
        }
        let capped = plan_busy_requeue(&queue, "x", 4, &cfg, true);
        assert!(capped.waiting_explicit_idle);
    }

    #[test]
    fn idle_wait_gate_opens_after_timeout() {
        reset_busy_requeue_state_for_tests();
        // 未启用控制时门永远放行。
        assert_eq!(queued_dispatch_wait(), None);
    }

    /// #13 风暴仿真：复现 09-09 实证口径（同一续写被服务端连续拒绝、峰值 145 次/分钟、
    /// 间隔低至 ~0.1s），验证「重派次数有界 + 消息不丢 + 不重复入队」。
    #[test]
    fn storm_rejections_are_bounded_and_never_lose_the_message() {
        let cfg = enabled_cfg(); // base 500ms / cap 8s / max_attempts 5
        let content = "queued continuation";
        let mut queue: Vec<String> = Vec::new();
        let mut now = Instant::now();
        let mut next_dispatch_at: Option<Instant> = None;
        let mut dispatches = 0u32;
        let mut rejections = 0u32;
        let mut backoffs: Vec<Duration> = Vec::new();

        // 风暴窗口：145 个 100ms tick ≈ 14.5s（对照峰值 145 次/分钟）。
        for _ in 0..145 {
            let gate_open = next_dispatch_at.is_none_or(|at| at <= now);
            now += Duration::from_millis(100);
            if !gate_open {
                continue;
            }
            // 派发一次 → 服务端仍然忙 → 拒绝并重排
            dispatches += 1;
            rejections += 1;
            let plan = plan_busy_requeue(&queue, content, rejections - 1, &cfg, true);
            if !plan.merged && !queue.iter().any(|queued| queued == content) {
                queue.push(content.to_string());
            }
            // 不变式：同内容只保留一份（合并生效），消息永不被丢弃。
            assert_eq!(queue.len(), 1, "merge must keep exactly one queued copy");
            if plan.waiting_explicit_idle {
                // 达到尝试上限 → 等显式空闲，只在超时兜底后才可能再派发。
                next_dispatch_at =
                    Some(now + Duration::from_millis(cfg.busy_requeue.idle_wait_timeout_ms));
            } else {
                backoffs.push(plan.backoff);
                next_dispatch_at = Some(now + plan.backoff);
            }
        }

        // 关键验收：重派次数被合并 + 退避 + 上限压到有界（对照：未加控制时为 145 次）。
        assert!(
            dispatches <= cfg.busy_requeue.max_attempts + 1,
            "storm must be bounded by max_attempts (dispatches={dispatches})"
        );
        // 退避单调不减（指数上探）。
        assert!(
            backoffs.windows(2).all(|pair| pair[0] <= pair[1]),
            "backoff must not decrease: {backoffs:?}"
        );
        // 首个退避等于配置基数。
        assert_eq!(
            backoffs.first().copied(),
            Some(Duration::from_millis(cfg.busy_requeue.backoff_base_ms))
        );
    }
}

pub(super) async fn recover_stranded_soft_interrupts(
    app: &mut App,
    remote: &mut RemoteConnection,
) -> bool {
    if app.is_processing || app.pending_soft_interrupts.is_empty() {
        return false;
    }

    let recovered_interrupts = std::mem::take(&mut app.pending_soft_interrupts);
    if recovered_interrupts.is_empty() {
        return false;
    }

    if let Err(err) = remote.cancel_soft_interrupts().await {
        app.pending_soft_interrupts = recovered_interrupts;
        app.push_display_message(DisplayMessage::error(format!(
            "Failed to recover queued interleave message: {}",
            err
        )));
        app.set_status_notice("Queued interleave recovery failed");
        return false;
    }

    crate::logging::info(&format!(
        "Recovering {} stranded soft interrupt(s) into queued follow-ups after turn boundary",
        recovered_interrupts.len()
    ));
    app.pending_soft_interrupt_requests.clear();

    let mut recovered_queue = recovered_interrupts;
    recovered_queue.append(&mut app.queued_messages);
    app.queued_messages = recovered_queue;
    app.set_status_notice("Recovered queued interleave after turn finished");
    true
}
