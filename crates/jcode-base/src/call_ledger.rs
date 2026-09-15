//! 会话级调用账本（变更 `agent-model-call-optimization` · P1 #3）。
//!
//! 目的：把「模型调用非常频繁、消耗巨大」变成**可读的事实**——按
//! `(发起方 × 原因)` 分桶累计调用数 / tokens / 耗时，并给出「打标缺口」
//! （unknown 维度）告警。数据面服务于 #4（信息部件展示）、#5（实测验证）、
//! 以及 P2/P3 的预算与收敛策略。
//!
//! 设计要点：
//! - **内存聚合 + 节流批写**：记录只改内存，按「时间/条数」节流写出侧车文件
//!   `~/.jcode/sessions/<session_id>.ledger.json`（`write_json_fast` 原子替换），
//!   避免每次调用都落盘；
//! - **只观测、fail-open**：读写失败只记日志，绝不影响主链路；
//! - **合并原语**：`merge_from` 支持 swarm 子会话账本归集到父会话；
//! - **保护性限幅**：分桶数上限 [`MAX_LEDGER_ENTRIES`]，超出丢弃并计数。
//!
//! 为什么用侧车文件而不是 `Session` 字段：不改动会话 JSON 结构与 journal 校验，
//! 降低对既有持久化稳定性的影响（方案「功能稳定性保护」不变式 I2/I5）。

use anyhow::Result;
use chrono::{DateTime, Utc};
use jcode_provider_core::{CallOrigin, CallReason, CallSource};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 账本格式版本（只增不改）。
pub const CALL_LEDGER_VERSION: u32 = 1;

/// 分桶上限（防御性限幅）：枚举组合空间为 `CallOrigin`(7) × `CallReason`(9) = 63，
/// 取 64 保证正常路径全覆盖、异常（合并/枚举扩展）时不无界增长。
pub const MAX_LEDGER_ENTRIES: usize = 64;

/// 批写节流：距上次落盘的最短间隔（毫秒）。
pub const FLUSH_MIN_INTERVAL_MS: u64 = 5_000;
/// 批写节流：自上次落盘以来最少新增记录数。
pub const FLUSH_MIN_RECORDS: u32 = 32;
/// unknown 打标缺口告警的最小间隔（毫秒），避免刷屏。
pub const UNKNOWN_ALARM_MIN_INTERVAL_MS: u64 = 600_000;
/// swarm 归集的深度与规模上限（保护性限幅，避免大扇出扫盘）。
const ROLLUP_MAX_DEPTH: usize = 3;
const ROLLUP_MAX_SESSIONS: usize = 32;

fn is_zero(value: &u64) -> bool {
    *value == 0
}

/// 单次调用携带的 token 用量（与 `Agent::TokenUsage` 解耦，便于测试）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LedgerUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
}

/// 一个分桶的累计事实：(发起方 × 原因)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CallLedgerEntry {
    pub origin: CallOrigin,
    pub reason: CallReason,
    pub count: u64,
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub cache_read_tokens: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub cache_write_tokens: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub elapsed_ms_total: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub elapsed_ms_max: u64,
    pub first_at: DateTime<Utc>,
    pub last_at: DateTime<Utc>,
}

impl CallLedgerEntry {
    fn new(source: CallSource, at: DateTime<Utc>) -> Self {
        Self {
            origin: source.origin,
            reason: source.reason,
            count: 0,
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            elapsed_ms_total: 0,
            elapsed_ms_max: 0,
            first_at: at,
            last_at: at,
        }
    }

    fn add_usage(&mut self, usage: LedgerUsage, elapsed_ms: u64, at: DateTime<Utc>) {
        self.count = self.count.saturating_add(1);
        self.input_tokens = self.input_tokens.saturating_add(usage.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(usage.output_tokens);
        self.cache_read_tokens = self
            .cache_read_tokens
            .saturating_add(usage.cache_read_tokens.unwrap_or(0));
        self.cache_write_tokens = self
            .cache_write_tokens
            .saturating_add(usage.cache_write_tokens.unwrap_or(0));
        self.elapsed_ms_total = self.elapsed_ms_total.saturating_add(elapsed_ms);
        self.elapsed_ms_max = self.elapsed_ms_max.max(elapsed_ms);
        if at < self.first_at {
            self.first_at = at;
        }
        if at > self.last_at {
            self.last_at = at;
        }
    }

    fn merge_from(&mut self, other: &Self) {
        self.count = self.count.saturating_add(other.count);
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
        self.cache_read_tokens = self
            .cache_read_tokens
            .saturating_add(other.cache_read_tokens);
        self.cache_write_tokens = self
            .cache_write_tokens
            .saturating_add(other.cache_write_tokens);
        self.elapsed_ms_total = self.elapsed_ms_total.saturating_add(other.elapsed_ms_total);
        self.elapsed_ms_max = self.elapsed_ms_max.max(other.elapsed_ms_max);
        if other.first_at < self.first_at {
            self.first_at = other.first_at;
        }
        if other.last_at > self.last_at {
            self.last_at = other.last_at;
        }
    }

    /// 平均耗时（毫秒），无样本时为 0。
    pub fn elapsed_ms_avg(&self) -> u64 {
        if self.count == 0 {
            0
        } else {
            self.elapsed_ms_total / self.count
        }
    }
}

/// 会话级账本：分桶累计 + 打标缺口计数。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CallLedger {
    #[serde(default)]
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default)]
    pub total_calls: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub unknown_origin_calls: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub unknown_reason_calls: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub dropped_entries: u64,
    #[serde(default)]
    pub entries: Vec<CallLedgerEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_unknown_alarm_at: Option<DateTime<Utc>>,
}

impl Default for CallLedger {
    fn default() -> Self {
        Self {
            version: CALL_LEDGER_VERSION,
            session_id: None,
            total_calls: 0,
            unknown_origin_calls: 0,
            unknown_reason_calls: 0,
            dropped_entries: 0,
            entries: Vec::new(),
            last_unknown_alarm_at: None,
        }
    }
}

impl CallLedger {
    /// 绑定会话 id（供展示与落盘定位）。
    pub fn with_session(session_id: impl Into<String>) -> Self {
        Self {
            session_id: Some(session_id.into()),
            ..Self::default()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.total_calls == 0 && self.entries.is_empty()
    }

    /// 记录一次 provider 调用（内存侧，无 IO）。
    pub fn record(
        &mut self,
        source: CallSource,
        usage: LedgerUsage,
        elapsed_ms: u64,
        at: DateTime<Utc>,
    ) {
        self.version = CALL_LEDGER_VERSION;
        self.total_calls = self.total_calls.saturating_add(1);
        if matches!(source.origin, CallOrigin::Unknown) {
            self.unknown_origin_calls = self.unknown_origin_calls.saturating_add(1);
        }
        if matches!(source.reason, CallReason::Unknown) {
            self.unknown_reason_calls = self.unknown_reason_calls.saturating_add(1);
        }

        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.origin == source.origin && entry.reason == source.reason)
        {
            entry.add_usage(usage, elapsed_ms, at);
            return;
        }

        if self.entries.len() >= MAX_LEDGER_ENTRIES {
            // 保护性限幅：合并进最旧的桶而不是无界增长（只丢分桶明细，不丢总量）。
            self.dropped_entries = self.dropped_entries.saturating_add(1);
            if let Some(oldest) = self.entries.iter_mut().min_by_key(|entry| entry.last_at) {
                let mut delta = CallLedgerEntry::new(source, at);
                delta.add_usage(usage, elapsed_ms, at);
                oldest.merge_from(&delta);
            }
            return;
        }

        let mut entry = CallLedgerEntry::new(source, at);
        entry.add_usage(usage, elapsed_ms, at);
        self.entries.push(entry);
    }

    /// 打标缺口计数：(origin=unknown, reason=unknown)。
    pub fn unknown_counts(&self) -> (u64, u64) {
        (self.unknown_origin_calls, self.unknown_reason_calls)
    }

    /// 取一次打标缺口告警文本；受 `min_interval_ms` 节流，无缺口返回 `None`。
    pub fn take_unknown_alarm(
        &mut self,
        now: DateTime<Utc>,
        min_interval_ms: u64,
    ) -> Option<String> {
        if self.unknown_origin_calls == 0 && self.unknown_reason_calls == 0 {
            return None;
        }
        if let Some(last) = self.last_unknown_alarm_at
            && now.signed_duration_since(last).num_milliseconds() < min_interval_ms as i64
        {
            return None;
        }
        self.last_unknown_alarm_at = Some(now);
        Some(format!(
            "call ledger attribution gap: origin=unknown {} calls, reason=unknown {} calls (total {} calls, session {})",
            self.unknown_origin_calls,
            self.unknown_reason_calls,
            self.total_calls,
            self.session_id.as_deref().unwrap_or("-"),
        ))
    }

    /// 按调用数倒序返回分桶（展示用）。
    pub fn top_entries(&self, limit: usize) -> Vec<&CallLedgerEntry> {
        let mut entries: Vec<&CallLedgerEntry> = self.entries.iter().collect();
        entries.sort_by(|a, b| {
            b.count
                .cmp(&a.count)
                .then_with(|| b.input_tokens.cmp(&a.input_tokens))
        });
        entries.truncate(limit);
        entries
    }

    /// 合并另一份账本（swarm 归集 / 恢复累加）。
    pub fn merge_from(&mut self, other: &CallLedger) {
        self.total_calls = self.total_calls.saturating_add(other.total_calls);
        self.unknown_origin_calls = self
            .unknown_origin_calls
            .saturating_add(other.unknown_origin_calls);
        self.unknown_reason_calls = self
            .unknown_reason_calls
            .saturating_add(other.unknown_reason_calls);
        self.dropped_entries = self.dropped_entries.saturating_add(other.dropped_entries);
        for incoming in &other.entries {
            if let Some(entry) = self
                .entries
                .iter_mut()
                .find(|entry| entry.origin == incoming.origin && entry.reason == incoming.reason)
            {
                entry.merge_from(incoming);
                continue;
            }
            if self.entries.len() >= MAX_LEDGER_ENTRIES {
                self.dropped_entries = self.dropped_entries.saturating_add(incoming.count);
                if let Some(oldest) = self.entries.iter_mut().min_by_key(|e| e.last_at) {
                    oldest.merge_from(incoming);
                }
                continue;
            }
            self.entries.push(incoming.clone());
        }
    }

    /// 汇总 tokens（输入 / 输出 / 缓存读 / 缓存写）。
    pub fn token_totals(&self) -> (u64, u64, u64, u64) {
        let mut input = 0u64;
        let mut output = 0u64;
        let mut cache_read = 0u64;
        let mut cache_write = 0u64;
        for entry in &self.entries {
            input = input.saturating_add(entry.input_tokens);
            output = output.saturating_add(entry.output_tokens);
            cache_read = cache_read.saturating_add(entry.cache_read_tokens);
            cache_write = cache_write.saturating_add(entry.cache_write_tokens);
        }
        (input, output, cache_read, cache_write)
    }
}

/// 侧车文件路径：`<sessions>/<session_id>.ledger.json`。
pub fn ledger_path_for_session_path(session_path: &Path) -> PathBuf {
    let mut name = session_path
        .file_stem()
        .map_or_else(std::ffi::OsString::new, |stem| stem.to_os_string());
    name.push(".ledger.json");
    session_path.with_file_name(name)
}

/// 由会话 id 解析账本路径。
pub fn ledger_path(session_id: &str) -> Result<PathBuf> {
    Ok(ledger_path_for_session_path(&crate::session::session_path(
        session_id,
    )?))
}

/// 读取账本；任何失败都 fail-open 返回空账本。
pub fn load_ledger(session_id: &str) -> CallLedger {
    match ledger_path(session_id) {
        Ok(path) => load_ledger_from_path(&path, session_id),
        Err(err) => {
            crate::logging::warn(&format!(
                "call ledger path unavailable for session {session_id}: {err}"
            ));
            CallLedger::with_session(session_id)
        }
    }
}

fn load_ledger_from_path(path: &Path, session_id: &str) -> CallLedger {
    if !path.exists() {
        return CallLedger::with_session(session_id);
    }
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) => {
            crate::logging::warn(&format!(
                "call ledger read failed ({}): {err}",
                path.display()
            ));
            return CallLedger::with_session(session_id);
        }
    };
    match serde_json::from_str::<CallLedger>(&text) {
        Ok(mut ledger) => {
            if ledger.session_id.is_none() {
                ledger.session_id = Some(session_id.to_string());
            }
            if ledger.version == 0 {
                ledger.version = CALL_LEDGER_VERSION;
            }
            ledger
        }
        Err(err) => {
            crate::logging::warn(&format!(
                "call ledger parse failed ({}): {err}",
                path.display()
            ));
            CallLedger::with_session(session_id)
        }
    }
}

/// 原子落盘账本；失败只记日志（fail-open）。返回是否写入成功。
pub fn save_ledger(session_id: &str, ledger: &CallLedger) -> bool {
    let path = match ledger_path(session_id) {
        Ok(path) => path,
        Err(err) => {
            crate::logging::warn(&format!(
                "call ledger path unavailable for session {session_id}: {err}"
            ));
            return false;
        }
    };
    match crate::storage::write_json_fast(&path, ledger) {
        Ok(()) => true,
        Err(err) => {
            crate::logging::warn(&format!(
                "call ledger write failed ({}): {err}",
                path.display()
            ));
            false
        }
    }
}

/// 会话最小头部（只读 `id` / `parent_id`，用于子会话枚举）。
#[derive(Debug, Deserialize)]
struct SessionParentStub {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    parent_id: Option<String>,
}

/// 枚举直接子会话 id（`parent_id == parent_id`）。
///
/// 扫描 `~/.jcode/sessions/`；仅用于按需展示/归集，失败一律返回空列表。
pub fn child_session_ids(parent_id: &str) -> Vec<String> {
    let mut children: Vec<String> = Vec::new();
    let base = match crate::storage::jcode_dir() {
        Ok(base) => base,
        Err(_) => return children,
    };
    let dir = base.join("sessions");
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(_) => return children,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map(|ext| ext != "json").unwrap_or(true) {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if stem.ends_with(".ledger") || stem == parent_id {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(stub) = serde_json::from_str::<SessionParentStub>(&text) else {
            continue;
        };
        if stub.parent_id.as_deref() != Some(parent_id) {
            continue;
        }
        children.push(stub.id.unwrap_or_else(|| stem.to_string()));
        if children.len() >= ROLLUP_MAX_SESSIONS {
            break;
        }
    }
    children.sort();
    children
}

/// 归集账本：根会话自身 + 其子会话（swarm 成员）账本合并结果。
pub fn load_ledger_rollup(session_id: &str) -> CallLedger {
    let mut ledger = load_ledger(session_id);
    ledger.session_id = Some(session_id.to_string());
    let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
    visited.insert(session_id.to_string());
    let mut frontier: Vec<(String, usize)> = vec![(session_id.to_string(), 0)];
    while let Some((current, depth)) = frontier.pop() {
        if depth >= ROLLUP_MAX_DEPTH || visited.len() >= ROLLUP_MAX_SESSIONS {
            continue;
        }
        for child in child_session_ids(&current) {
            if !visited.insert(child.clone()) {
                continue;
            }
            if visited.len() > ROLLUP_MAX_SESSIONS {
                break;
            }
            let child_ledger = load_ledger(&child);
            if !child_ledger.is_empty() {
                ledger.merge_from(&child_ledger);
            }
            frontier.push((child, depth + 1));
        }
    }
    ledger
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(seconds: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000 + seconds, 0)
            .single()
            .expect("valid timestamp")
    }

    fn source(origin: CallOrigin, reason: CallReason) -> CallSource {
        CallSource { origin, reason }
    }

    fn usage(input: u64, output: u64) -> LedgerUsage {
        LedgerUsage {
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: Some(1),
            cache_write_tokens: None,
        }
    }

    #[test]
    fn record_aggregates_by_origin_and_reason() {
        let mut ledger = CallLedger::with_session("s1");
        ledger.record(
            source(CallOrigin::User, CallReason::Initial),
            usage(100, 10),
            500,
            at(0),
        );
        ledger.record(
            source(CallOrigin::User, CallReason::ToolResults),
            usage(200, 20),
            700,
            at(1),
        );
        ledger.record(
            source(CallOrigin::User, CallReason::Initial),
            usage(50, 5),
            300,
            at(2),
        );

        assert_eq!(ledger.total_calls, 3);
        assert_eq!(ledger.entries.len(), 2);
        let initial = ledger
            .entries
            .iter()
            .find(|entry| entry.reason == CallReason::Initial)
            .expect("initial bucket");
        assert_eq!(initial.count, 2);
        assert_eq!(initial.input_tokens, 150);
        assert_eq!(initial.output_tokens, 15);
        assert_eq!(initial.cache_read_tokens, 2);
        assert_eq!(initial.elapsed_ms_total, 800);
        assert_eq!(initial.elapsed_ms_max, 500);
        assert_eq!(initial.elapsed_ms_avg(), 400);
        assert_eq!(initial.first_at, at(0));
        assert_eq!(initial.last_at, at(2));
        assert_eq!(ledger.unknown_counts(), (0, 0));
    }

    #[test]
    fn unknown_dimensions_are_counted_and_alarmed_with_throttle() {
        let mut ledger = CallLedger::with_session("s2");
        ledger.record(
            source(CallOrigin::Unknown, CallReason::Unknown),
            usage(1, 1),
            10,
            at(0),
        );
        assert_eq!(ledger.unknown_counts(), (1, 1));

        let first = ledger
            .take_unknown_alarm(at(10), UNKNOWN_ALARM_MIN_INTERVAL_MS)
            .expect("first alarm");
        assert!(first.contains("origin=unknown 1 calls"));
        // 同一节流窗口内不再重复告警。
        assert!(
            ledger
                .take_unknown_alarm(at(11), UNKNOWN_ALARM_MIN_INTERVAL_MS)
                .is_none()
        );
        // 窗口过后再次告警。
        assert!(
            ledger
                .take_unknown_alarm(at(700), UNKNOWN_ALARM_MIN_INTERVAL_MS)
                .is_some()
        );
    }

    #[test]
    fn merge_from_sums_buckets_and_totals() {
        let mut root = CallLedger::with_session("root");
        root.record(
            source(CallOrigin::User, CallReason::Initial),
            usage(10, 1),
            100,
            at(0),
        );
        let mut child = CallLedger::with_session("child");
        child.record(
            source(CallOrigin::Swarm, CallReason::ToolResults),
            usage(20, 2),
            200,
            at(1),
        );
        child.record(
            source(CallOrigin::User, CallReason::Initial),
            usage(5, 5),
            50,
            at(2),
        );

        root.merge_from(&child);
        assert_eq!(root.total_calls, 3);
        assert_eq!(root.entries.len(), 2);
        let user_initial = root
            .entries
            .iter()
            .find(|entry| entry.origin == CallOrigin::User)
            .expect("user bucket");
        assert_eq!(user_initial.count, 2);
        assert_eq!(user_initial.input_tokens, 15);
        assert_eq!(root.token_totals(), (35, 8, 3, 0));
    }

    #[test]
    fn entry_space_is_exhaustive_and_bounded() {
        // 7 种发起方 × 9 种原因 = 63 个可能分桶；上限 64 保证全覆盖且不无界增长。
        let mut ledger = CallLedger::default();
        for index in 0..63 {
            let origin = match index % 7 {
                0 => CallOrigin::User,
                1 => CallOrigin::AutoPoke,
                2 => CallOrigin::Gate,
                3 => CallOrigin::Swarm,
                4 => CallOrigin::ReloadRecovery,
                5 => CallOrigin::Ambient,
                _ => CallOrigin::Unknown,
            };
            let reason = match (index / 7) % 9 {
                0 => CallReason::Initial,
                1 => CallReason::ToolResults,
                2 => CallReason::ContextLimit,
                3 => CallReason::Incomplete,
                4 => CallReason::EmptyPostTool,
                5 => CallReason::Recovery,
                6 => CallReason::Retry,
                7 => CallReason::Compaction,
                _ => CallReason::Unknown,
            };
            ledger.record(source(origin, reason), usage(1, 1), 1, at(index as i64));
        }
        assert_eq!(ledger.entries.len(), 63);
        assert!(ledger.entries.len() < MAX_LEDGER_ENTRIES);
        assert_eq!(ledger.dropped_entries, 0);

        // 重复来源只累加同一分桶，不新增条目。
        let before = ledger.entries.len();
        ledger.record(
            source(CallOrigin::User, CallReason::Initial),
            usage(1, 1),
            1,
            at(1_000),
        );
        assert_eq!(ledger.entries.len(), before);
        assert_eq!(ledger.total_calls, 64);
    }

    #[test]
    fn entry_cap_merges_into_oldest_bucket_when_exceeded() {
        // 上限是防御性限幅：枚举组合空间收敛后正常不会触达，
        // 但合并/未来枚举扩展异常时必须不无界增长。
        let mut ledger = CallLedger::default();
        for index in 0..MAX_LEDGER_ENTRIES {
            let mut entry = CallLedgerEntry::new(
                source(CallOrigin::User, CallReason::Initial),
                at(index as i64),
            );
            entry.add_usage(usage(1, 1), 1, at(index as i64));
            ledger.entries.push(entry);
        }
        ledger.record(
            source(CallOrigin::Swarm, CallReason::Compaction),
            usage(4, 2),
            7,
            at(5_000),
        );
        assert_eq!(ledger.entries.len(), MAX_LEDGER_ENTRIES);
        assert_eq!(ledger.total_calls, 1);
        assert_eq!(ledger.dropped_entries, 1);
        // 被限幅的调用其 tokens 仍然被计入（总量不丢）。
        let (input, output, _, _) = ledger.token_totals();
        assert_eq!(input, MAX_LEDGER_ENTRIES as u64 + 4);
        assert_eq!(output, MAX_LEDGER_ENTRIES as u64 + 2);
    }

    #[test]
    fn top_entries_sorted_by_count_then_tokens() {
        let mut ledger = CallLedger::default();
        ledger.record(
            source(CallOrigin::User, CallReason::Initial),
            usage(10, 1),
            1,
            at(0),
        );
        for _ in 0..3 {
            ledger.record(
                source(CallOrigin::AutoPoke, CallReason::ToolResults),
                usage(5, 1),
                1,
                at(1),
            );
        }
        let top = ledger.top_entries(1);
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].origin, CallOrigin::AutoPoke);
        assert_eq!(top[0].count, 3);
    }

    #[test]
    fn ledger_serde_round_trip_is_stable() {
        let mut ledger = CallLedger::with_session("s3");
        ledger.record(
            source(CallOrigin::Swarm, CallReason::ContextLimit),
            usage(9, 3),
            42,
            at(0),
        );
        let text = serde_json::to_string(&ledger).expect("serialize");
        let parsed: CallLedger = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(parsed, ledger);
        // 缺字段的旧文件也能读（前后兼容）。
        let legacy = parsed;
        let minimal: CallLedger = serde_json::from_str("{}").expect("empty object deserializes");
        assert!(minimal.is_empty());
        assert_eq!(minimal.version, 0);
        assert_eq!(legacy.session_id.as_deref(), Some("s3"));
    }

    #[test]
    fn ledger_path_uses_sidecar_name() {
        let path = ledger_path_for_session_path(Path::new("/tmp/sessions/abc.json"));
        assert_eq!(path, PathBuf::from("/tmp/sessions/abc.ledger.json"));
    }
}
