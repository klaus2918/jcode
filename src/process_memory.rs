use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize)]
pub struct ProcessMemorySnapshot {
    pub rss_bytes: Option<u64>,
    pub peak_rss_bytes: Option<u64>,
    pub virtual_bytes: Option<u64>,
}

#[cfg(target_os = "linux")]
pub fn snapshot() -> ProcessMemorySnapshot {
    let Ok(status) = std::fs::read_to_string("/proc/self/status") else {
        return ProcessMemorySnapshot::default();
    };

    ProcessMemorySnapshot {
        rss_bytes: parse_proc_status_value_bytes(&status, "VmRSS:"),
        peak_rss_bytes: parse_proc_status_value_bytes(&status, "VmHWM:"),
        virtual_bytes: parse_proc_status_value_bytes(&status, "VmSize:"),
    }
}

#[cfg(not(target_os = "linux"))]
pub fn snapshot() -> ProcessMemorySnapshot {
    ProcessMemorySnapshot::default()
}

#[cfg(target_os = "linux")]
fn parse_proc_status_value_bytes(status: &str, key: &str) -> Option<u64> {
    status.lines().find_map(|line| {
        let trimmed = line.trim_start();
        if !trimmed.starts_with(key) {
            return None;
        }
        let value = trimmed.trim_start_matches(key).trim();
        let mut parts = value.split_whitespace();
        let number = parts.next()?.parse::<u64>().ok()?;
        let unit = parts.next().unwrap_or("kB");
        Some(match unit {
            "kB" | "KB" | "kb" => number.saturating_mul(1024),
            "mB" | "MB" | "mb" => number.saturating_mul(1024 * 1024),
            "gB" | "GB" | "gb" => number.saturating_mul(1024 * 1024 * 1024),
            _ => number,
        })
    })
}
