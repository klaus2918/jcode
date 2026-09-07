//! 布局助手。feature-simplification 移除图像侧栏后，其余助手只剩仍被导航 /
//! 输入命中检测使用的 point_in_rect；rect_from_capture 仅测试仍在使用，故
//! 连同其 use 一起标 cfg(test)。

pub(crate) use jcode_tui_render::layout::point_in_rect;

#[cfg(test)]
use super::visual_debug::RectCapture;

#[cfg(test)]
use ratatui::layout::Rect;

/// 将 visual-debug 抓取的矩形 capture 转成渲染 Rect。
#[cfg(test)]
pub(crate) fn rect_from_capture(rect: RectCapture) -> Rect {
    Rect {
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_from_capture_copies_all_fields() {
        let rect = rect_from_capture(RectCapture {
            x: 3,
            y: 5,
            width: 8,
            height: 13,
        });

        assert_eq!(rect, Rect::new(3, 5, 8, 13));
    }
}
