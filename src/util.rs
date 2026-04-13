/// Truncate a string at a valid UTF-8 character boundary.
///
/// Returns a slice of at most `max_bytes` bytes, ending at a valid char boundary.
/// This prevents panics when truncating strings that contain multi-byte characters.
pub fn truncate_str(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    // Find the largest valid char boundary at or before max_bytes
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

pub const APPROX_CHARS_PER_TOKEN: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApproxTokenSeverity {
    Normal,
    Warning,
    Danger,
}

/// Estimate token count using jcode's existing chars-per-token heuristic.
pub fn estimate_tokens(s: &str) -> usize {
    s.len() / APPROX_CHARS_PER_TOKEN
}

/// Format a number with ASCII thousands separators.
pub fn format_number(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (idx, ch) in digits.chars().enumerate() {
        if idx > 0 && (digits.len() - idx).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// Format a token count in the compact style used by the TUI.
pub fn format_approx_token_count(tokens: usize) -> String {
    match tokens {
        0..=999 => format!("{} tok", tokens),
        1_000..=9_999 => {
            let whole = tokens / 1_000;
            let tenth = (tokens % 1_000) / 100;
            if tenth == 0 {
                format!("{}k tok", whole)
            } else {
                format!("{}.{}k tok", whole, tenth)
            }
        }
        _ => format!("{}k tok", tokens / 1_000),
    }
}

/// Light severity levels for tool outputs that are unusually large for context.
pub fn approx_tool_output_token_severity(tokens: usize) -> ApproxTokenSeverity {
    if tokens >= 12_000 {
        ApproxTokenSeverity::Danger
    } else if tokens >= 4_000 {
        ApproxTokenSeverity::Warning
    } else {
        ApproxTokenSeverity::Normal
    }
}

/// Format an anyhow error including its full cause chain.
///
/// This preserves actionable upstream details such as HTTP status/body instead of
/// only showing the outermost context message.
pub fn format_error_chain(err: &anyhow::Error) -> String {
    let mut parts = Vec::new();
    for cause in err.chain() {
        let text = cause.to_string();
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        if parts.last().is_some_and(|prev: &String| prev == trimmed) {
            continue;
        }
        parts.push(trimmed.to_string());
    }

    match parts.len() {
        0 => "unknown error".to_string(),
        1 => parts.remove(0),
        _ => parts.join(": "),
    }
}

/// Extract the payload from an SSE `data:` line.
///
/// The SSE spec allows an optional single space after the colon, so both
/// `data:{...}` and `data: {...}` are valid and should parse identically.
pub fn sse_data_line(line: &str) -> Option<&str> {
    line.strip_prefix("data:")
        .map(|rest| rest.strip_prefix(' ').unwrap_or(rest))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_error_chain_includes_nested_causes() {
        let err =
            anyhow::anyhow!("HTTP 400: invalid argument").context("Gemini generateContent failed");
        assert_eq!(
            format_error_chain(&err),
            "Gemini generateContent failed: HTTP 400: invalid argument"
        );
    }

    #[test]
    fn test_format_error_chain_deduplicates_repeated_messages() {
        let err = anyhow::anyhow!("same").context("same");
        assert_eq!(format_error_chain(&err), "same");
    }

    #[test]
    fn test_truncate_ascii() {
        assert_eq!(truncate_str("hello", 10), "hello");
        assert_eq!(truncate_str("hello world", 5), "hello");
    }

    #[test]
    fn test_truncate_multibyte() {
        // "学" is 3 bytes (E5 AD A6)
        let s = "abc学def";
        assert_eq!(truncate_str(s, 3), "abc"); // exactly before 学
        assert_eq!(truncate_str(s, 4), "abc"); // mid-char, back up
        assert_eq!(truncate_str(s, 5), "abc"); // mid-char, back up
        assert_eq!(truncate_str(s, 6), "abc学"); // exactly after 学
    }

    #[test]
    fn test_truncate_emoji() {
        // "🦀" is 4 bytes
        let s = "hi🦀bye";
        assert_eq!(truncate_str(s, 2), "hi");
        assert_eq!(truncate_str(s, 3), "hi"); // mid-emoji
        assert_eq!(truncate_str(s, 5), "hi"); // mid-emoji
        assert_eq!(truncate_str(s, 6), "hi🦀");
    }

    #[test]
    fn test_truncate_empty() {
        assert_eq!(truncate_str("", 10), "");
        assert_eq!(truncate_str("hello", 0), "");
    }

    #[test]
    fn test_sse_data_line_accepts_optional_space() {
        assert_eq!(sse_data_line("data: {\"ok\":true}"), Some("{\"ok\":true}"));
        assert_eq!(sse_data_line("data:{\"ok\":true}"), Some("{\"ok\":true}"));
        assert_eq!(sse_data_line("event: message"), None);
    }

    #[test]
    fn test_format_number_adds_commas() {
        assert_eq!(format_number(0), "0");
        assert_eq!(format_number(12), "12");
        assert_eq!(format_number(1_234), "1,234");
        assert_eq!(format_number(12_345_678), "12,345,678");
    }

    #[test]
    fn test_format_approx_token_count_compacts_thousands() {
        assert_eq!(format_approx_token_count(999), "999 tok");
        assert_eq!(format_approx_token_count(1_000), "1k tok");
        assert_eq!(format_approx_token_count(1_900), "1.9k tok");
        assert_eq!(format_approx_token_count(10_000), "10k tok");
    }

    #[test]
    fn test_approx_tool_output_token_severity_thresholds() {
        assert_eq!(
            approx_tool_output_token_severity(3_999),
            ApproxTokenSeverity::Normal
        );
        assert_eq!(
            approx_tool_output_token_severity(4_000),
            ApproxTokenSeverity::Warning
        );
        assert_eq!(
            approx_tool_output_token_severity(11_999),
            ApproxTokenSeverity::Warning
        );
        assert_eq!(
            approx_tool_output_token_severity(12_000),
            ApproxTokenSeverity::Danger
        );
    }
}
