//! Caption-button bounds of a maximized window against its invisible top border.
//!
//! A maximized window's rect starts above the screen by the resize border (8 px at 96 dpi); only the part
//! below it is visible. DWM reports the caption-button bounds in two different ways:
//! - a window with a system caption (a plain Win32 or WinForms window) reports them from the rect top, so
//!   the visible caption is `invisible..bottom` (measured: bounds 0..30, caption drawn at 8..30);
//! - a window that draws its own caption (Chrome, Electron) reports them from the visible top, so the
//!   bounds have to move down by the border.

/// Left, top, right and bottom, window-relative.
pub(crate) type Bounds = (i32, i32, i32, i32);

/// A client area starting this far below the invisible border means the system draws a caption.
const MIN_SYSTEM_CAPTION_PX: i32 = 16;

/// The top of a window in window-relative pixels: where the invisible border ends and where the client area starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TopBorder {
    pub(crate) invisible: i32,
    pub(crate) client_top: i32,
}

impl TopBorder {
    /// A window without an invisible top border (not maximized).
    pub(crate) const NONE: TopBorder = TopBorder {
        invisible: 0,
        client_top: 0,
    };

    fn has_system_caption(&self) -> bool {
        self.client_top - self.invisible >= MIN_SYSTEM_CAPTION_PX
    }

    /// The caption-button `bounds` as the visible caption row: unchanged when they start at or below the visible top.
    pub(crate) fn visible_caption(&self, (l, t, r, b): Bounds) -> Bounds {
        if self.invisible <= 0 || t >= self.invisible {
            return (l, t, r, b);
        }
        if self.has_system_caption() {
            (l, self.invisible.min(b), r, b)
        } else {
            (
                l,
                t.saturating_add(self.invisible),
                r,
                b.saturating_add(self.invisible),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SYSTEM_CAPTION: TopBorder = TopBorder {
        invisible: 8,
        client_top: 31,
    };
    const CUSTOM_CAPTION: TopBorder = TopBorder {
        invisible: 8,
        client_top: 8,
    };

    #[test]
    fn a_system_caption_keeps_its_bottom_and_loses_the_invisible_rows() {
        assert_eq!(
            SYSTEM_CAPTION.visible_caption((2421, 0, 2567, 30)),
            (2421, 8, 2567, 30)
        );
    }

    #[test]
    fn a_system_caption_shorter_than_the_border_keeps_top_above_bottom() {
        let (_, top, _, bottom) = SYSTEM_CAPTION.visible_caption((0, 0, 100, 5));
        assert!(top <= bottom, "top {top} bottom {bottom}");
    }

    #[test]
    fn extreme_bounds_do_not_overflow() {
        let (_, _, _, bottom) = CUSTOM_CAPTION.visible_caption((0, 0, 10, i32::MAX));
        assert_eq!(bottom, i32::MAX);
    }

    #[test]
    fn a_custom_caption_moves_down_by_the_border() {
        assert_eq!(
            CUSTOM_CAPTION.visible_caption((2421, 0, 2567, 32)),
            (2421, 8, 2567, 40)
        );
    }

    #[test]
    fn bounds_at_or_below_the_visible_top_stay() {
        for border in [SYSTEM_CAPTION, CUSTOM_CAPTION] {
            assert_eq!(border.visible_caption((100, 8, 246, 40)), (100, 8, 246, 40));
        }
    }

    #[test]
    fn a_window_without_invisible_border_is_not_touched() {
        assert_eq!(
            TopBorder::NONE.visible_caption((287, 0, 433, 30)),
            (287, 0, 433, 30)
        );
    }
}
