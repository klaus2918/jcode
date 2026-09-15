#![cfg_attr(test, allow(clippy::items_after_test_module))]

pub(crate) mod model_names;

use crate::todo::TodoItem;
use crate::tui::info_widget::{AmbientWidgetData, CallLedgerSummary, GitInfo, MemoryInfo};
use crate::tui::session_picker::ResumeTarget;
use crossterm::event::{KeyCode, KeyModifiers};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

type AmbientInfoCacheEntry = (std::time::Instant, bool, Option<AmbientWidgetData>, bool);

static AMBIENT_INFO_CACHE: Mutex<Option<AmbientInfoCacheEntry>> = Mutex::new(None);

/// Stale-while-revalidate cache for the git status widget. Module-level so the
/// app can force a refresh the moment it mutates the repo (commit, shell, file
/// edits) instead of waiting out the TTL with a stale branch/dirty count.
type GitInfoCacheEntry = (std::time::Instant, Option<GitInfo>, bool);
static GIT_INFO_CACHE: Mutex<Option<GitInfoCacheEntry>> = Mutex::new(None);

/// Stale-while-revalidate cache for per-session todos plus their goal-level
/// assessments (closed feedback loop etc.). Module-level so the app can force a
/// refresh the moment it persists a todo write locally, instead of showing
/// the previous list until the TTL lapses.
type TodosCacheEntry = (
    std::time::Instant,
    Vec<TodoItem>,
    Vec<crate::todo::TodoGoal>,
    bool,
);
type TodosCache = std::collections::HashMap<String, TodosCacheEntry>;
static TODOS_CACHE: std::sync::LazyLock<Mutex<TodosCache>> =
    std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

/// 调用账本摘要的节流缓存（#4）：信息面板每帧重建，而账本侧车文件是批量落盘的
/// （≥5s 或 ≥32 条），所以读盘按 TTL 节流即可，不需要每帧 IO。
type CallLedgerCacheEntry = (String, std::time::Instant, Option<CallLedgerSummary>);
static CALL_LEDGER_CACHE: Mutex<Option<CallLedgerCacheEntry>> = Mutex::new(None);

/// 账本摘要缓存 TTL 与展示的 Top 分桶数量。
const CALL_LEDGER_CACHE_TTL: Duration = Duration::from_secs(3);
const CALL_LEDGER_TOP_ROWS: usize = 2;

/// 读取当前会话（含 swarm 子树归集）的调用账本摘要；按 session 与 TTL 缓存。
///
/// 仅本地会话可用：远端会话的账本由服务端进程持有，本地不读盘（返回 None）。
pub(crate) fn cached_call_ledger_summary(session_id: &str) -> Option<CallLedgerSummary> {
    if session_id.is_empty() {
        return None;
    }
    let now = std::time::Instant::now();
    if let Ok(guard) = CALL_LEDGER_CACHE.lock()
        && let Some((cached_id, at, summary)) = guard.as_ref()
        && cached_id == session_id
        && now.saturating_duration_since(*at) < CALL_LEDGER_CACHE_TTL
    {
        return summary.clone();
    }

    let summary = CallLedgerSummary::from_ledger(
        &crate::call_ledger::load_ledger_rollup(session_id),
        CALL_LEDGER_TOP_ROWS,
    );
    if let Ok(mut guard) = CALL_LEDGER_CACHE.lock() {
        *guard = Some((session_id.to_string(), now, summary.clone()));
    }
    summary
}

/// 预算条口径（#4）：上下文剩余率 = 1 - used/limit。
///
/// P1 阶段尚无「统一预算」（P2 #6–#9），先用真实上下文占用把 `budget_percent`
/// 从恒 None 变成真实数据；P2 预算落地后由那时决定是否切换口径。
pub(crate) fn context_headroom_percent(limit: u64, used: Option<u64>) -> Option<f32> {
    let used = used?;
    if limit == 0 {
        return None;
    }
    let ratio = used.min(limit) as f32 / limit as f32;
    Some((1.0 - ratio).clamp(0.0, 1.0))
}

// ---------------------------------------------------------------------------
// #10 auto-poke 治理：连续 poke 计数 + 软阈值提醒 / 硬阈值停止
// ---------------------------------------------------------------------------

/// auto-poke 预算状态（进程内单 App；控制默认关闭 → 只计数、不介入）。
#[derive(Debug, Default, Clone)]
pub(crate) struct AutoPokeBudgetState {
    /// 本轮连续自动 poke 次数。
    pub consecutive: u32,
    /// 软阈值提醒是否已发（每个计数周期只提醒一次）。
    pub soft_reminded: bool,
}

static AUTO_POKE_BUDGET: std::sync::LazyLock<Mutex<AutoPokeBudgetState>> =
    std::sync::LazyLock::new(|| Mutex::new(AutoPokeBudgetState::default()));

/// #10 裁决。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutoPokeVerdict {
    /// 允许本次自动 poke；`soft_reminder=true` 表示本次应给出软阈值提醒。
    Allow { soft_reminder: bool },
    /// 达到硬阈值 → 停止自动推进（todos 保留，交由用户接管）。
    Stop,
}

/// 纯逻辑：给定计数与配置给出裁决（便于单测；控制关闭时恒 `Allow{false}`）。
pub(crate) fn auto_poke_verdict(
    consecutive: u32,
    soft_reminded: bool,
    cfg: &crate::config::ModelCallControlConfig,
) -> AutoPokeVerdict {
    let control = crate::call_control::ControlName::AutoPokeStop;
    if !crate::call_control::is_enabled(cfg, control) {
        return AutoPokeVerdict::Allow {
            soft_reminder: false,
        };
    }
    // 预算口径复用「单回合续写上限」，避免两套参数打架。
    let max = cfg.continuation_budget.max_per_turn.max(1);
    if consecutive >= max {
        return AutoPokeVerdict::Stop;
    }
    let soft = ((max as f32) * cfg.auto_poke_stop.soft_ratio.clamp(0.0, 1.0)).ceil() as u32;
    if !soft_reminded && soft > 0 && consecutive >= soft {
        return AutoPokeVerdict::Allow {
            soft_reminder: true,
        };
    }
    AutoPokeVerdict::Allow {
        soft_reminder: false,
    }
}

/// 记录一次「即将自动 poke」并给出裁决（控制关闭时只计数）。
pub(crate) fn note_auto_poke_and_verdict() -> AutoPokeVerdict {
    use crate::call_control::{
        ControlAction, ControlName, control_config, control_event, is_enabled,
    };

    let cfg = control_config();
    let enabled = is_enabled(&cfg, ControlName::AutoPokeStop);
    let mut guard = AUTO_POKE_BUDGET
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.consecutive = guard.consecutive.saturating_add(1);
    let verdict = auto_poke_verdict(guard.consecutive, guard.soft_reminded, &cfg);
    if matches!(
        verdict,
        AutoPokeVerdict::Allow {
            soft_reminder: true
        }
    ) {
        guard.soft_reminded = true;
    }
    let consecutive = guard.consecutive;
    drop(guard);
    if enabled {
        control_event(
            ControlName::AutoPokeStop,
            match verdict {
                AutoPokeVerdict::Stop => ControlAction::Hit,
                AutoPokeVerdict::Allow { .. } => ControlAction::Observe,
            },
            format!("consecutive={consecutive} verdict={verdict:?}"),
        );
    }
    verdict
}

/// 用户重新接管（新输入 / 手动 poke）→ 复位计数。
pub(crate) fn reset_auto_poke_budget() {
    if let Ok(mut guard) = AUTO_POKE_BUDGET.lock() {
        *guard = AutoPokeBudgetState::default();
    }
}

/// #16 停用优先级：非用户触发的验证回路（gate digest）是否应先停。
///
/// 依据当前计数与配置判断「已经达到硬阈值」——达到则非用户触发的后续动作先停，
/// 用户轮不受影响（方案 §P3「预算紧张时优先停非用户触发调用」）。
pub(crate) fn auto_poke_budget_exhausted() -> bool {
    let cfg = crate::call_control::control_config();
    let consecutive = AUTO_POKE_BUDGET
        .lock()
        .map(|guard| guard.consecutive)
        .unwrap_or(0);
    matches!(
        auto_poke_verdict(consecutive, true, &cfg),
        AutoPokeVerdict::Stop
    )
}

// ---------------------------------------------------------------------------
// #15/#16 提醒去重（一次性领取模式）
// ---------------------------------------------------------------------------

/// 上下文提醒窗口：0 = 未达阈值；1 = 70%；2 = 80%；3 = 90%。
pub(crate) fn context_warning_window(usage_percent: f64) -> u8 {
    if usage_percent >= 90.0 {
        3
    } else if usage_percent >= 80.0 {
        2
    } else if usage_percent >= 70.0 {
        1
    } else {
        0
    }
}

/// 内容指纹（FNV-1a 64），用于提醒/摘要去重。
pub(crate) fn content_fingerprint(content: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in content.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// 已领取的提醒（session → (窗口 → 已发, gate digest 指纹集合)）。
#[derive(Debug, Default)]
pub(crate) struct ReminderDedupState {
    /// 已发布过的最高上下文窗口（跨窗口后允许再提醒一次）。
    pub context_window: u8,
    /// 已投递的 gate digest 内容指纹（同内容不重复投递）。
    pub gate_digests: std::collections::HashSet<u64>,
}

const MAX_REMINDER_DIGESTS: usize = 32;

static REMINDER_DEDUP: std::sync::LazyLock<
    Mutex<std::collections::HashMap<String, ReminderDedupState>>,
> = std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

/// #15：领取一次上下文阈值提醒；`true` = 本窗口首次（应当提醒）。
pub(crate) fn claim_context_warning_window(session_id: &str, window: u8) -> bool {
    if window == 0 {
        return false;
    }
    let mut state = match REMINDER_DEDUP.lock() {
        Ok(guard) => guard,
        Err(_) => return false,
    };
    let entry = state.entry(session_id.to_string()).or_default();
    if entry.context_window >= window {
        return false;
    }
    entry.context_window = window;
    true
}

/// #16：领取一次 gate digest 投递；`true` = 该内容首次投递。
pub(crate) fn claim_gate_digest(session_id: &str, digest_fingerprint: u64) -> bool {
    let mut state = match REMINDER_DEDUP.lock() {
        Ok(guard) => guard,
        Err(_) => return false,
    };
    let entry = state.entry(session_id.to_string()).or_default();
    if entry.gate_digests.contains(&digest_fingerprint) {
        return false;
    }
    if entry.gate_digests.len() >= MAX_REMINDER_DIGESTS {
        // 保护性限幅：清空后重新计数（宁可重复一次，不无界增长）。
        entry.gate_digests.clear();
    }
    entry.gate_digests.insert(digest_fingerprint);
    true
}

/// 复位提醒去重状态（会话切换 / 测试）。
#[cfg(test)]
pub(crate) fn reset_reminder_dedup(session_id: &str) {
    if let Ok(mut state) = REMINDER_DEDUP.lock() {
        state.remove(session_id);
    }
}

#[cfg(test)]
mod reminder_dedup_tests {
    use super::*;

    #[test]
    fn context_window_matches_thresholds() {
        assert_eq!(context_warning_window(0.0), 0);
        assert_eq!(context_warning_window(69.9), 0);
        assert_eq!(context_warning_window(70.0), 1);
        assert_eq!(context_warning_window(85.0), 2);
        assert_eq!(context_warning_window(95.0), 3);
    }

    #[test]
    fn context_warning_claimed_once_per_window() {
        let session = "session-window";
        reset_reminder_dedup(session);
        assert!(claim_context_warning_window(session, 1));
        // 同一窗口不重复提醒（修复 80% 分支反复追加的问题）。
        assert!(!claim_context_warning_window(session, 1));
        assert!(claim_context_warning_window(session, 2));
        assert!(!claim_context_warning_window(session, 2));
        assert!(claim_context_warning_window(session, 3));
        // 降级窗口不再提醒。
        assert!(!claim_context_warning_window(session, 1));
        reset_reminder_dedup(session);
    }

    #[test]
    fn gate_digest_dedups_same_content() {
        let session = "session-digest";
        reset_reminder_dedup(session);
        let first = content_fingerprint("digest body");
        assert!(claim_gate_digest(session, first));
        assert!(!claim_gate_digest(session, first));
        assert!(claim_gate_digest(
            session,
            content_fingerprint("other body")
        ));
        reset_reminder_dedup(session);
    }

    #[test]
    fn content_fingerprint_is_stable() {
        assert_eq!(content_fingerprint("same"), content_fingerprint("same"));
        assert_ne!(content_fingerprint("a"), content_fingerprint("b"));
    }
}

#[cfg(test)]
mod auto_poke_budget_tests {
    use super::*;

    fn budget_cfg(max_per_turn: u32, soft_ratio: f32) -> crate::config::ModelCallControlConfig {
        let mut cfg = crate::config::ModelCallControlConfig::default();
        cfg.auto_poke_stop.enabled = true;
        cfg.auto_poke_stop.soft_ratio = soft_ratio;
        cfg.continuation_budget.max_per_turn = max_per_turn;
        cfg
    }

    #[test]
    fn disabled_control_never_intervenes() {
        let cfg = crate::config::ModelCallControlConfig::default();
        for consecutive in [0, 1, 4, 50] {
            assert_eq!(
                auto_poke_verdict(consecutive, false, &cfg),
                AutoPokeVerdict::Allow {
                    soft_reminder: false
                }
            );
        }
    }

    #[test]
    fn soft_threshold_reminds_once_then_hard_threshold_stops() {
        let cfg = budget_cfg(5, 0.8); // soft = ceil(5*0.8) = 4
        assert_eq!(
            auto_poke_verdict(3, false, &cfg),
            AutoPokeVerdict::Allow {
                soft_reminder: false
            }
        );
        assert_eq!(
            auto_poke_verdict(4, false, &cfg),
            AutoPokeVerdict::Allow {
                soft_reminder: true
            }
        );
        // 已提醒过 → 不重复提醒。
        assert_eq!(
            auto_poke_verdict(4, true, &cfg),
            AutoPokeVerdict::Allow {
                soft_reminder: false
            }
        );
        assert_eq!(auto_poke_verdict(5, true, &cfg), AutoPokeVerdict::Stop);
    }

    #[test]
    fn state_reset_clears_counters() {
        reset_auto_poke_budget();
        {
            let mut guard = AUTO_POKE_BUDGET.lock().expect("lock");
            guard.consecutive = 3;
            guard.soft_reminded = true;
        }
        reset_auto_poke_budget();
        let guard = AUTO_POKE_BUDGET.lock().expect("lock");
        assert_eq!(guard.consecutive, 0);
        assert!(!guard.soft_reminded);
    }
}

/// Backdate `Instant::now()` by up to `amount`, saturating instead of
/// panicking when the clock's epoch is too recent.
///
/// `Instant` counts from boot on Windows (QPC) and Linux (CLOCK_MONOTONIC), so
/// `Instant::now() - one_hour` panics with "overflow when subtracting duration
/// from instant" when the machine booted more recently than that. This hit
/// real users right after a reboot: the git cache invalidation below runs
/// after every bash/edit tool, crashing the whole TUI (issue #424).
pub(crate) fn backdated_now(amount: Duration) -> std::time::Instant {
    let now = std::time::Instant::now();
    let mut backdate = amount;
    loop {
        if let Some(instant) = now.checked_sub(backdate) {
            return instant;
        }
        if backdate < Duration::from_millis(1) {
            return now;
        }
        backdate /= 2;
    }
}

/// Force the git-status widget cache to refetch on its next read.
///
/// Call this right after the app changes the working tree or HEAD (commits,
/// shell commands, file edits) so the info widget reflects the new repo state
/// immediately rather than after the 5s TTL. Stale-while-revalidate still
/// applies: the next read returns the last value and kicks a background refresh.
pub(crate) fn invalidate_git_info_cache() {
    if let Ok(mut guard) = GIT_INFO_CACHE.lock()
        && let Some((ts, _cached, refreshing)) = guard.as_mut()
    {
        // Backdate the timestamp past the TTL so the next `gather_git_info`
        // treats the entry as expired and spawns a refresh, while still
        // returning the last-known value (no flicker to empty).
        *ts = backdated_now(Duration::from_secs(3600));
        *refreshing = false;
    }
}

/// Force the todos widget cache to refetch the given session on its next read.
///
/// Call this right after the app persists a local todo write so the info widget
/// reflects the new list immediately rather than after the 1s TTL.
pub(crate) fn invalidate_todos_cache(session_id: &str) {
    if let Ok(mut cache) = TODOS_CACHE.lock()
        && let Some((ts, _todos, _goals, refreshing)) = cache.get_mut(session_id)
    {
        *ts = backdated_now(Duration::from_secs(3600));
        *refreshing = false;
    }
}

/// Force the ambient widget cache to refetch on its next read.
///
/// Call this after the app changes ambient state (e.g. the `schedule` tool
/// queues or cancels a task) so the ambient panel reflects the new queue/next
/// wake immediately rather than after the 2s TTL.
pub(crate) fn invalidate_ambient_info_cache() {
    if let Ok(mut guard) = AMBIENT_INFO_CACHE.lock()
        && let Some((ts, _enabled, _cached, refreshing)) = guard.as_mut()
    {
        *ts = backdated_now(Duration::from_secs(3600));
        *refreshing = false;
    }
}

/// Open a file/URL with the system opener, unless suppressed.
///
/// Every TUI-initiated `open::that_detached` must go through here: it honors
/// NO_BROWSER/JCODE_NO_BROWSER and refuses to open anything from test binaries
/// (`browser_suppressed` detects the test harness), so `cargo test` runs never
/// pop browser windows, image viewers, or OAuth pages on the developer's
/// desktop.
pub(crate) fn open_path_or_url_detached(
    target: impl AsRef<std::ffi::OsStr>,
) -> std::io::Result<()> {
    if crate::auth::browser_suppressed(false) {
        return Err(std::io::Error::other(
            "opening files/URLs is suppressed (NO_BROWSER/JCODE_NO_BROWSER or test harness)",
        ));
    }
    open::that_detached(target)
}

/// Test-only: snapshot `(elapsed_secs, refreshing)` for a session's todos cache
/// entry, or `None` when no entry exists yet. Lets tests assert that
/// invalidation backdates the entry so the next gather treats it as expired.
#[cfg(test)]
pub(crate) fn todos_cache_entry_age_for_tests(session_id: &str) -> Option<(u64, bool)> {
    let cache = TODOS_CACHE.lock().ok()?;
    cache
        .get(session_id)
        .map(|(ts, _todos, _goals, refreshing)| (ts.elapsed().as_secs(), *refreshing))
}

/// Test-only: clear the entire todos cache so tests start from a known state.
#[cfg(test)]
pub(crate) fn clear_todos_cache_for_tests() {
    if let Ok(mut cache) = TODOS_CACHE.lock() {
        cache.clear();
    }
}

#[derive(Clone)]
pub(super) struct CachedContextSnapshot {
    pub session_key: String,
    pub is_remote: bool,
    pub display_messages_version: u64,
    pub context_revision: u64,
    pub message_count: usize,
    pub compaction_count: usize,
    pub compaction_summary_chars: usize,
    pub is_compacting: bool,
    pub snapshot: crate::tui::ContextSnapshot,
}

pub(super) fn extract_bracketed_system_message(message: &str) -> Option<String> {
    let trimmed = message.trim();
    let body = trimmed.strip_prefix("[SYSTEM:")?.trim_start();
    let body = body.strip_suffix(']').unwrap_or(body).trim();
    if body.is_empty() {
        None
    } else {
        Some(body.to_string())
    }
}

pub(super) fn launch_client_executable() -> PathBuf {
    crate::build::client_update_candidate(jcode_selfdev_types::client_selfdev_requested())
        .map(|(path, _label)| path)
        .or_else(|| std::env::current_exe().ok())
        .unwrap_or_else(|| PathBuf::from("jcode"))
}

pub(super) fn partition_queued_messages(
    messages: Vec<String>,
    reminders: Vec<String>,
) -> (Vec<String>, Option<String>, Vec<String>) {
    let mut user_messages = Vec::new();
    let mut display_system_messages = Vec::new();
    let mut reminder_parts = reminders;

    for message in messages {
        if let Some(system_message) = extract_bracketed_system_message(&message) {
            reminder_parts.push(system_message.clone());
            display_system_messages.push(system_message);
        } else {
            user_messages.push(message);
        }
    }

    let reminder = if reminder_parts.is_empty() {
        None
    } else {
        Some(reminder_parts.join("\n\n"))
    };

    (user_messages, reminder, display_system_messages)
}

#[cfg(target_os = "macos")]
pub(super) fn ctrl_bracket_fallback_to_esc(code: &mut KeyCode, modifiers: &mut KeyModifiers) {
    if !modifiers.contains(KeyModifiers::CONTROL) {
        return;
    }
    match code {
        KeyCode::Esc => {
            *code = KeyCode::Char('[');
        }
        KeyCode::Char('5') => {
            // Legacy tty mapping for Ctrl+]
            *code = KeyCode::Char(']');
        }
        _ => {}
    }
}

#[cfg(not(target_os = "macos"))]
pub(super) fn ctrl_bracket_fallback_to_esc(_code: &mut KeyCode, _modifiers: &mut KeyModifiers) {}

/// Debug command file path
pub(super) fn debug_cmd_path() -> PathBuf {
    if let Ok(path) = std::env::var("JCODE_DEBUG_CMD_PATH") {
        return PathBuf::from(path);
    }
    std::env::temp_dir().join("jcode_debug_cmd")
}

/// Debug response file path
pub(super) fn debug_response_path() -> PathBuf {
    if let Ok(path) = std::env::var("JCODE_DEBUG_RESPONSE_PATH") {
        return PathBuf::from(path);
    }
    std::env::temp_dir().join("jcode_debug_response")
}

#[path = "helpers_rate_limit_parse.rs"]
mod rate_limit_parse;
pub(super) use rate_limit_parse::parse_rate_limit_error;

pub(super) fn is_context_limit_error(error: &str) -> bool {
    if crate::provider::openai_request::is_openai_encrypted_content_too_large_error(error) {
        return true;
    }
    let lower = error.to_lowercase();
    lower.contains("context length")
        || lower.contains("context window")
        || lower.contains("maximum context")
        || lower.contains("max context")
        || lower.contains("token limit")
        || lower.contains("too many tokens")
        || lower.contains("prompt is too long")
        || lower.contains("input is too long")
        || lower.contains("request too large")
        || lower.contains("length limit")
        || lower.contains("maximum tokens")
        || (lower.contains("exceeded") && lower.contains("tokens"))
}

/// Whether `error` is a provider HTTP 413 "request too large" / payload-size
/// rejection. This is distinct from a token-context overflow: it is driven by
/// the serialized request body size (dominated by inline base64 images), so it
/// is recovered by stripping oversized images rather than by token compaction.
pub(super) fn is_request_payload_too_large_error(error: &str) -> bool {
    crate::compaction::is_request_payload_too_large_error(error)
}

/// Parse a clock time like "5am" or "12:30pm" and return duration until that time
pub(super) fn parse_clock_time_to_duration(time_str: &str) -> Option<Duration> {
    let time_lower = time_str.to_lowercase();
    let is_pm = time_lower.ends_with("pm");
    let time_part = time_lower.trim_end_matches("am").trim_end_matches("pm");

    let (hour, minute) = if time_part.contains(':') {
        let parts: Vec<&str> = time_part.split(':').collect();
        if parts.len() != 2 {
            return None;
        }
        let h: u32 = parts[0].parse().ok()?;
        let m: u32 = parts[1].parse().ok()?;
        (h, m)
    } else {
        let h: u32 = time_part.parse().ok()?;
        (h, 0)
    };

    let hour_24 = if is_pm && hour != 12 {
        hour + 12
    } else if !is_pm && hour == 12 {
        0
    } else {
        hour
    };

    if hour_24 >= 24 || minute >= 60 {
        return None;
    }

    let now = chrono::Local::now();
    let today = now.date_naive();
    let target_time = chrono::NaiveTime::from_hms_opt(hour_24, minute, 0)?;
    let mut target_datetime = today.and_time(target_time);

    if target_datetime <= now.naive_local() {
        target_datetime = (today + chrono::Duration::days(1)).and_time(target_time);
    }

    let duration_secs = (target_datetime - now.naive_local()).num_seconds();
    if duration_secs > 0 {
        Some(Duration::from_secs(duration_secs as u64))
    } else {
        None
    }
}

pub(super) fn format_cache_footer(
    read_tokens: Option<u64>,
    write_tokens: Option<u64>,
) -> Option<String> {
    let _ = (read_tokens, write_tokens);
    None
}

/// Format token count for display (e.g., 63000 -> "63K")
pub(super) fn format_tokens(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.0}k", tokens as f64 / 1_000.0)
    } else {
        format!("{}", tokens)
    }
}

/// Test-only clipboard sink.
///
/// A headless CI runner has no Wayland socket, no X11 display, and a
/// non-terminal stdout, so every real clipboard path correctly fails and
/// `copy_to_clipboard` returns false. Tests that only care about shortcut
/// wiring (does Alt+S reach the copy handler with the right text?) then fail
/// for an environment reason rather than a code reason. Capturing into this
/// sink lets those tests assert the wiring *and* the copied text without
/// depending on a desktop session (refs #596).
#[cfg(test)]
static TEST_CLIPBOARD: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Test-only: route clipboard writes into an in-process sink instead of the OS.
#[cfg(test)]
pub(crate) fn capture_clipboard_for_tests() {
    if let Ok(mut sink) = TEST_CLIPBOARD.lock() {
        *sink = Some(String::new());
    }
}

/// Test-only: the last text written while capture was enabled.
#[cfg(test)]
pub(crate) fn captured_clipboard_for_tests() -> Option<String> {
    TEST_CLIPBOARD.lock().ok().and_then(|sink| sink.clone())
}

/// Test-only: stop capturing and drop any captured text.
#[cfg(test)]
pub(crate) fn stop_capturing_clipboard_for_tests() {
    if let Ok(mut sink) = TEST_CLIPBOARD.lock() {
        *sink = None;
    }
}

/// Copy text to clipboard. On Windows and macOS, the native clipboard API
/// (arboard) is authoritative, with OSC 52 as a remote-session fallback.
/// Elsewhere, try wl-copy first (Wayland), then OSC 52 (works over SSH /
/// Docker / tmux), then arboard as a final fallback.
pub(super) fn copy_to_clipboard(text: &str) -> bool {
    // Tests that opted into capture never touch the real clipboard, so they
    // behave identically on a desktop and on a headless runner.
    #[cfg(test)]
    if let Ok(mut sink) = TEST_CLIPBOARD.lock()
        && let Some(captured) = sink.as_mut()
    {
        captured.clear();
        captured.push_str(text);
        return true;
    }

    // On Windows, the native clipboard API must run before OSC 52. Writing an
    // OSC 52 sequence to stdout "succeeds" even when the console (conhost,
    // older Windows Terminal) silently ignores it, which reported "Copied"
    // while leaving the clipboard empty (issue #497). arboard talks to the
    // Win32 clipboard directly and is authoritative there.
    #[cfg(windows)]
    {
        if arboard::Clipboard::new()
            .and_then(|mut cb| cb.set_text(text.to_string()))
            .is_ok()
        {
            return true;
        }
        copy_to_clipboard_osc52(text)
    }

    // Same class of bug on macOS: Apple Terminal (Terminal.app) silently
    // ignores OSC 52, yet writing the sequence to stdout "succeeds", so we
    // reported "Copied" while leaving the clipboard untouched. NSPasteboard
    // via arboard (with pbcopy as a belt-and-braces fallback) is authoritative
    // for local sessions; OSC 52 remains as the final remote-session fallback.
    #[cfg(target_os = "macos")]
    {
        if arboard::Clipboard::new()
            .and_then(|mut cb| cb.set_text(text.to_string()))
            .is_ok()
        {
            return true;
        }
        if let Ok(mut child) = std::process::Command::new("pbcopy")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            use std::io::Write;
            if let Some(stdin) = child.stdin.as_mut()
                && stdin.write_all(text.as_bytes()).is_ok()
            {
                drop(child.stdin.take());
                if child.wait().map(|s| s.success()).unwrap_or(false) {
                    return true;
                }
            }
        }
        return copy_to_clipboard_osc52(text);
    }

    // Linux has the same failure class (issue #504, Kali/X11): wl-copy fails
    // outside Wayland, and many terminals (xterm, older VTE) silently ignore
    // OSC 52 while the stdout write still "succeeds", so the arboard fallback
    // never ran. Prefer native clipboards when a display is available: wl-copy
    // (Wayland), then arboard (X11), and only then OSC 52 for genuinely
    // headless/remote sessions (SSH, Docker, tmux) where both native paths
    // fail fast for lack of a display server.
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        if let Ok(mut child) = std::process::Command::new("wl-copy")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            use std::io::Write;
            if let Some(stdin) = child.stdin.as_mut()
                && stdin.write_all(text.as_bytes()).is_ok()
            {
                drop(child.stdin.take());
                if child.wait().map(|s| s.success()).unwrap_or(false) {
                    return true;
                }
            }
        }
        if arboard::Clipboard::new()
            .and_then(|mut cb| cb.set_text(text.to_string()))
            .is_ok()
        {
            return true;
        }
        copy_to_clipboard_osc52(text)
    }
}

/// Copy to clipboard using the OSC 52 terminal escape sequence. This asks the
/// terminal emulator to set the system clipboard without needing a local
/// display server, making it work over SSH, inside Docker, and under tmux
/// (with `set -g set-clipboard on`). Returns false if stdout is not a TTY.
fn copy_to_clipboard_osc52(text: &str) -> bool {
    use base64::Engine as _;
    use std::io::{IsTerminal, Write};

    let mut out = std::io::stdout();
    if !out.is_terminal() {
        return false;
    }
    let encoded = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
    // OSC 52: ESC ] 52 ; c ; <base64> BEL
    let seq = format!("\x1b]52;c;{}\x07", encoded);
    out.write_all(seq.as_bytes()).is_ok() && out.flush().is_ok()
}

pub(super) fn effort_display_label(effort: &str) -> &str {
    match effort {
        "swarm" => "Swarm (light fan-out) [Beta]",
        "swarm-deep" => "Swarm Deep (Max + task graph) [Beta]",
        "max" => "Max",
        "xhigh" => "xHigh",
        "high" => "High",
        "medium" => "Medium",
        "low" => "Low",
        "none" => "None",
        other => other,
    }
}

pub(super) fn inferred_reasoning_efforts(
    provider_name: Option<&str>,
    model_name: Option<&str>,
) -> Vec<&'static str> {
    jcode_provider_core::inferred_reasoning_efforts(provider_name, model_name)
}

pub(super) fn effort_bar(index: usize, total: usize) -> String {
    let mut bar = String::new();
    for i in 0..total {
        if i == index {
            bar.push('●');
        } else {
            bar.push('○');
        }
    }
    bar
}

pub(super) fn service_tier_display_label(service_tier: &str) -> &str {
    match service_tier {
        "priority" | "fast" => "Fast",
        "flex" => "Flex",
        // Explicit disable values persisted by "/fast default off" (issue
        // #506) and accepted by the OpenAI runtime.
        "off" | "default" | "auto" | "none" => "Standard",
        other => other,
    }
}

pub(super) fn fast_mode_success_message(
    enabled: bool,
    label: &str,
    applies_next_request: bool,
) -> String {
    let status = if enabled { "on" } else { "off" };
    if applies_next_request {
        format!(
            "✓ Fast mode {} ({})\nApplies to the next request/turn. The current in-flight request keeps its existing tier.",
            status, label
        )
    } else {
        format!("✓ Fast mode {} ({})", status, label)
    }
}

pub(super) fn fast_mode_status_notice(enabled: bool, applies_next_request: bool) -> String {
    let status = if enabled { "on" } else { "off" };
    if applies_next_request {
        format!("Fast: {} (next request)", status)
    } else {
        format!("Fast: {}", status)
    }
}

pub(super) fn fast_mode_overview_message(
    enabled: bool,
    current_label: &str,
    default_enabled: bool,
    default_label: &str,
) -> String {
    format!(
        "Fast mode is {}.\nCurrent tier: {}\nSaved default: {} ({})\nUse /fast on, /fast off, or /fast default on|off.",
        if enabled { "on" } else { "off" },
        current_label,
        if default_enabled { "on" } else { "off" },
        default_label
    )
}

pub(super) fn fast_mode_default_message(default_enabled: bool, default_label: &str) -> String {
    format!(
        "Saved default fast mode is {}.\nDefault tier: {}\nUse /fast default on or /fast default off.",
        if default_enabled { "on" } else { "off" },
        default_label
    )
}

#[allow(dead_code)] // 登录流程已删，保留工具函数
pub(super) fn mask_email(email: &str) -> String {
    let trimmed = email.trim();
    let Some((local, domain)) = trimmed.split_once('@') else {
        return trimmed.to_string();
    };

    if local.is_empty() {
        return format!("***@{}", domain);
    }

    let mut chars = local.chars();
    let first = chars.next().unwrap_or('*');
    let last = chars.last().unwrap_or(first);

    let masked_local = if local.chars().count() <= 2 {
        format!("{}*", first)
    } else {
        format!("{}***{}", first, last)
    };

    format!("{}@{}", masked_local, domain)
}

/// Spawn a new terminal window that resumes a jcode session.
/// Returns Ok(true) if a terminal was successfully launched, Ok(false) if no terminal found.
fn resume_invocation_args(session_id: &str, socket: Option<&str>) -> Vec<String> {
    let mut args = vec![
        "--fresh-spawn".to_string(),
        "--resume".to_string(),
        session_id.to_string(),
    ];
    if let Some(socket) = socket.filter(|s| !s.trim().is_empty()) {
        args.push("--socket".to_string());
        args.push(socket.to_string());
    }
    args
}

fn command_display(program: &Path, args: &[String]) -> String {
    std::iter::once(program.to_string_lossy().to_string())
        .chain(args.iter().cloned())
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn build_resume_command(
    target: &ResumeTarget,
    socket: Option<&str>,
) -> (PathBuf, Vec<String>, String) {
    match target {
        ResumeTarget::JcodeSession { session_id } => {
            let exe = launch_client_executable();
            let args = resume_invocation_args(session_id, socket);
            let title = resumed_window_title(session_id);
            (exe, args, title)
        }
        ResumeTarget::ClaudeCodeSession { session_id, .. } => {
            let exe = launch_client_executable();
            let imported_id = crate::import::imported_claude_code_session_id(session_id);
            let args = resume_invocation_args(&imported_id, socket);
            let title = format!(
                "🧵 Claude Code {}",
                jcode_core::util::truncate_str(session_id, 8)
            );
            (exe, args, title)
        }
        ResumeTarget::CodexSession { session_id, .. } => {
            let exe = launch_client_executable();
            let imported_id = crate::import::imported_codex_session_id(session_id);
            let args = resume_invocation_args(&imported_id, socket);
            let title = format!("🧠 Codex {}", jcode_core::util::truncate_str(session_id, 8));
            (exe, args, title)
        }
        ResumeTarget::PiSession { session_path } => {
            let exe = launch_client_executable();
            let imported_id = crate::import::imported_pi_session_id(session_path);
            let args = resume_invocation_args(&imported_id, socket);
            let title = format!(
                "π Pi {}",
                Path::new(session_path)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("session")
            );
            (exe, args, title)
        }
        ResumeTarget::OpenCodeSession { session_id, .. } => {
            let exe = launch_client_executable();
            let imported_id = crate::import::imported_opencode_session_id(session_id);
            let args = resume_invocation_args(&imported_id, socket);
            let title = format!(
                "◌ OpenCode {}",
                jcode_core::util::truncate_str(session_id, 8)
            );
            (exe, args, title)
        }
        ResumeTarget::CursorSession { session_id, .. } => {
            let exe = launch_client_executable();
            let imported_id = crate::import::imported_cursor_session_id(session_id);
            let args = resume_invocation_args(&imported_id, socket);
            let title = format!("▮ Cursor {}", jcode_core::util::truncate_str(session_id, 8));
            (exe, args, title)
        }
    }
}

pub(super) fn resume_target_manual_command(target: &ResumeTarget, socket: Option<&str>) -> String {
    let (exe, args, _) = build_resume_command(target, socket);
    command_display(&exe, &args)
}

fn spawn_command_in_new_terminal(
    program: &Path,
    args: &[String],
    title: &str,
    cwd: &Path,
) -> anyhow::Result<bool> {
    if cfg!(test) {
        // Never launch real terminal windows from unit tests. Server-event
        // handlers (e.g. SplitResponse) call this with current_exe(), which in
        // tests is the libtest harness and would pop up a broken window.
        return Ok(false);
    }
    let command = crate::terminal_launch::TerminalCommand::new(program, args.to_vec())
        .title(title.to_string());
    crate::terminal_launch::spawn_command_in_new_terminal(&command, cwd)
}

pub(super) fn spawn_resume_target_in_new_terminal(
    target: &ResumeTarget,
    cwd: &Path,
    socket: Option<&str>,
) -> anyhow::Result<bool> {
    let (program, args, title) = build_resume_command(target, socket);
    spawn_command_in_new_terminal(&program, &args, &title, cwd)
}

/// Build the terminal command used to spawn a brand-new jcode session.
/// Split from `spawn_fresh_session_in_new_terminal` so tests can verify the
/// invocation without launching a window.
fn build_fresh_session_command(socket: Option<&str>) -> crate::terminal_launch::TerminalCommand {
    let exe = launch_client_executable();
    let mut args = vec!["--fresh-spawn".to_string()];
    if let Some(socket) = socket.map(str::trim).filter(|s| !s.is_empty()) {
        args.push("--socket".to_string());
        args.push(socket.to_string());
    }
    crate::terminal_launch::TerminalCommand::new(&exe, args)
        .title("jcode · new session".to_string())
        .kind("new-terminal")
        .fresh_spawn()
}

/// Spawn a brand-new jcode session in a new terminal window, staying on the
/// same server socket when one is configured. Returns Ok(true) when a terminal
/// was launched, Ok(false) when no supported terminal was found.
pub(super) fn spawn_fresh_session_in_new_terminal(cwd: &Path) -> anyhow::Result<bool> {
    if cfg!(test) {
        // Never launch real terminal windows from unit tests.
        return Ok(false);
    }
    let socket = std::env::var("JCODE_SOCKET").ok();
    let command = build_fresh_session_command(socket.as_deref());
    crate::terminal_launch::spawn_command_in_new_terminal(&command, cwd)
}

fn resumed_window_title(session_id: &str) -> String {
    let session_name = crate::process_title::session_name(session_id);
    let icon = crate::id::session_icon(&session_name);
    let display_title = crate::process_title::terminal_display_title_for_id(session_id);
    let session_label = crate::process_title::terminal_session_label(&session_name, None);
    let fallback_label = if let Some(server_info) =
        crate::registry::find_server_by_socket_sync(&crate::server::socket_path())
    {
        format!("jcode/{} {}", server_info.name, session_label)
    } else {
        format!("jcode {}", session_label)
    };
    crate::process_title::terminal_window_title(
        icon,
        display_title.as_deref(),
        Some(&fallback_label),
        false,
    )
}

#[cfg(unix)]
pub(super) fn spawn_in_new_terminal(
    exe: &Path,
    session_id: &str,
    cwd: &Path,
    socket: Option<&str>,
) -> anyhow::Result<bool> {
    let title = resumed_window_title(session_id);
    let args = resume_invocation_args(session_id, socket);
    spawn_command_in_new_terminal(exe, &args, &title, cwd)
}

#[cfg(not(unix))]
pub(super) fn spawn_in_new_terminal(
    _exe: &Path,
    _session_id: &str,
    _cwd: &Path,
    _socket: Option<&str>,
) -> anyhow::Result<bool> {
    Ok(false)
}

#[cfg(test)]
#[path = "helpers_tests.rs"]
mod helpers_tests;

/// Try to get an image from the system clipboard.
///
/// Returns `Some((media_type, base64_data))` if an image is available.
/// Uses `wl-paste` on Wayland, `osascript` on macOS, falls back to `arboard::get_image()`.
pub(super) fn clipboard_image() -> Option<(String, String)> {
    use base64::Engine;

    // Try wl-paste first (native Wayland - better image format support)
    if std::env::var("WAYLAND_DISPLAY").is_ok()
        && let Ok(output) = std::process::Command::new("wl-paste")
            .arg("--list-types")
            .output()
    {
        let types = String::from_utf8_lossy(&output.stdout);
        crate::logging::info(&format!(
            "clipboard_image: wl-paste types: {:?}",
            types.trim()
        ));
        let (mime, wl_type) = if types.lines().any(|t| t.trim() == "image/png") {
            ("image/png", "image/png")
        } else if types.lines().any(|t| t.trim() == "image/jpeg") {
            ("image/jpeg", "image/jpeg")
        } else if types.lines().any(|t| t.trim() == "image/webp") {
            ("image/webp", "image/webp")
        } else if types.lines().any(|t| t.trim() == "image/gif") {
            ("image/gif", "image/gif")
        } else {
            ("", "")
        };

        if !mime.is_empty()
            && let Ok(img_output) = std::process::Command::new("wl-paste")
                .args(["--type", wl_type, "--no-newline"])
                .output()
            && img_output.status.success()
            && !img_output.stdout.is_empty()
        {
            let b64 = base64::engine::general_purpose::STANDARD.encode(&img_output.stdout);
            return Some((mime.to_string(), b64));
        }

        // Fallback: check text/html for <img> tags (Discord copies HTML with image URLs)
        if types.lines().any(|t| t.trim() == "text/html")
            && let Ok(html_output) = std::process::Command::new("wl-paste")
                .args(["--type", "text/html"])
                .output()
            && html_output.status.success()
            && !html_output.stdout.is_empty()
        {
            let html = String::from_utf8_lossy(&html_output.stdout);
            crate::logging::info(&format!(
                "clipboard_image: checking HTML for img tags ({} bytes)",
                html.len()
            ));
            if let Some(url) = extract_image_url(&html) {
                crate::logging::info(&format!(
                    "clipboard_image: found image URL in HTML: {}",
                    jcode_core::util::truncate_str(&url, 80)
                ));
                if let Some(result) = download_image_url(&url) {
                    return Some(result);
                }
            }
        }
    }

    // macOS: use osascript to check clipboard for images and save as PNG via temp file
    #[cfg(target_os = "macos")]
    {
        let temp_path = std::env::temp_dir().join("jcode_clipboard.png");
        let script = format!(
            r#"use framework \"AppKit\"
            set pb to current application's NSPasteboard's generalPasteboard()
            set imgClasses to current application's NSArray's arrayWithObject:(current application's NSImage)
            if (pb's canReadObjectForClasses:imgClasses options:(missing value)) then
                set imgList to pb's readObjectsForClasses:imgClasses options:(missing value)
                set img to item 1 of imgList
                set tiffData to img's TIFFRepresentation()
                set bitmapRep to current application's NSBitmapImageRep's imageRepWithData:tiffData
                set pngData to bitmapRep's representationUsingType:(current application's NSBitmapImageFileTypePNG) properties:(missing value)
                pngData's writeToFile:\"{}\" atomically:true
                return \"ok\"
            else
                return \"none\"
            end if"#,
            temp_path.to_string_lossy()
        );
        if let Ok(output) = std::process::Command::new("osascript")
            .args(["-l", "AppleScript", "-e", &script])
            .output()
        {
            let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if result == "ok" {
                if let Ok(data) = std::fs::read(&temp_path) {
                    let _ = std::fs::remove_file(&temp_path);
                    if !data.is_empty() {
                        let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
                        return Some(("image/png".to_string(), b64));
                    }
                }
            }
        }
    }

    // Fallback: arboard (works on X11/XWayland and macOS via NSPasteboard)
    if let Ok(mut clipboard) = arboard::Clipboard::new()
        && let Ok(img) = clipboard.get_image()
        && let Some(png_data) = encode_rgba_as_png(img.width, img.height, &img.bytes)
    {
        let b64 = base64::engine::general_purpose::STANDARD.encode(&png_data);
        return Some(("image/png".to_string(), b64));
    }

    None
}

/// Extract an image URL from text that looks like an HTML img tag or a bare image URL.
/// Returns the URL if found.
pub(super) fn extract_image_url(text: &str) -> Option<String> {
    let trimmed = text.trim();

    // Check for <img src="..."> pattern (Discord web copies)
    if let Some(start) = trimmed.find("<img") {
        if let Some(src_start) = trimmed[start..].find("src=\"") {
            let url_start = start + src_start + 5;
            if let Some(url_end) = trimmed[url_start..].find('"') {
                let url = &trimmed[url_start..url_start + url_end];
                if url.starts_with("http") {
                    return Some(url.to_string());
                }
            }
        }
        if let Some(src_start) = trimmed[start..].find("src='") {
            let url_start = start + src_start + 5;
            if let Some(url_end) = trimmed[url_start..].find('\'') {
                let url = &trimmed[url_start..url_start + url_end];
                if url.starts_with("http") {
                    return Some(url.to_string());
                }
            }
        }
    }

    // Check for bare image URL
    if trimmed.starts_with("http")
        && (trimmed.contains(".png")
            || trimmed.contains(".jpg")
            || trimmed.contains(".jpeg")
            || trimmed.contains(".gif")
            || trimmed.contains(".webp"))
    {
        // Strip query params for extension check but return full URL
        return Some(trimmed.to_string());
    }

    None
}

/// Download an image from a URL and return (media_type, base64_data).
/// Uses curl for simplicity (available on all platforms).
pub(super) fn download_image_url(url: &str) -> Option<(String, String)> {
    use base64::Engine;

    let output = std::process::Command::new("curl")
        .args(["-sL", "--max-time", "10", "--max-filesize", "10000000", url])
        .output()
        .ok()?;

    if !output.status.success() || output.stdout.is_empty() {
        return None;
    }

    // Detect image type from magic bytes
    let data = &output.stdout;
    let media_type = if data.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        "image/png"
    } else if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if data.starts_with(b"GIF8") {
        "image/gif"
    } else if data.starts_with(b"RIFF") && data.len() > 12 && &data[8..12] == b"WEBP" {
        "image/webp"
    } else {
        return None;
    };

    let b64 = base64::engine::general_purpose::STANDARD.encode(data);
    Some((media_type.to_string(), b64))
}

/// Encode raw RGBA pixel data as PNG bytes.
pub(super) fn encode_rgba_as_png(width: usize, height: usize, rgba: &[u8]) -> Option<Vec<u8>> {
    use image::{ImageBuffer, RgbaImage};
    use std::io::Cursor;

    let img: RgbaImage = ImageBuffer::from_raw(width as u32, height as u32, rgba.to_vec())?;
    let mut buf = Vec::new();
    img.write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png)
        .ok()?;
    Some(buf)
}

pub(super) fn gather_git_info() -> Option<GitInfo> {
    use std::time::Instant;

    const TTL: Duration = Duration::from_secs(5);

    if let Ok(mut guard) = GIT_INFO_CACHE.lock() {
        if let Some((ts, cached, refreshing)) = guard.as_mut() {
            if ts.elapsed() < TTL {
                return cached.clone();
            }
            if *refreshing {
                return cached.clone();
            }
            let stale = cached.clone();
            *refreshing = true;
            std::thread::spawn(|| {
                let result = gather_git_info_inner();
                if let Ok(mut guard) = GIT_INFO_CACHE.lock() {
                    *guard = Some((Instant::now(), result, false));
                }
            });
            return stale;
        }

        *guard = Some((backdated_now(TTL + Duration::from_secs(1)), None, true));
        std::thread::spawn(|| {
            let result = gather_git_info_inner();
            if let Ok(mut guard) = GIT_INFO_CACHE.lock() {
                *guard = Some((Instant::now(), result, false));
            }
        });
    }
    None
}

/// Fetch a session's todos plus its goal-level assessments through the same
/// stale-while-revalidate cache, so the info widget can render goal metadata
/// (closed feedback loop and objectives) without extra disk reads per frame.
pub(super) fn gather_todos_and_goals_for_session(
    session_id: Option<&str>,
) -> (Vec<TodoItem>, Vec<crate::todo::TodoGoal>) {
    use std::time::Instant;

    const TTL: Duration = Duration::from_secs(1);

    let Some(session_id) = session_id else {
        return (Vec::new(), Vec::new());
    };

    fn fetch(session_id: &str) -> (Vec<TodoItem>, Vec<crate::todo::TodoGoal>) {
        (
            crate::todo::load_todos(session_id).unwrap_or_default(),
            crate::todo::load_goals(session_id).unwrap_or_default(),
        )
    }

    if let Ok(mut cache) = TODOS_CACHE.lock() {
        if let Some((ts, todos, goals, refreshing)) = cache.get_mut(session_id) {
            if ts.elapsed() < TTL {
                return (todos.clone(), goals.clone());
            }
            if *refreshing {
                return (todos.clone(), goals.clone());
            }
            let stale = (todos.clone(), goals.clone());
            *refreshing = true;
            let session_id = session_id.to_string();
            std::thread::spawn(move || {
                let (todos, goals) = fetch(&session_id);
                if let Ok(mut cache) = TODOS_CACHE.lock() {
                    cache.insert(session_id, (Instant::now(), todos, goals, false));
                }
            });
            return stale;
        }

        let session_id = session_id.to_string();
        cache.insert(
            session_id.clone(),
            (
                backdated_now(TTL + Duration::from_secs(1)),
                Vec::new(),
                Vec::new(),
                true,
            ),
        );
        std::thread::spawn(move || {
            let (todos, goals) = fetch(&session_id);
            if let Ok(mut cache) = TODOS_CACHE.lock() {
                cache.insert(session_id, (Instant::now(), todos, goals, false));
            }
        });
    }
    (Vec::new(), Vec::new())
}

pub(super) fn gather_memory_info(
    memory_enabled: bool,
    working_dir: Option<String>,
) -> Option<MemoryInfo> {
    use std::sync::Mutex;
    use std::time::Instant;

    static CACHE: Mutex<Option<(Instant, Option<MemoryInfo>, bool)>> = Mutex::new(None);
    const TTL: Duration = Duration::from_secs(2);

    // When memory is disabled we still surface the stored counts (with a
    // DISABLED badge) so the user can see they have memories but recall is off.
    // Live activity and the sidecar model are suppressed in that case.
    let activity = if memory_enabled {
        crate::memory::get_activity()
    } else {
        None
    };
    let sidecar_model = if memory_enabled && crate::memory::memory_sidecar_enabled() {
        let sidecar = crate::sidecar::Sidecar::new();
        Some(format!(
            "{} · {}",
            sidecar.backend_name(),
            sidecar.model_name()
        ))
    } else {
        None
    };

    let finalize = |mut info: MemoryInfo| {
        info.activity = activity.clone();
        info.sidecar_model = sidecar_model.clone();
        info.disabled = !memory_enabled;
        info
    };

    if let Ok(mut guard) = CACHE.lock() {
        if let Some((ts, cached, refreshing)) = guard.as_mut() {
            if ts.elapsed() < TTL || *refreshing {
                return match cached.clone() {
                    Some(info) => Some(finalize(info)),
                    None => fallback_memory_info(memory_enabled, &activity, &sidecar_model),
                };
            }
            let stale = match cached.clone() {
                Some(info) => Some(finalize(info)),
                None => fallback_memory_info(memory_enabled, &activity, &sidecar_model),
            };
            *refreshing = true;
            let working_dir = working_dir.clone();
            std::thread::spawn(move || {
                let result = gather_memory_info_inner(working_dir);
                if let Ok(mut guard) = CACHE.lock() {
                    *guard = Some((Instant::now(), result, false));
                }
            });
            return stale;
        }

        *guard = Some((backdated_now(TTL + Duration::from_secs(1)), None, true));
        std::thread::spawn(move || {
            let result = gather_memory_info_inner(working_dir);
            if let Ok(mut guard) = CACHE.lock() {
                *guard = Some((Instant::now(), result, false));
            }
        });
    }

    fallback_memory_info(memory_enabled, &activity, &sidecar_model)
}

fn fallback_memory_info(
    memory_enabled: bool,
    activity: &Option<crate::memory_types::MemoryActivity>,
    sidecar_model: &Option<String>,
) -> Option<MemoryInfo> {
    // No cached counts yet. Show whatever live signal we have.
    if activity.is_none() && sidecar_model.is_none() && memory_enabled {
        return None;
    }
    Some(MemoryInfo {
        sidecar_available: crate::memory::memory_sidecar_enabled(),
        sidecar_model: sidecar_model.clone(),
        activity: activity.clone(),
        disabled: !memory_enabled,
        ..Default::default()
    })
}

fn gather_memory_info_inner(working_dir: Option<String>) -> Option<MemoryInfo> {
    let activity = crate::memory::get_activity();
    let sidecar_model = if crate::memory::memory_sidecar_enabled() {
        let sidecar = crate::sidecar::Sidecar::new();
        Some(format!(
            "{} · {}",
            sidecar.backend_name(),
            sidecar.model_name()
        ))
    } else {
        None
    };

    use crate::memory::MemoryManager;

    // Scope the manager to the session working dir so the project count reads
    // the same projects/<hash>.json store the memory tool writes (issue #491).
    let manager = match working_dir.as_deref() {
        Some(dir) if !dir.trim().is_empty() => MemoryManager::new().with_project_dir(dir),
        _ => MemoryManager::new(),
    };
    let project_graph = manager.load_project_graph().ok();
    let global_graph = manager.load_global_graph().ok();

    let (project_count, global_count, by_category) = {
        let mut by_category = std::collections::HashMap::new();
        let project_count = project_graph
            .as_ref()
            .map(|p| {
                for entry in p.memories.values() {
                    *by_category.entry(entry.category.to_string()).or_insert(0) += 1;
                }
                p.memory_count()
            })
            .unwrap_or(0);
        let global_count = global_graph
            .as_ref()
            .map(|g| {
                for entry in g.memories.values() {
                    *by_category.entry(entry.category.to_string()).or_insert(0) += 1;
                }
                g.memory_count()
            })
            .unwrap_or(0);
        (project_count, global_count, by_category)
    };

    let total_count = project_count + global_count;
    let (graph_nodes, graph_edges) = crate::tui::info_widget::build_graph_topology(
        project_graph.as_ref(),
        global_graph.as_ref(),
    );

    if total_count > 0 || activity.is_some() || sidecar_model.is_some() {
        Some(MemoryInfo {
            total_count,
            project_count,
            global_count,
            by_category,
            sidecar_available: crate::memory::memory_sidecar_enabled(),
            sidecar_model,
            activity,
            disabled: false,
            graph_nodes,
            graph_edges,
        })
    } else {
        None
    }
}

pub(super) fn gather_ambient_info(ambient_enabled: bool) -> Option<AmbientWidgetData> {
    use std::time::Instant;
    const TTL: Duration = Duration::from_secs(2);

    if let Ok(mut guard) = AMBIENT_INFO_CACHE.lock() {
        if let Some((ts, cached_enabled, cached, refreshing)) = guard.as_mut() {
            if *cached_enabled == ambient_enabled && ts.elapsed() < TTL {
                return cached.clone();
            }
            if *cached_enabled == ambient_enabled && *refreshing {
                return cached.clone();
            }
            let stale = if *cached_enabled == ambient_enabled {
                cached.clone()
            } else {
                None
            };
            *refreshing = true;
            *cached_enabled = ambient_enabled;
            std::thread::spawn(move || {
                let result = gather_ambient_info_inner(ambient_enabled);
                if let Ok(mut guard) = AMBIENT_INFO_CACHE.lock() {
                    *guard = Some((Instant::now(), ambient_enabled, result, false));
                }
            });
            return stale;
        }

        *guard = Some((
            backdated_now(TTL + Duration::from_secs(1)),
            ambient_enabled,
            None,
            true,
        ));
        std::thread::spawn(move || {
            let result = gather_ambient_info_inner(ambient_enabled);
            if let Ok(mut guard) = AMBIENT_INFO_CACHE.lock() {
                *guard = Some((Instant::now(), ambient_enabled, result, false));
            }
        });
    }

    None
}

fn gather_ambient_info_inner(ambient_enabled: bool) -> Option<AmbientWidgetData> {
    let state = crate::ambient::AmbientState::load().unwrap_or_default();
    let manager = crate::ambient::AmbientManager::new().ok();
    let queue_items: Vec<_> = manager
        .as_ref()
        .map(|m| m.queue().items().to_vec())
        .unwrap_or_default();
    let queue_count = queue_items.len();
    let next_queue_item = queue_items.iter().min_by_key(|item| item.scheduled_for);
    let reminder_items: Vec<_> = queue_items
        .iter()
        .filter(|item| item.target.is_direct_delivery())
        .collect();
    let reminder_count = reminder_items.len();
    let next_reminder_item = reminder_items
        .iter()
        .min_by_key(|item| item.scheduled_for)
        .copied();

    if !ambient_enabled && reminder_count == 0 {
        return None;
    }

    let last_run_ago = state.last_run.map(|t| {
        let ago = chrono::Utc::now() - t;
        if ago.num_hours() > 0 {
            format!("{}h ago", ago.num_hours())
        } else {
            format!("{}m ago", ago.num_minutes().max(0))
        }
    });
    let next_wake = match &state.status {
        crate::ambient::AmbientStatus::Scheduled { next_wake } => {
            Some(format_countdown_until(*next_wake))
        }
        _ => None,
    };

    let next_queue_preview = next_queue_item.map(|item| {
        item.task_description
            .as_deref()
            .unwrap_or(&item.context)
            .to_string()
    });
    let next_reminder_preview = next_reminder_item.map(|item| {
        item.task_description
            .as_deref()
            .unwrap_or(&item.context)
            .to_string()
    });

    Some(AmbientWidgetData {
        show_widget: ambient_enabled || reminder_count > 1,
        status: state.status,
        queue_count,
        next_queue_preview,
        reminder_count,
        next_reminder_preview,
        last_run_ago,
        last_summary: state.last_summary,
        next_wake,
        next_reminder_wake: next_reminder_item
            .map(|item| format_countdown_until(item.scheduled_for)),
        budget_percent: None,
    })
}

#[cfg(test)]
pub(crate) fn clear_ambient_info_cache_for_tests() {
    if let Ok(mut guard) = AMBIENT_INFO_CACHE.lock() {
        *guard = None;
    }
}

pub(crate) fn format_countdown_until(target: chrono::DateTime<chrono::Utc>) -> String {
    let secs = (target - chrono::Utc::now()).num_seconds().max(0);
    match secs {
        0..=59 => format!("in {}s", secs),
        60..=3599 => {
            let mins = secs / 60;
            let rem = secs % 60;
            if rem == 0 {
                format!("in {}m", mins)
            } else {
                format!("in {}m {}s", mins, rem)
            }
        }
        _ => {
            let hours = secs / 3600;
            let mins = (secs % 3600) / 60;
            if mins == 0 {
                format!("in {}h", hours)
            } else {
                format!("in {}h {}m", hours, mins)
            }
        }
    }
}

fn gather_git_info_inner() -> Option<GitInfo> {
    use std::process::Command;

    let in_repo = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .ok()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !in_repo {
        return None;
    }

    let branch = Command::new("git")
        .args(["branch", "--show-current"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                let b = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if b.is_empty() { None } else { Some(b) }
            } else {
                None
            }
        })
        .unwrap_or_else(|| "HEAD".to_string());

    let mut modified = 0;
    let mut staged = 0;
    let mut untracked = 0;
    let mut dirty_files = Vec::new();

    if let Ok(output) = Command::new("git").args(["status", "--porcelain"]).output()
        && output.status.success()
    {
        let status = String::from_utf8_lossy(&output.stdout);
        for line in status.lines() {
            if line.len() < 3 {
                continue;
            }
            let index_status = line.as_bytes()[0];
            let worktree_status = line.as_bytes()[1];
            let file_path = line[3..].to_string();

            if index_status == b'?' {
                untracked += 1;
            } else {
                if index_status != b' ' && index_status != b'?' {
                    staged += 1;
                }
                if worktree_status != b' ' && worktree_status != b'?' {
                    modified += 1;
                }
            }

            if dirty_files.len() < 10 {
                dirty_files.push(file_path);
            }
        }
    }

    let (ahead, behind) = Command::new("git")
        .args(["rev-list", "--left-right", "--count", "HEAD...@{upstream}"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                let text = String::from_utf8_lossy(&o.stdout).trim().to_string();
                let parts: Vec<&str> = text.split('\t').collect();
                if parts.len() == 2 {
                    let a = parts[0].parse::<usize>().unwrap_or(0);
                    let b = parts[1].parse::<usize>().unwrap_or(0);
                    Some((a, b))
                } else {
                    None
                }
            } else {
                None
            }
        })
        .unwrap_or((0, 0));

    Some(GitInfo {
        branch,
        modified,
        staged,
        untracked,
        ahead,
        behind,
        dirty_files,
    })
}
