//! Update-path helpers (merged from the removed jcode-update-core crate,
//! feature-simplification #36 / M-6, 2026-08-16).

/// Summary emitted when `git pull` cannot reconcile the local and upstream
/// histories on its own (diverged branches, non-fast-forward, unrelated
/// histories). Callers use this to recognize a divergence and offer a merge
/// affordance instead of a generic failure.
pub const GIT_PULL_DIVERGED_SUMMARY: &str =
    "Local and upstream have diverged, so the update could not fast-forward.";

const DOWNLOAD_PROGRESS_BAR_WIDTH: usize = 24;

#[derive(Debug, Clone, Copy)]
pub struct DownloadProgress {
    pub downloaded: u64,
    pub total: Option<u64>,
}

pub fn summarize_git_pull_failure(stderr: &[u8]) -> String {
    let stderr = String::from_utf8_lossy(stderr);
    let text = stderr.trim();
    if text.is_empty() {
        return "git pull failed".to_string();
    }

    if git_pull_failure_is_divergence(text) {
        return GIT_PULL_DIVERGED_SUMMARY.to_string();
    }

    if text.contains("There is no tracking information for the current branch") {
        return "git pull failed: current branch has no upstream tracking branch".to_string();
    }

    let line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("hint:"))
        .unwrap_or("git pull failed");
    let line = line.strip_prefix("fatal: ").unwrap_or(line);
    if line.eq_ignore_ascii_case("git pull failed") {
        "git pull failed".to_string()
    } else {
        format!("git pull failed: {}", line)
    }
}

/// Whether `git pull` stderr indicates the local and upstream branches have
/// diverged (and therefore need a manual merge/rebase, not a fast-forward).
pub fn git_pull_failure_is_divergence(stderr: &str) -> bool {
    stderr.contains("Need to specify how to reconcile divergent branches")
        || stderr.contains("Not possible to fast-forward")
        || stderr.contains("refusing to merge unrelated histories")
        || stderr.contains("have diverged")
}

/// Whether a `summarize_git_pull_failure` summary describes a divergence.
pub fn summary_is_divergence(summary: &str) -> bool {
    summary == GIT_PULL_DIVERGED_SUMMARY
}

/// Longest single-line update summary we hand to the UI.
const UPDATE_ERROR_SUMMARY_MAX_CHARS: usize = 72;

/// Condense any update-path error into a single short line fit for a status
/// notice or a one-line card.
///
/// Update errors reach the UI from many layers (git, cargo, tar, install), so
/// raw text is often multi-line, prefixed with redundant "Update failed:"
/// wrappers, and long enough to wrap several times. Users only need to know
/// what went wrong in a few words; the full text stays in the log.
pub fn summarize_update_error(error: &str) -> String {
    let text = error.trim();
    // Divergence is matched verbatim by callers that offer a merge affordance,
    // so never rewrite it.
    if summary_is_divergence(text) || git_pull_failure_is_divergence(text) {
        return GIT_PULL_DIVERGED_SUMMARY.to_string();
    }

    // Strip the wrapper prefixes callers stack on top of each other.
    let mut stripped = text;
    while let Some(next) = ["Update failed:", "Update check failed:", "Check failed:"]
        .iter()
        .find_map(|prefix| stripped.strip_prefix(prefix))
        .map(str::trim_start)
    {
        stripped = next;
    }

    let first_line = stripped
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("unknown error");
    if summary_is_divergence(first_line) {
        return GIT_PULL_DIVERGED_SUMMARY.to_string();
    }
    if let Some(known) = known_update_error_summary(first_line) {
        return known.to_string();
    }

    // Keep one clause: drop any trailing context sentence and punctuation.
    let clause = first_line
        .split_once(". ")
        .map(|(head, _)| head)
        .unwrap_or(first_line)
        .trim_end_matches(['.', ':'])
        .trim();
    let clause = if clause.is_empty() {
        first_line
    } else {
        clause
    };

    if clause.chars().count() <= UPDATE_ERROR_SUMMARY_MAX_CHARS {
        return clause.to_string();
    }
    let truncated: String = clause
        .chars()
        .take(UPDATE_ERROR_SUMMARY_MAX_CHARS - 1)
        .collect();
    format!("{}…", truncated.trim_end())
}

/// Map noisy tooling failures onto short human phrases.
fn known_update_error_summary(line: &str) -> Option<&'static str> {
    let lower = line.to_ascii_lowercase();
    let has = |needle: &str| lower.contains(needle);

    if has("dns") || has("failed to lookup address") || has("name or service not known") {
        return Some("no network connection");
    }
    if has("timed out") || has("timeout") {
        return Some("connection timed out");
    }
    if has("connection refused")
        || has("connection reset")
        || has("network is unreachable")
        || has("tcp connect error")
    {
        return Some("could not connect");
    }
    if has("permission denied") || has("read-only file system") || has("os error 13") {
        return Some("no permission to install the update");
    }
    if has("no space left") {
        return Some("not enough disk space");
    }
    if has("cargo build failed") {
        return Some("cargo build failed");
    }
    None
}

pub fn format_download_progress_bar(progress: DownloadProgress) -> String {
    let human_downloaded = format_bytes(progress.downloaded);
    let Some(total) = progress.total.filter(|total| *total > 0) else {
        return format!("Downloading update... {} downloaded", human_downloaded);
    };

    let ratio = (progress.downloaded as f64 / total as f64).clamp(0.0, 1.0);
    let filled = (ratio * DOWNLOAD_PROGRESS_BAR_WIDTH as f64).round() as usize;
    let filled = filled.min(DOWNLOAD_PROGRESS_BAR_WIDTH);
    let empty = DOWNLOAD_PROGRESS_BAR_WIDTH.saturating_sub(filled);
    let percent = (ratio * 100.0).round() as u64;
    format!(
        "Downloading update... [{}{}] {:>3}% ({}/{})",
        "█".repeat(filled),
        "░".repeat(empty),
        percent,
        human_downloaded,
        format_bytes(total)
    )
}

pub fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    let bytes_f = bytes as f64;
    if bytes_f >= GIB {
        format!("{:.1} GiB", bytes_f / GIB)
    } else if bytes_f >= MIB {
        format!("{:.1} MiB", bytes_f / MIB)
    } else if bytes_f >= KIB {
        format!("{:.1} KiB", bytes_f / KIB)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every UI surface renders these on one line, so the summary must stay
    /// short and never contain a newline.
    #[test]
    fn summarize_update_error_is_always_one_short_line() {
        let inputs = [
            "Update failed: Failed to install /home/u/.jcode/builds/versions/0.1.0/jcode\n\nCaused by:\n    Permission denied (os error 13)",
            "cargo build failed: error[E0308]: mismatched types\n  --> src/lib.rs:1:1",
            "Failed to read local package jcode-update.tar.gz: No such file or directory (os error 2)",
            "Failed to install /home/u/.jcode/builds/versions/0.1.0/jcode: Permission denied (os error 13)",
            "a very long single clause with no recognizable cause that just keeps going and going well past any sensible terminal width",
            "",
        ];
        for input in inputs {
            let summary = summarize_update_error(input);
            assert!(!summary.contains('\n'), "multi-line summary for {input:?}");
            assert!(!summary.is_empty(), "empty summary for {input:?}");
            assert!(
                summary.chars().count() <= UPDATE_ERROR_SUMMARY_MAX_CHARS,
                "summary too long ({}) for {input:?}: {summary}",
                summary.chars().count()
            );
        }
    }

    #[test]
    fn summarize_update_error_maps_known_causes() {
        assert_eq!(
            summarize_update_error("Update failed: dns error: failed to lookup address"),
            "no network connection"
        );
        assert_eq!(
            summarize_update_error("Check failed: operation timed out"),
            "connection timed out"
        );
        assert_eq!(
            summarize_update_error("Failed to install /x: Permission denied (os error 13)"),
            "no permission to install the update"
        );
    }

    #[test]
    fn summarize_update_error_strips_stacked_wrappers() {
        assert_eq!(
            summarize_update_error("Update failed: Update check failed: something odd happened"),
            "something odd happened"
        );
    }

    /// The merge affordance matches the divergence summary verbatim, so
    /// summarizing must not rewrite it.
    #[test]
    fn summarize_update_error_preserves_divergence_summary() {
        assert_eq!(
            summarize_update_error(GIT_PULL_DIVERGED_SUMMARY),
            GIT_PULL_DIVERGED_SUMMARY
        );
        assert_eq!(
            summarize_update_error(&format!("Update failed: {GIT_PULL_DIVERGED_SUMMARY}")),
            GIT_PULL_DIVERGED_SUMMARY
        );
        assert!(summary_is_divergence(&summarize_update_error(
            "fatal: Need to specify how to reconcile divergent branches."
        )));
    }

    #[test]
    fn progress_bar_known_total() {
        let text = format_download_progress_bar(DownloadProgress {
            downloaded: 512,
            total: Some(1024),
        });
        assert!(text.contains("50%"));
        assert!(text.contains("512 B/1.0 KiB"));
    }

    #[test]
    fn progress_bar_unknown_total() {
        let text = format_download_progress_bar(DownloadProgress {
            downloaded: 2048,
            total: None,
        });
        assert_eq!(text, "Downloading update... 2.0 KiB downloaded");
    }

    #[test]
    fn git_pull_failure_summaries_are_stable() {
        assert_eq!(
            summarize_git_pull_failure(
                b"fatal: Need to specify how to reconcile divergent branches\n"
            ),
            GIT_PULL_DIVERGED_SUMMARY
        );
        assert!(summary_is_divergence(&summarize_git_pull_failure(
            b"fatal: Need to specify how to reconcile divergent branches\n"
        )));
        assert_eq!(
            summarize_git_pull_failure(b"hint: ignore me\nfatal: no upstream\n"),
            "git pull failed: no upstream"
        );
        assert!(!summary_is_divergence(&summarize_git_pull_failure(
            b"hint: ignore me\nfatal: no upstream\n"
        )));
    }
}
