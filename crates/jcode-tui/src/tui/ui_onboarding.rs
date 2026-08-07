//! First-run onboarding welcome screen.
//!
//! Rendered in place of the normal empty-state transcript when
//! `TuiState::onboarding_welcome_active()` is true (brand-new install /
//! unauthenticated / new user, or `/onboarding-preview`).
//!
//! Layout, top to bottom, vertically centered in the chat area:
//!   1. Grayed telemetry notice header.
//!   2. The animated donut (attention grab).
//!   3. "Welcome to jcode onboarding" title.
//!   4. The login / getting-started prompt with suggestions.
//!
//! The donut is drawn as a live widget (not part of the cached transcript) so
//! it animates every frame, matching the idle-donut behavior elsewhere.

use super::animations;
use super::dim_color;
use crate::tui::TuiState;
use crate::tui::color_support::rgb;
use ratatui::{prelude::*, widgets::Paragraph};

const DONUT_HEIGHT: u16 = 18;
const GAP: u16 = 1;

/// Accent color for the welcome title.
fn welcome_accent() -> Color {
    rgb(138, 180, 248)
}

/// Append the universal "Esc to skip" hint shown on every guided onboarding
/// phase. This advertises the escape hatch that guarantees the user can always
/// leave onboarding (see `handle_onboarding_continue_prompt_key`), so a first-
/// run user is never visibly trapped. Kept dim and on its own line so it never
/// competes with the primary action.
fn push_esc_skip_hint(lines: &mut Vec<Line<'static>>, align: Alignment) {
    lines.push(Line::from(""));
    lines.push(
        Line::from(Span::styled(
            "Esc to skip onboarding (connect a provider later with `jcode provider add`).",
            Style::default().fg(dim_color()),
        ))
        .alignment(align),
    );
}

/// Whether the terminal renders the U+25D6/U+25D7 half-circle pill caps as
/// clean full-cell semicircles. Kitty does; ghostty, Apple Terminal, and the
/// VS Code terminal draw them as small floating glyphs that break the capsule
/// illusion, so those fall back to half-block caps that render solidly
/// everywhere. Override with `JCODE_ROUNDED_PILLS=on|off`.
fn rounded_pill_caps_supported() -> bool {
    static SUPPORTED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *SUPPORTED.get_or_init(|| {
        // Golden/eval tests assert the canonical rounded glyphs; keep test
        // renders deterministic regardless of the terminal running the tests.
        if cfg!(test) {
            return true;
        }
        if let Ok(raw) = std::env::var("JCODE_ROUNDED_PILLS") {
            match raw.trim().to_ascii_lowercase().as_str() {
                "1" | "true" | "yes" | "on" => return true,
                "0" | "false" | "no" | "off" => return false,
                _ => {}
            }
        }
        if let Ok(tp) = std::env::var("TERM_PROGRAM") {
            let tp = tp.to_ascii_lowercase();
            if tp == "ghostty" || tp == "apple_terminal" || tp == "vscode" {
                return false;
            }
        }
        if std::env::var("GHOSTTY_RESOURCES_DIR").is_ok()
            || std::env::var("GHOSTTY_BIN_DIR").is_ok()
        {
            return false;
        }
        if let Ok(term) = std::env::var("TERM")
            && term.to_ascii_lowercase().contains("ghostty")
        {
            return false;
        }
        true
    })
}

/// Build one rounded "lozenge" pill: half-circle end caps (`◖` / `◗`) around a
/// padded label. Both states are solid capsules; the selected pill has a bright
/// accent fill + BOLD label, the unselected one a muted dark-gray fill with no
/// bold. The BOLD-vs-not-bold contrast is a non-color attribute, so the
/// selection survives on monochrome terminals (Tier 10 color-independence).
fn lozenge_pill_spans(label: &str, filled: bool) -> Vec<Span<'static>> {
    // Both states are solid capsules (the ◖/◗ caps are filled half-circles, so a
    // "hollow" outline reads as stray half-moons). The selected pill uses the
    // bright accent fill + BOLD label; the unselected one a muted dark-gray fill
    // with no bold. The BOLD-vs-not contrast is a non-color attribute, so the
    // selection survives on monochrome terminals (Tier 10 color-independence).
    let (fill, text_fg, bold) = if filled {
        (welcome_accent(), rgb(20, 24, 32), true)
    } else {
        (rgb(58, 62, 70), rgb(170, 174, 182), false)
    };

    let cap = Style::default().fg(fill);
    let mut body = Style::default().fg(text_fg).bg(fill);
    if bold {
        body = body.add_modifier(Modifier::BOLD);
    }
    // Half-circle caps where they render well; half-block caps (`▐` / `▌`)
    // elsewhere. Half blocks are part of the universally-supported block
    // elements range, so the capsule stays a solid, aligned shape in every
    // terminal font (the corners are square, but nothing floats or clips).
    let (left_cap, right_cap) = if rounded_pill_caps_supported() {
        ("\u{25D6}", "\u{25D7}")
    } else {
        ("\u{2590}", "\u{258C}")
    };
    vec![
        Span::styled(left_cap, cap),
        Span::styled(format!(" {label} "), body),
        Span::styled(right_cap, cap),
    ]
}

/// Build the Yes/No selector as a pair of rounded lozenge pills. The selected
/// option is a bright filled pill; the other is a muted dark capsule. The shape
/// and fill carry the selection visually so no instruction sentence is needed.
fn yes_no_pill_line(yes_highlighted: bool, align: Alignment) -> Line<'static> {
    let mut spans = Vec::new();
    spans.extend(lozenge_pill_spans("Yes", yes_highlighted));
    spans.push(Span::raw("   "));
    spans.extend(lozenge_pill_spans("No", !yes_highlighted));
    Line::from(spans).alignment(align)
}

/// Welcome title line, rendered just above the donut.
fn welcome_title_line() -> Line<'static> {
    Line::from(Span::styled(
        "Welcome to jcode onboarding",
        Style::default()
            .fg(welcome_accent())
            .add_modifier(Modifier::BOLD),
    ))
    .alignment(Alignment::Center)
}

/// Short keyboard hint rendered just below the donut on guided phases. Replaces
/// the old multi-line instruction prose: the interactive pills/rows already show
/// what is selectable, so a one-liner is enough.
fn keyboard_hint_line() -> Line<'static> {
    Line::from(Span::styled(
        "Use your keyboard to navigate.",
        Style::default().fg(dim_color()),
    ))
    .alignment(Alignment::Center)
}

/// The phase-specific body of the welcome screen (everything below the donut and
/// keyboard hint). The title now lives above the donut, so this no longer emits
/// it.
fn welcome_body_lines(app: &dyn TuiState) -> Vec<Line<'static>> {
    let align = Alignment::Center;
    let mut lines: Vec<Line<'static>> = Vec::new();

    use crate::tui::OnboardingWelcomeKind;
    match app.onboarding_welcome_kind() {
        OnboardingWelcomeKind::ConfigureProvider { yes_highlighted } => {
            lines.push(
                Line::from(Span::styled(
                    "Configure a model provider?",
                    Style::default()
                        .fg(welcome_accent())
                        .add_modifier(Modifier::BOLD),
                ))
                .alignment(align),
            );
            lines.push(Line::from(""));

            // Rounded Yes/No lozenge pills; the selection is shown visually (the
            // filled capsule), so no instruction sentence is needed.
            lines.push(yes_no_pill_line(yes_highlighted, align));
            // The Esc hint below already says you can log in later with /login,
            // so we don't repeat a "choose No to skip" line here.
            push_esc_skip_hint(&mut lines, align);
            return lines;
        }
        OnboardingWelcomeKind::Suggestions => {}
    }

    let suggestions = app.suggestion_prompts();
    if !suggestions.is_empty() {
        lines.push(Line::from(""));
        for (i, (label, prompt)) in suggestions.iter().enumerate() {
            let is_login = prompt.starts_with('/');
            let spans = if is_login {
                vec![
                    Span::styled(
                        format!("{} ", label),
                        Style::default()
                            .fg(welcome_accent())
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("(type {})", prompt),
                        Style::default().fg(dim_color()),
                    ),
                ]
            } else {
                vec![
                    Span::styled(
                        format!("[{}] ", i + 1),
                        Style::default().fg(welcome_accent()),
                    ),
                    Span::styled(label.clone(), Style::default().fg(rgb(200, 200, 200))),
                ]
            };
            lines.push(Line::from(spans).alignment(align));
        }
        if suggestions.len() > 1 {
            lines.push(Line::from(""));
            lines.push(
                Line::from(Span::styled(
                    format!("Press 1-{} or type anything to start", suggestions.len()),
                    Style::default().fg(dim_color()),
                ))
                .alignment(align),
            );
        }
    }

    lines
}

/// Draw the full onboarding welcome screen into `area`.
///
/// Vertical structure (top to bottom):
///   telemetry header, gap, title, donut, keyboard hint, gap, phase body.
/// The title sits directly above the donut and a one-line keyboard hint sits
/// directly below it, so the phase body underneath can stay lean.
pub(super) fn draw_onboarding_welcome(frame: &mut Frame, app: &dyn TuiState, area: Rect) {
    if area.width < 4 || area.height < 6 {
        // Too small for the full treatment: fall back to a minimal welcome.
        let mut lines = vec![welcome_title_line()];
        lines.extend(welcome_body_lines(app));
        frame.render_widget(Paragraph::new(lines), area);
        return;
    }

    let body = welcome_body_lines(app);
    let body_h = body.len() as u16;
    // Title above the donut, keyboard hint below it. Both are single lines and
    // only shown when there is room for the donut treatment.
    const TITLE_H: u16 = 1;
    const HINT_H: u16 = 1;

    // Donut shrinks if the area is short so the welcome text always fits. The
    // title + hint lines that hug the donut are part of the reserved chrome.
    let donut_h = DONUT_HEIGHT.min(
        area.height
            .saturating_sub(TITLE_H + HINT_H + body_h + GAP * 2 + 1),
    );
    let show_donut_block = donut_h > 0;

    let used = if show_donut_block {
        GAP + TITLE_H + donut_h + HINT_H + GAP + body_h
    } else {
        GAP + body_h
    };
    let pad_top = area.height.saturating_sub(used) / 2;

    let mut constraints = vec![Constraint::Length(pad_top)];
    if show_donut_block {
        constraints.push(Constraint::Length(GAP));
        constraints.push(Constraint::Length(TITLE_H));
        constraints.push(Constraint::Length(donut_h));
        constraints.push(Constraint::Length(HINT_H));
    }
    constraints.push(Constraint::Length(GAP));
    constraints.push(Constraint::Length(body_h));
    constraints.push(Constraint::Min(0));

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    // chunks[0] = top pad, then optional gap/title/donut/hint, gap, body.
    let mut idx = 1;
    if show_donut_block {
        idx += 1; // skip gap chunk
        frame.render_widget(
            Paragraph::new(welcome_title_line()).alignment(Alignment::Center),
            chunks[idx],
        );
        idx += 1; // title -> donut
        animations::draw_idle_animation(frame, app, chunks[idx]);
        idx += 1; // donut -> hint
        frame.render_widget(
            Paragraph::new(keyboard_hint_line()).alignment(Alignment::Center),
            chunks[idx],
        );
        idx += 1; // hint -> gap
    }
    idx += 1; // skip gap chunk
    frame.render_widget(
        Paragraph::new(body).alignment(Alignment::Center),
        chunks[idx],
    );
}
