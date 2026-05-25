#![cfg_attr(test, allow(dead_code))]

use crate::single_session::{GitHubIssueBrowserState, GitHubIssuePreview, GitHubIssueVisualState};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) const ISSUE_CACHE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct GitHubIssueCache {
    #[serde(default = "default_schema_version")]
    pub(crate) schema_version: u32,
    pub(crate) repo: String,
    #[serde(default)]
    pub(crate) synced_at: Option<String>,
    #[serde(default)]
    pub(crate) issues: Vec<CachedGitHubIssue>,
    #[serde(default)]
    pub(crate) local_overrides: Vec<CachedGitHubIssueOverride>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct CachedGitHubIssue {
    pub(crate) number: u64,
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) body: Option<String>,
    #[serde(default)]
    pub(crate) labels: Vec<CachedGitHubLabel>,
    #[serde(default)]
    pub(crate) comments: Vec<CachedGitHubComment>,
    #[serde(default)]
    pub(crate) state: Option<String>,
    #[serde(default)]
    pub(crate) created_at: Option<String>,
    #[serde(default)]
    pub(crate) updated_at: Option<String>,
    #[serde(default)]
    pub(crate) assignees: Vec<String>,
    #[serde(default)]
    pub(crate) milestone: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct CachedGitHubLabel {
    pub(crate) name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct CachedGitHubComment {
    #[serde(default)]
    pub(crate) author: Option<String>,
    pub(crate) body: String,
    #[serde(default)]
    pub(crate) created_at: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct CachedGitHubIssueOverride {
    pub(crate) number: u64,
    #[serde(default)]
    pub(crate) priority: Option<String>,
    #[serde(default)]
    pub(crate) pinned: bool,
}

fn default_schema_version() -> u32 {
    ISSUE_CACHE_SCHEMA_VERSION
}

pub(crate) fn load_current_repo_issue_browser() -> Result<Option<GitHubIssueBrowserState>> {
    let Some(repo) = detect_current_github_repo()? else {
        return Ok(None);
    };
    match load_issue_browser_for_repo(&repo) {
        Ok(browser) => Ok(Some(browser)),
        Err(error) if is_missing_cache_error(&error) => Ok(None),
        Err(error) => Err(error),
    }
}

fn is_missing_cache_error(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound)
    })
}

pub(crate) fn load_issue_browser_for_repo(repo: &str) -> Result<GitHubIssueBrowserState> {
    let path = issue_cache_path_for_repo(repo);
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read GitHub issue cache {}", path.display()))?;
    let cache: GitHubIssueCache = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse GitHub issue cache {}", path.display()))?;
    Ok(issue_browser_from_cache(cache))
}

#[allow(dead_code)]
pub(crate) fn write_issue_cache(cache: &GitHubIssueCache) -> Result<PathBuf> {
    let path = issue_cache_path_for_repo(&cache.repo);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create GitHub issue cache dir {}",
                parent.display()
            )
        })?;
    }
    let raw =
        serde_json::to_string_pretty(cache).context("failed to serialize GitHub issue cache")?;
    std::fs::write(&path, raw)
        .with_context(|| format!("failed to write GitHub issue cache {}", path.display()))?;
    Ok(path)
}

pub(crate) fn issue_cache_path_for_repo(repo: &str) -> PathBuf {
    issue_cache_root().join(format!("{}.json", repo_cache_key(repo)))
}

pub(crate) fn issue_cache_root() -> PathBuf {
    if let Some(path) = std::env::var_os("JCODE_DESKTOP_ISSUE_CACHE_DIR") {
        return PathBuf::from(path);
    }
    jcode_data_dir().join("desktop/github/issues")
}

fn jcode_data_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("JCODE_HOME") {
        return PathBuf::from(path);
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".jcode");
    }
    PathBuf::from(".jcode")
}

fn repo_cache_key(repo: &str) -> String {
    repo.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

pub(crate) fn detect_current_github_repo() -> Result<Option<String>> {
    detect_github_repo_from_dir(&std::env::current_dir().context("failed to get current dir")?)
}

pub(crate) fn detect_github_repo_from_dir(dir: &Path) -> Result<Option<String>> {
    let output = Command::new("git")
        .args(["-C"])
        .arg(dir)
        .args(["remote", "get-url", "origin"])
        .output();
    let Ok(output) = output else {
        return Ok(None);
    };
    if !output.status.success() {
        return Ok(None);
    }
    let remote = String::from_utf8_lossy(&output.stdout);
    Ok(parse_github_repo_from_remote(remote.trim()))
}

pub(crate) fn parse_github_repo_from_remote(remote: &str) -> Option<String> {
    let remote = remote.trim().trim_end_matches(".git");
    if let Some(rest) = remote.strip_prefix("git@github.com:") {
        return normalize_repo(rest);
    }
    if let Some(rest) = remote.strip_prefix("ssh://git@github.com/") {
        return normalize_repo(rest);
    }
    for prefix in ["https://github.com/", "http://github.com/"] {
        if let Some(rest) = remote.strip_prefix(prefix) {
            return normalize_repo(rest);
        }
    }
    None
}

fn normalize_repo(raw: &str) -> Option<String> {
    let mut parts = raw.split('/');
    let owner = parts.next()?.trim();
    let name = parts.next()?.trim();
    if owner.is_empty() || name.is_empty() || parts.next().is_some() {
        return None;
    }
    Some(format!("{owner}/{name}"))
}

pub(crate) fn issue_browser_from_cache(cache: GitHubIssueCache) -> GitHubIssueBrowserState {
    let override_by_number = cache
        .local_overrides
        .iter()
        .map(|override_| (override_.number, override_))
        .collect::<HashMap<_, _>>();
    let mut ranked = cache
        .issues
        .into_iter()
        .filter(|issue| {
            issue
                .state
                .as_deref()
                .unwrap_or("open")
                .eq_ignore_ascii_case("open")
        })
        .map(|issue| {
            let override_ = override_by_number.get(&issue.number).copied();
            let priority = issue_priority(&issue, override_);
            let score = issue_priority_score(&issue, override_);
            let preview = cached_issue_to_preview(&cache.repo, issue, priority, score, override_);
            (
                priority_rank(&preview.priority),
                std::cmp::Reverse(score),
                std::cmp::Reverse(preview.number),
                preview,
            )
        })
        .collect::<Vec<_>>();
    ranked.sort_by_key(|(rank, score, number, _)| (*rank, *score, *number));
    let mut issues = ranked
        .into_iter()
        .map(|(_, _, _, preview)| preview)
        .collect::<Vec<_>>();
    if let Some(first) = issues.first_mut() {
        first.state = GitHubIssueVisualState::Selected;
    }
    let sync_label = cache
        .synced_at
        .unwrap_or_else(|| "unsynced cache".to_string());
    GitHubIssueBrowserState {
        repo: cache.repo,
        filter_label: format!("priority · open · cached {sync_label}"),
        selected: 0,
        list_scroll: 0,
        preview_scroll: 0,
        issues,
    }
}

fn cached_issue_to_preview(
    _repo: &str,
    issue: CachedGitHubIssue,
    priority: String,
    score: i32,
    override_: Option<&CachedGitHubIssueOverride>,
) -> GitHubIssuePreview {
    let labels = issue
        .labels
        .into_iter()
        .map(|label| label.name)
        .collect::<Vec<_>>();
    let body_lines = split_preview_lines(issue.body.unwrap_or_default(), 10);
    let comment_lines = issue
        .comments
        .into_iter()
        .rev()
        .take(4)
        .map(|comment| match comment.author {
            Some(author) if !author.is_empty() => {
                format!("{author}: {}", compact_line(&comment.body))
            }
            _ => compact_line(&comment.body),
        })
        .collect::<Vec<_>>();
    let age = issue
        .updated_at
        .or(issue.created_at)
        .map(|value| format!("updated {value}"))
        .unwrap_or_else(|| "cached".to_string());
    let mut reason = issue_priority_reason(&priority, score, &labels);
    if override_.is_some_and(|override_| override_.priority.is_some()) {
        reason.push_str(" · local override");
    }
    GitHubIssuePreview {
        number: issue.number,
        priority,
        title: issue.title,
        labels,
        age,
        comments: comment_lines.len() as u32,
        state: GitHubIssueVisualState::Idle,
        body_lines,
        comment_lines,
        priority_reason: reason,
    }
}

fn split_preview_lines(text: String, limit: usize) -> Vec<String> {
    let lines = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(limit)
        .map(compact_line)
        .collect::<Vec<_>>();
    if lines.is_empty() {
        vec!["No cached issue body yet. Refresh the issue cache to pull full context.".to_string()]
    } else {
        lines
    }
}

fn compact_line(line: &str) -> String {
    let mut compact = line.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.len() > 240 {
        compact.truncate(237);
        compact.push_str("...");
    }
    compact
}

fn issue_priority(
    issue: &CachedGitHubIssue,
    override_: Option<&CachedGitHubIssueOverride>,
) -> String {
    if let Some(priority) = override_.and_then(|override_| override_.priority.as_deref()) {
        if let Some(normalized) = normalize_priority(priority) {
            return normalized.to_string();
        }
    }
    let labels = issue_label_names(issue);
    if labels.iter().any(|label| {
        matches!(
            label.as_str(),
            "p0" | "priority:p0" | "priority:critical" | "critical" | "sev0"
        )
    }) {
        return "P0".to_string();
    }
    if labels.iter().any(|label| {
        matches!(
            label.as_str(),
            "p1" | "priority:p1" | "priority:high" | "high" | "sev1"
        )
    }) {
        return "P1".to_string();
    }
    if labels
        .iter()
        .any(|label| label.contains("regression") || label.contains("crash"))
        && labels.iter().any(|label| label.contains("bug"))
    {
        return "P1".to_string();
    }
    "P2".to_string()
}

fn normalize_priority(priority: &str) -> Option<&'static str> {
    match priority.trim().to_ascii_lowercase().as_str() {
        "p0" | "0" | "critical" => Some("P0"),
        "p1" | "1" | "high" => Some("P1"),
        "p2" | "2" | "medium" | "normal" | "low" => Some("P2"),
        _ => None,
    }
}

fn issue_priority_score(
    issue: &CachedGitHubIssue,
    override_: Option<&CachedGitHubIssueOverride>,
) -> i32 {
    let labels = issue_label_names(issue);
    let text = format!(
        "{} {}",
        issue.title,
        issue.body.as_deref().unwrap_or_default()
    )
    .to_ascii_lowercase();
    let mut score = 0;
    if override_.is_some_and(|override_| override_.pinned) {
        score += 50;
    }
    for label in &labels {
        if label.contains("regression") {
            score += 18;
        }
        if label.contains("crash") || label.contains("panic") {
            score += 16;
        }
        if label.contains("bug") {
            score += 10;
        }
        if label.contains("desktop") {
            score += 4;
        }
    }
    for keyword in [
        "regression",
        "crash",
        "panic",
        "data loss",
        "hang",
        "broken",
    ] {
        if text.contains(keyword) {
            score += 5;
        }
    }
    score += (issue.comments.len() as i32).min(10);
    if issue
        .milestone
        .as_deref()
        .is_some_and(|milestone| !milestone.is_empty())
    {
        score += 3;
    }
    if !issue.assignees.is_empty() {
        score += 2;
    }
    score
}

fn issue_label_names(issue: &CachedGitHubIssue) -> Vec<String> {
    issue
        .labels
        .iter()
        .map(|label| label.name.trim().to_ascii_lowercase())
        .collect()
}

fn priority_rank(priority: &str) -> u8 {
    match priority {
        "P0" => 0,
        "P1" => 1,
        _ => 2,
    }
}

fn issue_priority_reason(priority: &str, score: i32, labels: &[String]) -> String {
    let label_summary = if labels.is_empty() {
        "no labels".to_string()
    } else {
        labels.join(",")
    };
    format!("{priority} from labels/signals ({label_summary}), score {score}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue(number: u64, title: &str, labels: &[&str], comments: usize) -> CachedGitHubIssue {
        CachedGitHubIssue {
            number,
            title: title.to_string(),
            body: Some(format!("body for {title}")),
            labels: labels
                .iter()
                .map(|name| CachedGitHubLabel {
                    name: (*name).to_string(),
                })
                .collect(),
            comments: (0..comments)
                .map(|index| CachedGitHubComment {
                    author: Some(format!("user{index}")),
                    body: format!("comment {index}"),
                    created_at: None,
                })
                .collect(),
            state: Some("open".to_string()),
            created_at: Some("2026-05-01".to_string()),
            updated_at: None,
            assignees: Vec::new(),
            milestone: None,
        }
    }

    #[test]
    fn parses_common_github_remote_urls() {
        assert_eq!(
            parse_github_repo_from_remote("git@github.com:1jehuang/jcode.git").as_deref(),
            Some("1jehuang/jcode")
        );
        assert_eq!(
            parse_github_repo_from_remote("https://github.com/owner/repo.git").as_deref(),
            Some("owner/repo")
        );
        assert_eq!(
            parse_github_repo_from_remote("ssh://git@github.com/owner/repo").as_deref(),
            Some("owner/repo")
        );
        assert!(parse_github_repo_from_remote("https://example.com/owner/repo").is_none());
    }

    #[test]
    fn ranks_explicit_priority_and_local_overrides_first() {
        let cache = GitHubIssueCache {
            schema_version: ISSUE_CACHE_SCHEMA_VERSION,
            repo: "owner/repo".to_string(),
            synced_at: Some("now".to_string()),
            issues: vec![
                issue(10, "normal cleanup", &["enhancement"], 0),
                issue(11, "crash regression", &["bug", "regression"], 1),
                issue(12, "user override", &["docs"], 0),
            ],
            local_overrides: vec![CachedGitHubIssueOverride {
                number: 12,
                priority: Some("P0".to_string()),
                pinned: true,
            }],
        };
        let browser = issue_browser_from_cache(cache);
        let numbers = browser
            .issues
            .iter()
            .map(|issue| issue.number)
            .collect::<Vec<_>>();
        assert_eq!(numbers, vec![12, 11, 10]);
        assert_eq!(browser.issues[0].priority, "P0");
        assert_eq!(browser.issues[1].priority, "P1");
        assert_eq!(browser.issues[0].state, GitHubIssueVisualState::Selected);
    }

    #[test]
    fn filters_closed_issues_and_is_deterministic() {
        let mut closed = issue(20, "closed", &["P0"], 10);
        closed.state = Some("closed".to_string());
        let cache = GitHubIssueCache {
            schema_version: ISSUE_CACHE_SCHEMA_VERSION,
            repo: "owner/repo".to_string(),
            synced_at: None,
            issues: vec![
                issue(1, "same score low number", &["bug"], 0),
                issue(2, "same score high number", &["bug"], 0),
                closed,
            ],
            local_overrides: Vec::new(),
        };
        let browser = issue_browser_from_cache(cache);
        assert_eq!(
            browser
                .issues
                .iter()
                .map(|issue| issue.number)
                .collect::<Vec<_>>(),
            vec![2, 1]
        );
        assert!(browser.filter_label.contains("cached unsynced cache"));
    }
}
