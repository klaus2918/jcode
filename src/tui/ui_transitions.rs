use super::theme_support::color_to_floats;
use super::*;
use crate::tui::color_support::rgb;

#[cfg(test)]
pub(crate) fn inline_ui_gap_height(app: &dyn TuiState) -> u16 {
    if app.inline_ui_state().is_some() {
        1
    } else {
        0
    }
}

#[cfg(test)]
pub(crate) fn extract_line_text(line: &Line) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

fn extract_line_styled_chars(line: &Line) -> Vec<(char, Style)> {
    let mut chars = Vec::new();
    for span in &line.spans {
        for ch in span.content.chars() {
            chars.push((ch, span.style));
        }
    }
    chars
}

pub(super) fn morph_lines_to_header(
    anim_lines: &[Line<'static>],
    header_lines: &[Line<'static>],
    morph_t: f32,
    width: u16,
) -> Vec<Line<'static>> {
    let blend = ((morph_t - 0.6) / 0.35).clamp(0.0, 1.0);
    let max_rows = anim_lines.len().max(header_lines.len());
    let w = width as usize;

    let mut result = Vec::with_capacity(max_rows);

    let anim_row_count = anim_lines.len();
    let header_row_count = header_lines.len();
    let row_blend = blend * blend;
    let target_rows =
        anim_row_count as f32 + (header_row_count as f32 - anim_row_count as f32) * row_blend;
    let output_rows = target_rows.round() as usize;

    for out_row in 0..output_rows {
        let anim_row_f = if output_rows > 1 {
            out_row as f32 / (output_rows - 1) as f32 * (anim_row_count.max(1) - 1) as f32
        } else {
            0.0
        };
        let header_row_f = if output_rows > 1 {
            out_row as f32 / (output_rows - 1) as f32 * (header_row_count.max(1) - 1) as f32
        } else {
            0.0
        };

        let anim_idx = (anim_row_f.round() as usize).min(anim_row_count.saturating_sub(1));
        let header_idx = (header_row_f.round() as usize).min(header_row_count.saturating_sub(1));

        let anim_chars: Vec<(char, Style)> = if anim_idx < anim_row_count {
            extract_line_styled_chars(&anim_lines[anim_idx])
        } else {
            Vec::new()
        };
        let header_chars: Vec<(char, Style)> = if header_idx < header_row_count {
            extract_line_styled_chars(&header_lines[header_idx])
        } else {
            Vec::new()
        };

        let anim_text: String = anim_chars.iter().map(|(c, _)| *c).collect();
        let header_text: String = header_chars.iter().map(|(c, _)| *c).collect();
        let anim_trimmed = anim_text.trim();
        let header_trimmed = header_text.trim();

        let anim_start = anim_text.find(anim_trimmed).unwrap_or(0);
        let header_start = header_text.find(header_trimmed).unwrap_or(0);

        let anim_center = if !anim_trimmed.is_empty() {
            anim_start as f32 + anim_trimmed.len() as f32 / 2.0
        } else {
            w as f32 / 2.0
        };
        let header_center = if !header_trimmed.is_empty() {
            header_start as f32 + header_trimmed.len() as f32 / 2.0
        } else {
            w as f32 / 2.0
        };

        let center = anim_center + (header_center - anim_center) * blend;
        let max_col = anim_chars.len().max(header_chars.len()).max(w);

        let mut spans: Vec<Span<'static>> = Vec::new();

        for col in 0..max_col {
            let anim_ch = anim_chars.get(col).map(|(c, _)| *c).unwrap_or(' ');
            let anim_style = anim_chars.get(col).map(|(_, s)| *s).unwrap_or_default();
            let header_ch = header_chars.get(col).map(|(c, _)| *c).unwrap_or(' ');
            let header_style = header_chars.get(col).map(|(_, s)| *s).unwrap_or_default();

            let dist_from_center = ((col as f32) - center).abs() / (w as f32 / 2.0).max(1.0);
            let flip_hash = {
                let mut h = DefaultHasher::new();
                out_row.hash(&mut h);
                col.hash(&mut h);
                (std::hash::Hasher::finish(&h) % 1000) as f32 / 1000.0
            };
            let flip_threshold = (0.3 + dist_from_center * 0.4 + flip_hash * 0.3).clamp(0.0, 1.0);

            let (ch, style) = if blend >= flip_threshold {
                let style_blend = ((blend - flip_threshold) / 0.15).clamp(0.0, 1.0);
                if style_blend < 0.3 {
                    let glitch_chars = b"@#$%&*!?~=+<>";
                    let gi = {
                        let mut h = DefaultHasher::new();
                        out_row.hash(&mut h);
                        col.hash(&mut h);
                        ((blend * 100.0) as u32).hash(&mut h);
                        (std::hash::Hasher::finish(&h) % glitch_chars.len() as u64) as usize
                    };
                    let gc = glitch_chars[gi] as char;
                    (gc, lerp_style(anim_style, header_style, style_blend))
                } else {
                    (header_ch, lerp_style(anim_style, header_style, style_blend))
                }
            } else {
                let fade = (1.0 - blend / flip_threshold.max(0.01)).clamp(0.0, 1.0);
                let mut s = anim_style;
                if let Some(fg) = s.fg {
                    let (r, g, b) = color_to_floats(fg, (80.0, 80.0, 80.0));
                    s.fg = Some(rgb((r * fade) as u8, (g * fade) as u8, (b * fade) as u8));
                }
                (anim_ch, s)
            };

            spans.push(Span::styled(ch.to_string(), style));
        }

        let align = header_lines
            .get(header_idx)
            .and_then(|l| l.alignment)
            .or_else(|| anim_lines.get(anim_idx).and_then(|l| l.alignment))
            .unwrap_or(ratatui::layout::Alignment::Center);

        result.push(Line::from(spans).alignment(align));
    }

    result
}

fn lerp_style(from: Style, to: Style, t: f32) -> Style {
    let fg = match (from.fg, to.fg) {
        (Some(f), Some(toc)) => {
            let (r1, g1, b1) = color_to_floats(f, (80.0, 80.0, 80.0));
            let (r2, g2, b2) = color_to_floats(toc, (200.0, 200.0, 200.0));
            Some(rgb(
                (r1 + (r2 - r1) * t).clamp(0.0, 255.0) as u8,
                (g1 + (g2 - g1) * t).clamp(0.0, 255.0) as u8,
                (b1 + (b2 - b1) * t).clamp(0.0, 255.0) as u8,
            ))
        }
        (Some(f), _) => {
            let (r, g, b) = color_to_floats(f, (80.0, 80.0, 80.0));
            let dim = 1.0 - t;
            Some(rgb((r * dim) as u8, (g * dim) as u8, (b * dim) as u8))
        }
        (_, Some(toc)) => {
            let (r, g, b) = color_to_floats(toc, (200.0, 200.0, 200.0));
            Some(rgb((r * t) as u8, (g * t) as u8, (b * t) as u8))
        }
        (_, to_fg) => to_fg,
    };
    let mut s = to;
    s.fg = fg;
    s
}
