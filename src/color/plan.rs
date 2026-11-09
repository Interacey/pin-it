//! Where in a window the title bar is and which points to read: the capture plan.

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Dwm::{DWMWA_CAPTION_BUTTON_BOUNDS, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, IsZoomed};

use crate::placement::scale_dp;
use crate::win32::{dwm_rect, window_dpi};
use crate::windows_filter::{
    CAPTION_BUTTON_COUNT, FALLBACK_BUTTON_WIDTH_DP, FALLBACK_CAPTION_HEIGHT_DP, WINDOW_BORDER_DP,
    top_border,
};

const SAMPLE_COUNT: i32 = 5;
/// The color samples sit between 20% and 80% of the caption button height.
const SAMPLE_TOP_FRACTION: f32 = 0.2;
const SAMPLE_SPAN_FRACTION: f32 = 0.6;
/// First sample column: this far left of the caption buttons, then one column per `SAMPLE_SPACING_PX` further left.
const SAMPLE_GAP_TO_BUTTONS_PX: i32 = 10;
const SAMPLE_SPACING_PX: i32 = 4;
/// Rows below the caption buttons that the capture bitmap still covers.
const CAPTURE_MARGIN: i32 = 4;
/// The placement scan keeps every pixel row between 15% and 85% of the caption height: a one pixel
/// symbol (the minimize line) would fall between sparse rows.
const SCAN_TOP_PERCENT: i32 = 15;
const SCAN_BOTTOM_PERCENT: i32 = 85;
/// Caption-button group width at 96 dpi for windows without native caption-button bounds.
const FALLBACK_BUTTON_GROUP_DP: i32 = CAPTION_BUTTON_COUNT * FALLBACK_BUTTON_WIDTH_DP;
/// Caption buttons whose symbols give the ink: minimize and maximize.
const SYMBOL_BUTTONS: i32 = 2;

/// What to capture for one window: its size, how many rows the capture bitmap needs, the color sample points and the scan rows.
pub(super) struct CapturePlan {
    pub(super) width: i32,
    pub(super) caption_rows: i32,
    /// Color sample points; empty when the window has no native caption-button bounds.
    pub(super) points: Vec<(i32, i32)>,
    pub(super) scan_rows: Vec<i32>,
    /// Window width and height.
    pub(super) window_size: (i32, i32),
    /// Height of the caption row in pixels.
    pub(super) caption_px: i32,
    pub(super) dpi: u32,
    /// Minimize and maximize buttons, inside the capture bitmap; None when they are not visible.
    pub(super) ink_area: Option<RECT>,
}

impl CapturePlan {
    /// Every pixel of `ink_area`, row by row.
    pub(super) fn ink_points(&self) -> Vec<(i32, i32)> {
        let Some(a) = self.ink_area else {
            return Vec::new();
        };
        (a.top..a.bottom)
            .flat_map(|y| (a.left..a.right).map(move |x| (x, y)))
            .collect()
    }
}

/// Where the caption row is, window-relative, and whether DWM reported it.
struct CaptionRect {
    bounds: RECT,
    native: bool,
}

/// Window width, window height, capture height (caption rows only), sample points and scan rows of a window.
pub(super) fn capture_plan(h: HWND) -> Option<CapturePlan> {
    let mut window = RECT::default();
    // SAFETY: window is a valid out param.
    unsafe { GetWindowRect(h, &mut window) }.ok()?;
    let (width, height) = window_size(&window)?;
    let dpi = window_dpi(h);
    let caption = caption_rect(h, &window, dpi)?;
    let points = if caption.native {
        visible_sample_points(&caption.bounds, width, height)
    } else {
        Vec::new()
    };
    let scan_rows = visible_scan_rows(&caption.bounds, height);
    let caption_rows = capture_rows(caption.bounds.bottom, height);
    let ink_area = symbol_area(&caption.bounds, dpi, (width, caption_rows));
    if points.is_empty() && scan_rows.is_empty() {
        return None;
    }
    Some(CapturePlan {
        width,
        caption_rows,
        points,
        scan_rows,
        window_size: (width, height),
        caption_px: caption.bounds.bottom - caption.bounds.top,
        dpi,
        ink_area,
    })
}

/// Width and height of a window rectangle; None when either is not positive (a collapsed window).
fn window_size(window: &RECT) -> Option<(i32, i32)> {
    let width = window.right.saturating_sub(window.left);
    let height = window.bottom.saturating_sub(window.top);
    (width > 0 && height > 0).then_some((width, height))
}

/// Rows of the capture bitmap: down to the caption plus `CAPTURE_MARGIN`, at least one, at most the window height.
/// `Ord::clamp` panics when min > max, so a height below one must not reach it.
fn capture_rows(caption_bottom: i32, height: i32) -> i32 {
    caption_bottom
        .saturating_add(CAPTURE_MARGIN)
        .clamp(1, height.max(1))
}

/// The minimize and maximize buttons (the left end of the caption-button group `caption`), cut to the capture bitmap `bitmap` (width, rows).
/// Close is left out: it turns red under the mouse. The minimize line alone is thin and anti-aliased at fractional dpi,
/// so the maximize symbol next to it supplies the full-strength pixels. A group of fewer than three buttons is read as far as it goes.
fn symbol_area(caption: &RECT, dpi: u32, bitmap: (i32, i32)) -> Option<RECT> {
    let button = scale_dp(FALLBACK_BUTTON_WIDTH_DP, dpi).max(1);
    let buttons = (caption.right - caption.left + button / 2) / button;
    let symbol_buttons = (buttons - 1).clamp(1, SYMBOL_BUTTONS);
    let area = RECT {
        left: caption.left.max(0),
        top: caption.top.max(0),
        right: (caption.left + symbol_buttons * button)
            .min(caption.right)
            .min(bitmap.0),
        bottom: caption.bottom.min(bitmap.1),
    };
    is_non_empty(&area).then_some(area)
}

fn visible_sample_points(caption_buttons: &RECT, width: i32, height: i32) -> Vec<(i32, i32)> {
    sample_points(caption_buttons)
        .into_iter()
        .filter(|&(x, y)| x >= 0 && y >= 0 && x < width && y < height)
        .collect()
}

fn visible_scan_rows(caption: &RECT, height: i32) -> Vec<i32> {
    scan_row_ys(caption.top, caption.bottom)
        .into_iter()
        .filter(|&y| y >= 0 && y < height)
        .collect()
}

/// Window-relative sample points: 5 rows at 20-80% of the caption button height, x = left - 10 - k*4.
fn sample_points(b: &RECT) -> Vec<(i32, i32)> {
    let height = (b.bottom - b.top) as f32;
    (0..SAMPLE_COUNT)
        .map(|k| {
            let f =
                SAMPLE_TOP_FRACTION + SAMPLE_SPAN_FRACTION * k as f32 / (SAMPLE_COUNT - 1) as f32;
            (
                b.left - SAMPLE_GAP_TO_BUTTONS_PX - k * SAMPLE_SPACING_PX,
                b.top + (height * f) as i32,
            )
        })
        .collect()
}

/// Window-relative rows for the placement scan: every row between 15% and 85% of the caption rows `top..bottom`.
fn scan_row_ys(top: i32, bottom: i32) -> Vec<i32> {
    let height = (bottom - top).max(1);
    let first = top + height * SCAN_TOP_PERCENT / 100;
    let last = top + height * SCAN_BOTTOM_PERCENT / 100;
    (first..=last).collect()
}

/// Caption row of the window: the DWM caption-button bounds, or for windows without them (custom title bars)
/// the row where the three standard buttons would be. Rows above the visible frame of a maximized window are skipped.
fn caption_rect(h: HWND, window: &RECT, dpi: u32) -> Option<CaptionRect> {
    // SAFETY: plain query.
    let maximized = unsafe { IsZoomed(h).as_bool() };
    let frame = dwm_rect(h, DWMWA_EXTENDED_FRAME_BOUNDS);
    match dwm_rect(h, DWMWA_CAPTION_BUTTON_BOUNDS).filter(is_non_empty) {
        Some(buttons) => Some(native_caption(
            buttons,
            h,
            window,
            frame.filter(|_| maximized),
        )),
        None => Some(estimated_caption(frame?, window, dpi, maximized)),
    }
}

fn is_non_empty(r: &RECT) -> bool {
    r.right > r.left && r.bottom > r.top
}

/// DWM caption-button bounds as the visible caption row of a maximized window (see `TopBorder`).
fn native_caption(
    buttons: RECT,
    h: HWND,
    window: &RECT,
    maximized_frame: Option<RECT>,
) -> CaptionRect {
    let top = top_border(h, window, maximized_frame);
    let (left, top_px, right, bottom) =
        top.visible_caption((buttons.left, buttons.top, buttons.right, buttons.bottom));
    CaptionRect {
        bounds: RECT {
            left,
            top: top_px,
            right,
            bottom,
        },
        native: true,
    }
}

/// The row where the three standard caption buttons would be, from the visible frame.
fn estimated_caption(frame: RECT, window: &RECT, dpi: u32, maximized: bool) -> CaptionRect {
    let border = if maximized {
        0
    } else {
        scale_dp(WINDOW_BORDER_DP, dpi)
    };
    let buttons = scale_dp(FALLBACK_BUTTON_GROUP_DP, dpi);
    let right = frame.right - window.left - border;
    let top = frame.top - window.top + border;
    CaptionRect {
        bounds: RECT {
            left: right - buttons,
            top,
            right,
            bottom: top + scale_dp(FALLBACK_CAPTION_HEIGHT_DP, dpi),
        },
        native: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXTREMES: [i32; 9] = [
        i32::MIN,
        i32::MIN + 1,
        -1000,
        -1,
        0,
        1,
        1000,
        i32::MAX - 1,
        i32::MAX,
    ];

    #[test]
    fn a_window_without_positive_width_and_height_has_no_size() {
        for left in EXTREMES {
            for right in EXTREMES {
                let window = RECT {
                    left,
                    top: 0,
                    right,
                    bottom: 40,
                };
                let size = window_size(&window);
                let expected = right.saturating_sub(left) > 0;
                assert_eq!(size.is_some(), expected, "x {left}..{right}");
                let window = RECT {
                    left: 0,
                    top: left,
                    right: 40,
                    bottom: right,
                };
                assert_eq!(
                    window_size(&window).is_some(),
                    expected,
                    "y {left}..{right}"
                );
            }
        }
        let normal = RECT {
            left: 10,
            top: 20,
            right: 110,
            bottom: 70,
        };
        assert_eq!(window_size(&normal), Some((100, 50)));
    }

    #[test]
    fn capture_rows_never_panic_and_stay_within_one_and_the_height() {
        for bottom in EXTREMES {
            for height in EXTREMES {
                let rows = capture_rows(bottom, height);
                assert!(
                    rows >= 1 && rows <= height.max(1),
                    "{bottom} {height}: {rows}"
                );
            }
        }
        assert_eq!(capture_rows(30, 800), 34);
        assert_eq!(capture_rows(30, 20), 20);
        assert_eq!(capture_rows(-50, 800), 1);
    }

    #[test]
    fn sample_points_stay_in_caption_rows() {
        let b = RECT {
            left: 800,
            top: 0,
            right: 900,
            bottom: 30,
        };
        let p = sample_points(&b);
        assert_eq!(p.len(), 5);
        assert_eq!(p[0], (790, 6));
        assert!(p.iter().all(|&(_, y)| (6..=24).contains(&y)));
        assert_eq!(p[4].0, 774);
    }

    #[test]
    fn symbol_area_is_the_first_two_buttons_of_the_group() {
        let group = RECT {
            left: 800,
            top: 0,
            right: 938,
            bottom: 32,
        };
        let area = symbol_area(&group, 96, (940, 36)).unwrap();
        assert_eq!(
            (area.left, area.top, area.right, area.bottom),
            (800, 0, 892, 32)
        );
        let wide = RECT {
            right: 1007,
            ..group
        };
        let area = symbol_area(&wide, 144, (1010, 48)).unwrap();
        assert_eq!((area.left, area.right), (800, 938));
        let two_buttons = RECT {
            right: 892,
            ..group
        };
        assert_eq!(symbol_area(&two_buttons, 96, (940, 36)).unwrap().right, 846);
    }

    #[test]
    fn symbol_area_is_cut_to_the_bitmap_and_the_group() {
        let group = RECT {
            left: 800,
            top: 0,
            right: 830,
            bottom: 32,
        };
        let area = symbol_area(&group, 96, (820, 20)).unwrap();
        assert_eq!((area.right, area.bottom), (820, 20));
        assert!(symbol_area(&group, 96, (800, 20)).is_none());
    }

    #[test]
    fn scan_rows_cover_the_middle_of_the_caption_without_gaps() {
        let ys = scan_row_ys(0, 100);
        assert_eq!((ys[0], *ys.last().unwrap(), ys.len()), (15, 85, 71));
        let ys = scan_row_ys(8, 40);
        assert!(ys.iter().all(|&y| (8..40).contains(&y)));
        assert!(ys.windows(2).all(|w| w[1] == w[0] + 1));
        assert!(ys.contains(&(8 + 16)), "the center row is scanned");
    }
}
