//! 测试专用辅助：构造可滚动的合成内容。
//!
//! 历史上本模块还承载 side-panel 延迟基准与「带 Mermaid 图」的内容构造器。
//! Mermaid 渲染面已随功能精简移除，因此该构造器改为纯文本版本。

impl crate::tui::app::App {
    /// 为滚动/复制类测试构造合成 content。
    ///
    /// `blocks` 重复若干「填充段」（每段固定 6 行，使内容高度与原先的
    /// diagram 段相当），`padding` 为每段追加的尾随填充行数。
    #[cfg(test)]
    pub(in crate::tui::app) fn build_scroll_test_content(blocks: usize, padding: usize) -> String {
        const BLOCK_FILLER: [&str; 6] = [
            "Block marker alpha - the quick brown fox jumps over the lazy dog.",
            "Block marker beta - pack my box with five dozen liquor jugs.",
            "Block marker gamma - how vexingly quick daft zebras jump.",
            "Block marker delta - sphinx of black quartz, judge my vow.",
            "Block marker epsilon - jinxed wizards pluck ivy from the big quilt.",
            "Block marker zeta - bright vixens jump; dozy fowl quack.",
        ];

        let mut out = String::new();
        let intro_lines = padding.max(4);
        for i in 0..intro_lines {
            out.push_str(&format!(
                "Intro line {:02} - quick brown fox jumps over the lazy dog.\n",
                i + 1
            ));
        }

        for idx in 0..blocks {
            for line in BLOCK_FILLER {
                out.push_str(line);
                out.push('\n');
            }
            for j in 0..padding {
                out.push_str(&format!(
                    "After block {} line {:02} - stretch content for scrolling.\n",
                    idx + 1,
                    j + 1
                ));
            }
        }

        out
    }
}
