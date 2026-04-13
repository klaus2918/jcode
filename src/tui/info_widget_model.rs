use super::text::{truncate_chars, truncate_smart};
use super::{AuthMethod, InfoWidgetData};
use crate::tui::color_support::rgb;
use ratatui::prelude::*;

pub(super) fn render_model_widget(data: &InfoWidgetData, inner: Rect) -> Vec<Line<'static>> {
    let Some(model) = &data.model else {
        return Vec::new();
    };

    let mut lines: Vec<Line> = Vec::new();

    let short_name = shorten_model_name(model);
    let max_len = inner.width.saturating_sub(2) as usize;

    let mut spans = vec![
        Span::styled("⚡ ", Style::default().fg(rgb(140, 180, 255))),
        Span::styled(
            truncate_smart(&short_name, max_len.saturating_sub(2)),
            Style::default().fg(rgb(180, 180, 190)).bold(),
        ),
    ];

    if let Some(effort) = &data.reasoning_effort {
        let effort_short = match effort.as_str() {
            "xhigh" => "xhi",
            "high" => "hi",
            "medium" => "med",
            "low" => "lo",
            "none" => "∅",
            other => other,
        };
        spans.push(Span::styled(" ", Style::default()));
        spans.push(Span::styled(
            format!("({})", effort_short),
            Style::default().fg(rgb(255, 200, 100)),
        ));
    }

    if let Some(service_tier) = &data.service_tier {
        let tier_short = match service_tier.as_str() {
            "priority" => "fast",
            "flex" => "flex",
            other => other,
        };
        spans.push(Span::styled(" ", Style::default()));
        spans.push(Span::styled(
            format!("[{}]", tier_short),
            Style::default().fg(rgb(200, 140, 255)).bold(),
        ));
    }

    lines.push(Line::from(spans));

    if data.session_count.is_some() || data.session_name.is_some() {
        let mut parts = Vec::new();

        if let Some(sessions) = data.session_count {
            parts.push(format!(
                "{} session{}",
                sessions,
                if sessions == 1 { "" } else { "s" }
            ));
        }

        if let Some(name) = data.session_name.as_deref()
            && !name.trim().is_empty()
        {
            parts.push(name.to_string());
        }

        if !parts.is_empty() {
            let detail = truncate_smart(&parts.join(" · "), max_len.saturating_sub(2));
            lines.push(Line::from(vec![Span::styled(
                detail,
                Style::default().fg(rgb(140, 140, 150)),
            )]));
        }
    }

    if let Some(provider) = data
        .provider_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let mut provider_spans = vec![
            Span::styled("☁ ", Style::default().fg(rgb(140, 180, 255))),
            Span::styled(
                provider.to_lowercase(),
                Style::default().fg(rgb(140, 180, 255)),
            ),
        ];
        if let Some(upstream) = data.upstream_provider.as_deref().map(str::trim)
            && !upstream.is_empty()
        {
            provider_spans.push(Span::styled(
                " -> ",
                Style::default().fg(rgb(100, 100, 110)),
            ));
            provider_spans.push(Span::styled(
                upstream.to_string(),
                Style::default().fg(rgb(220, 190, 120)),
            ));
        }
        lines.push(Line::from(provider_spans));
    }

    if let Some(connection) = data
        .connection_type
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        lines.push(Line::from(vec![
            Span::styled("↔ ", Style::default().fg(rgb(140, 180, 255))),
            Span::styled(
                connection.to_lowercase(),
                Style::default().fg(rgb(140, 180, 255)),
            ),
        ]));
    }

    if data.auth_method != AuthMethod::Unknown {
        let (icon, label, color) = match data.auth_method {
            AuthMethod::AnthropicOAuth => ("🔐", "OAuth", rgb(255, 160, 100)),
            AuthMethod::AnthropicApiKey => ("🔑", "API Key", rgb(180, 180, 190)),
            AuthMethod::OpenAIOAuth => ("🔐", "OAuth", rgb(100, 200, 180)),
            AuthMethod::OpenAIApiKey => ("🔑", "API Key", rgb(180, 180, 190)),
            AuthMethod::OpenRouterApiKey => ("🔑", "API Key", rgb(140, 180, 255)),
            AuthMethod::CopilotOAuth => ("🔐", "OAuth", rgb(110, 200, 140)),
            AuthMethod::GeminiOAuth => ("🔐", "OAuth", rgb(120, 190, 255)),
            AuthMethod::Unknown => unreachable!(),
        };

        if let Some(ref upstream) = data.upstream_provider {
            lines.push(Line::from(vec![
                Span::styled(format!("{} ", icon), Style::default().fg(color)),
                Span::styled(label, Style::default().fg(rgb(140, 140, 150))),
                Span::styled(" via ", Style::default().fg(rgb(100, 100, 110))),
                Span::styled(upstream.clone(), Style::default().fg(rgb(200, 180, 100))),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled(format!("{} ", icon), Style::default().fg(color)),
                Span::styled(label, Style::default().fg(rgb(140, 140, 150))),
            ]));
        }
    }

    if let Some(tps) = data.tokens_per_second
        && tps.is_finite()
        && tps > 0.1
    {
        lines.push(Line::from(vec![
            Span::styled("⏱ ", Style::default().fg(rgb(140, 180, 255))),
            Span::styled(
                format!("{:.1} t/s", tps),
                Style::default().fg(rgb(140, 140, 150)),
            ),
        ]));
    }

    lines
}

pub(super) fn render_model_info(data: &InfoWidgetData, inner: Rect) -> Vec<Line<'static>> {
    let Some(model) = &data.model else {
        return Vec::new();
    };

    let short_name = shorten_model_name(model);
    let max_len = inner.width.saturating_sub(2) as usize;

    let mut spans = vec![Span::styled(
        if short_name.chars().count() > max_len {
            format!(
                "{}...",
                truncate_chars(&short_name, max_len.saturating_sub(3))
            )
        } else {
            short_name
        },
        Style::default().fg(rgb(180, 180, 190)).bold(),
    )];

    if let Some(effort) = &data.reasoning_effort {
        let effort_short = match effort.as_str() {
            "xhigh" => "xhi",
            "high" => "hi",
            "medium" => "med",
            "low" => "lo",
            "none" => "∅",
            other => other,
        };
        spans.push(Span::styled(" ", Style::default()));
        spans.push(Span::styled(
            format!("({})", effort_short),
            Style::default().fg(rgb(255, 200, 100)),
        ));
    }

    if let Some(mode) = &data.native_compaction_mode {
        let label = if let Some(tokens) = data.native_compaction_threshold_tokens {
            format!("native {} @ {}k", mode, tokens / 1000)
        } else {
            format!("native {}", mode)
        };
        spans.push(Span::styled(" ", Style::default()));
        spans.push(Span::styled(label, Style::default().fg(rgb(120, 210, 230))));
    }

    let mut lines = vec![Line::from(spans)];

    let has_provider = data
        .provider_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .is_some();
    let has_auth = data.auth_method != AuthMethod::Unknown;

    if has_provider || has_auth {
        let mut detail_spans: Vec<Span> = Vec::new();

        if let Some(provider) = data
            .provider_name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            detail_spans.push(Span::styled(
                provider.to_lowercase(),
                Style::default().fg(rgb(140, 180, 255)),
            ));
        }

        if has_auth {
            let (icon, label, _color) = match data.auth_method {
                AuthMethod::AnthropicOAuth => ("🔐", "OAuth", rgb(255, 160, 100)),
                AuthMethod::AnthropicApiKey => ("🔑", "API Key", rgb(180, 180, 190)),
                AuthMethod::OpenAIOAuth => ("🔐", "OAuth", rgb(100, 200, 180)),
                AuthMethod::OpenAIApiKey => ("🔑", "API Key", rgb(180, 180, 190)),
                AuthMethod::OpenRouterApiKey => ("🔑", "API Key", rgb(140, 180, 255)),
                AuthMethod::CopilotOAuth => ("🔐", "OAuth", rgb(110, 200, 140)),
                AuthMethod::GeminiOAuth => ("🔐", "OAuth", rgb(120, 190, 255)),
                AuthMethod::Unknown => unreachable!(),
            };
            if !detail_spans.is_empty() {
                detail_spans.push(Span::styled(" · ", Style::default().fg(rgb(80, 80, 90))));
            }
            detail_spans.push(Span::styled(
                format!("{} {}", icon, label),
                Style::default().fg(rgb(140, 140, 150)),
            ));
        }

        if !detail_spans.is_empty() {
            lines.push(Line::from(detail_spans));
        }
    }

    if data.session_count.is_some() || data.session_name.is_some() {
        let mut parts = Vec::new();

        if let Some(sessions) = data.session_count {
            parts.push(format!(
                "{} session{}",
                sessions,
                if sessions == 1 { "" } else { "s" }
            ));
        }

        if let Some(name) = data.session_name.as_deref()
            && !name.trim().is_empty()
        {
            parts.push(name.to_string());
        }

        if !parts.is_empty() {
            let detail = truncate_smart(&parts.join(" · "), max_len.saturating_sub(2));
            lines.push(Line::from(vec![Span::styled(
                detail,
                Style::default().fg(rgb(140, 140, 150)),
            )]));
        }
    }

    lines
}

pub(super) fn shorten_model_name(model: &str) -> String {
    if model.contains("claude") {
        if model.contains("opus-4-5") || model.contains("opus-4.5") {
            return "opus-4.5".to_string();
        }
        if model.contains("sonnet-4") {
            return "sonnet-4".to_string();
        }
        if model.contains("sonnet-3-5") || model.contains("sonnet-3.5") {
            return "sonnet-3.5".to_string();
        }
        if model.contains("haiku") {
            return "haiku".to_string();
        }
        if let Some(idx) = model.find("claude-") {
            let rest = &model[idx + 7..];
            if let Some(end) = rest.find('-') {
                return rest[..end].to_string();
            }
        }
    }

    if model.contains("gpt")
        && let Some(start) = model.find("gpt-")
    {
        let rest = &model[start..];
        let parts: Vec<&str> = rest.splitn(3, '-').collect();
        if parts.len() >= 2 {
            return format!("{}-{}", parts[0], parts[1]);
        }
    }

    if model.len() > 15 {
        format!("{}…", crate::util::truncate_str(model, 14))
    } else {
        model.to_string()
    }
}
