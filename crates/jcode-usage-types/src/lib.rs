//! Usage tracking types shared across the stack (provider usage reports and
//! the Copilot-style usage tracker). Telemetry event types were removed when
//! client-side telemetry reporting was deleted.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default)]
pub struct ProviderUsage {
    pub provider_name: String,
    pub limits: Vec<UsageLimit>,
    pub extra_info: Vec<(String, String)>,
    pub hard_limit_reached: bool,
    pub error: Option<String>,
    /// When jcode last successfully used this login/credential (unix seconds).
    /// Drives most-recently-used-first ordering in `/usage`. `None` sorts last.
    pub last_used_unix_secs: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct UsageLimit {
    pub name: String,
    pub usage_percent: f32,
    pub resets_at: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ProviderUsageProgress {
    pub results: Vec<ProviderUsage>,
    pub completed: usize,
    pub total: usize,
    pub done: bool,
    pub from_cache: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CopilotUsageTracker {
    pub today: DayUsage,
    pub month: MonthUsage,
    pub all_time: AllTimeUsage,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DayUsage {
    pub date: String,
    pub requests: u64,
    pub premium_requests: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MonthUsage {
    pub month: String,
    pub requests: u64,
    pub premium_requests: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AllTimeUsage {
    pub requests: u64,
    pub premium_requests: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

// ---------------------------------------------------------------------------
// Ambient usage records (merged from the removed jcode-ambient-types crate,
// feature-simplification #32 / M-2, 2026-08-16).
// ---------------------------------------------------------------------------

use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum UsageSource {
    User,
    Ambient,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRecord {
    pub timestamp: DateTime<Utc>,
    pub source: UsageSource,
    pub tokens_input: u32,
    pub tokens_output: u32,
    pub provider: String,
}

impl UsageRecord {
    pub fn total_tokens(&self) -> u64 {
        self.tokens_input as u64 + self.tokens_output as u64
    }
}

#[derive(Debug, Clone)]
pub struct RateLimitInfo {
    pub limit_tokens: Option<u64>,
    pub remaining_tokens: Option<u64>,
    pub limit_requests: Option<u64>,
    pub remaining_requests: Option<u64>,
    pub reset_at: Option<DateTime<Utc>>,
}
