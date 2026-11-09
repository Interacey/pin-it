//! Positions in the excluded apps window. Everything is defined in dp (a 4 dp grid) and turned
//! into pixels for the window's scale. No Win32 here.

/// Window size in dp.
pub(super) const WIDTH_DP: f32 = 320.0;
pub(super) const HEIGHT_DP: f32 = 400.0;

const MARGIN_DP: f32 = 16.0;
const HEADER_TOP_DP: f32 = 16.0;
const HEADER_HEIGHT_DP: f32 = 24.0;
const LIST_TOP_DP: f32 = 48.0;
const LIST_HEIGHT_DP: f32 = 260.0;
/// Space right of the rows for the scroll indicator.
const GUTTER_DP: f32 = 8.0;
const ROW_HEIGHT_DP: f32 = 36.0;
const ROW_RADIUS_DP: f32 = 8.0;
/// Space between the row's left edge and its text, and the text's own left edge in the header.
const TEXT_INSET_DP: f32 = 12.0;
/// The trash button is a square hit target with the icon centered in it.
const TRASH_BOX_DP: f32 = 28.0;
const TRASH_ICON_DP: f32 = 20.0;
const TRASH_INSET_DP: f32 = 4.0;
const SCROLLBAR_WIDTH_DP: f32 = 4.0;
const SCROLLBAR_PAD_DP: f32 = 4.0;
const SCROLLBAR_MIN_THUMB_DP: f32 = 24.0;
const INPUT_TOP_DP: f32 = 320.0;
const INPUT_HEIGHT_DP: f32 = 40.0;
const INPUT_RADIUS_DP: f32 = 8.0;
/// Height of the line the EDIT control occupies inside the input box.
const EDIT_HEIGHT_DP: f32 = 20.0;
const HINT_TOP_DP: f32 = 364.0;
const HINT_HEIGHT_DP: f32 = 20.0;
/// Width of the accent mark of the selected row.
const SELECT_MARK_WIDTH_DP: f32 = 2.0;
const SELECT_MARK_HEIGHT_DP: f32 = 16.0;

/// A rectangle in pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Area {
    pub(super) x: f32,
    pub(super) y: f32,
    pub(super) w: f32,
    pub(super) h: f32,
}

impl Area {
    pub(super) fn right(&self) -> f32 {
        self.x + self.w
    }

    pub(super) fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub(super) fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    /// A `size` square centered in this rectangle.
    pub(super) fn centered_square(&self, size: f32) -> Area {
        Area {
            x: self.x + (self.w - size) / 2.0,
            y: self.y + (self.h - size) / 2.0,
            w: size,
            h: size,
        }
    }
}

/// Pixel positions for one window scale (pixels per dp).
#[derive(Debug, Clone, Copy)]
pub(super) struct Layout {
    pub(super) scale: f32,
}

impl Layout {
    pub(super) fn new(scale: f32) -> Self {
        Self { scale }
    }

    pub(super) fn px(&self, dp: f32) -> f32 {
        dp * self.scale
    }

    fn area(&self, x: f32, y: f32, w: f32, h: f32) -> Area {
        Area {
            x: self.px(x),
            y: self.px(y),
            w: self.px(w),
            h: self.px(h),
        }
    }

    /// Window size in pixels.
    pub(super) fn size_px(&self) -> (u32, u32) {
        (
            self.px(WIDTH_DP).round() as u32,
            self.px(HEIGHT_DP).round() as u32,
        )
    }

    pub(super) fn header(&self) -> Area {
        let width = WIDTH_DP - MARGIN_DP - TEXT_INSET_DP - MARGIN_DP;
        self.area(
            MARGIN_DP + TEXT_INSET_DP,
            HEADER_TOP_DP,
            width,
            HEADER_HEIGHT_DP,
        )
    }

    /// The part of the window that shows rows.
    pub(super) fn list(&self) -> Area {
        let width = WIDTH_DP - 2.0 * MARGIN_DP;
        self.area(MARGIN_DP, LIST_TOP_DP, width, LIST_HEIGHT_DP)
    }

    pub(super) fn row_height(&self) -> f32 {
        self.px(ROW_HEIGHT_DP)
    }

    pub(super) fn row_radius(&self) -> f32 {
        self.px(ROW_RADIUS_DP)
    }

    /// The row whose top edge is at `top` pixels; rows are as wide as the list without the gutter.
    pub(super) fn row_at(&self, top: f32) -> Area {
        let list = self.list();
        Area {
            x: list.x,
            y: top,
            w: list.w - self.px(GUTTER_DP),
            h: self.row_height(),
        }
    }

    pub(super) fn trash_box(&self, row: &Area) -> Area {
        let size = self.px(TRASH_BOX_DP);
        Area {
            x: row.right() - self.px(TRASH_INSET_DP) - size,
            y: row.y + (row.h - size) / 2.0,
            w: size,
            h: size,
        }
    }

    pub(super) fn trash_icon(&self, row: &Area) -> Area {
        self.trash_box(row).centered_square(self.px(TRASH_ICON_DP))
    }

    /// Where the name of a row is written: between the left inset and the trash button.
    pub(super) fn name_area(&self, row: &Area) -> Area {
        let left = row.x + self.px(TEXT_INSET_DP);
        Area {
            x: left,
            y: row.y,
            w: self.trash_box(row).x - left,
            h: row.h,
        }
    }

    /// The accent mark at the left edge of the selected row.
    pub(super) fn select_mark(&self, row: &Area) -> Area {
        let (w, h) = (
            self.px(SELECT_MARK_WIDTH_DP),
            self.px(SELECT_MARK_HEIGHT_DP),
        );
        Area {
            x: row.x + self.px(TRASH_INSET_DP) / 2.0,
            y: row.y + (row.h - h) / 2.0,
            w,
            h,
        }
    }

    /// The groove of the scroll indicator, in the gutter right of the rows.
    pub(super) fn scrollbar_track(&self) -> Area {
        let list = self.list();
        let pad = self.px(SCROLLBAR_PAD_DP);
        let width = self.px(SCROLLBAR_WIDTH_DP);
        Area {
            x: list.right() - (self.px(GUTTER_DP) + width) / 2.0,
            y: list.y + pad,
            w: width,
            h: list.h - 2.0 * pad,
        }
    }

    pub(super) fn scrollbar_min_thumb(&self) -> f32 {
        self.px(SCROLLBAR_MIN_THUMB_DP)
    }

    pub(super) fn input_box(&self) -> Area {
        let width = WIDTH_DP - 2.0 * MARGIN_DP;
        self.area(MARGIN_DP, INPUT_TOP_DP, width, INPUT_HEIGHT_DP)
    }

    pub(super) fn input_radius(&self) -> f32 {
        self.px(INPUT_RADIUS_DP)
    }

    /// Where the EDIT control sits: one line, centered vertically in the input box.
    pub(super) fn edit(&self) -> Area {
        let outer = self.input_box();
        let inset = self.px(TEXT_INSET_DP);
        let height = self.px(EDIT_HEIGHT_DP);
        Area {
            x: outer.x + inset,
            y: outer.y + (outer.h - height) / 2.0,
            w: outer.w - 2.0 * inset,
            h: height,
        }
    }

    /// The line under the input for the hint about an invalid name.
    pub(super) fn hint(&self) -> Area {
        let width = WIDTH_DP - 2.0 * MARGIN_DP - 2.0 * TEXT_INSET_DP;
        self.area(
            MARGIN_DP + TEXT_INSET_DP,
            HINT_TOP_DP,
            width,
            HINT_HEIGHT_DP,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_size_follows_the_scale() {
        for (scale, size) in [(1.0, (320, 400)), (1.5, (480, 600)), (2.0, (640, 800))] {
            assert_eq!(Layout::new(scale).size_px(), size);
        }
    }

    #[test]
    fn the_parts_do_not_overlap_and_stay_inside_the_window() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let l = Layout::new(scale);
            let (w, h) = l.size_px();
            let (w, h) = (w as f32, h as f32);
            let (header, list, input, hint) = (l.header(), l.list(), l.input_box(), l.hint());
            assert!(header.bottom() <= list.y && list.bottom() <= input.y);
            assert!(input.bottom() <= hint.y && hint.bottom() <= h);
            for a in [header, list, input, hint, l.edit(), l.scrollbar_track()] {
                assert!(a.x >= 0.0 && a.right() <= w, "{a:?} at scale {scale}");
            }
        }
    }

    #[test]
    fn the_trash_button_sits_at_the_right_end_of_the_row_and_the_name_ends_before_it() {
        let l = Layout::new(1.0);
        let row = l.row_at(100.0);
        let (trash, name) = (l.trash_box(&row), l.name_area(&row));
        assert_eq!(trash.right(), row.right() - 4.0);
        assert!(name.right() <= trash.x && name.x > row.x);
        let icon = l.trash_icon(&row);
        assert_eq!(
            (icon.x - trash.x, icon.y - trash.y),
            (trash.right() - icon.right(), trash.bottom() - icon.bottom())
        );
    }

    #[test]
    fn the_scroll_indicator_lies_in_the_gutter_right_of_the_rows() {
        let l = Layout::new(1.0);
        let (track, row) = (l.scrollbar_track(), l.row_at(0.0));
        assert!(track.x >= row.right() && track.right() <= l.list().right());
    }

    #[test]
    fn the_edit_control_is_centered_in_the_input_box() {
        let l = Layout::new(2.0);
        let (outer, edit) = (l.input_box(), l.edit());
        assert_eq!(edit.y - outer.y, outer.bottom() - edit.bottom());
        assert_eq!(edit.x - outer.x, outer.right() - edit.right());
    }

    #[test]
    fn area_contains_is_half_open() {
        let a = Area {
            x: 10.0,
            y: 10.0,
            w: 5.0,
            h: 5.0,
        };
        assert!(a.contains(10.0, 10.0) && a.contains(14.9, 14.9));
        assert!(!a.contains(15.0, 12.0) && !a.contains(12.0, 15.0) && !a.contains(9.9, 12.0));
    }
}
