pub(super) fn line_display_width(text: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(text)
}

pub(super) fn display_col_to_byte_offset(text: &str, display_col: usize) -> usize {
    let mut width = 0usize;
    for (idx, ch) in text.char_indices() {
        if width >= display_col {
            return idx;
        }
        let next_width =
            width.saturating_add(unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0));
        if next_width > display_col {
            return idx;
        }
        width = next_width;
    }
    text.len()
}

pub(super) fn clamp_display_col(text: &str, display_col: usize) -> usize {
    display_col.min(line_display_width(text))
}

#[cfg(test)]
mod tests {
    use super::{clamp_display_col, display_col_to_byte_offset, line_display_width};

    #[test]
    fn line_display_width_counts_wide_chars() {
        assert_eq!(line_display_width("abc"), 3);
        assert_eq!(line_display_width("a🙂b"), 4);
    }

    #[test]
    fn display_col_to_byte_offset_stops_before_partial_wide_char() {
        let text = "a🙂b";

        assert_eq!(display_col_to_byte_offset(text, 0), 0);
        assert_eq!(display_col_to_byte_offset(text, 1), 1);
        assert_eq!(display_col_to_byte_offset(text, 2), 1);
        assert_eq!(display_col_to_byte_offset(text, 3), "a🙂".len());
        assert_eq!(display_col_to_byte_offset(text, 99), text.len());
    }

    #[test]
    fn clamp_display_col_caps_at_line_display_width() {
        assert_eq!(clamp_display_col("a🙂b", 99), 4);
        assert_eq!(clamp_display_col("a🙂b", 2), 2);
    }
}
