//! One title-bar capture: the median title color plus the sampled caption rows (the scan).

use std::sync::Arc;

use windows::Win32::Foundation::HWND;

use super::accent_scan::{AccentContext, dominant_accent};
use super::capture::{SENTINEL, capture_pixels, is_usable};
use super::ink::glyph_ink;
use super::math::{dominant_cluster, median_color};
use super::plan::{CapturePlan, capture_plan};
use crate::Hwnd;
use crate::placement::{BACKGROUND_TOLERANCE, TitleBarScan};
use crate::win32::to_hwnd;

/// Result of one title-bar capture. `color` is the median title color (None without native caption bounds),
/// `scan` the sampled caption rows (None when no pixel could be read).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TitleSample {
    pub(crate) color: Option<[u8; 3]>,
    pub(crate) scan: Option<Arc<TitleBarScan>>,
}

#[cfg(test)]
impl TitleSample {
    pub(super) fn from_color(color: [u8; 3]) -> Self {
        Self {
            color: Some(color),
            scan: None,
        }
    }
}

/// Captures the title bar of a window via PrintWindow; None on empty bounds, hung, hidden, minimized or cloaked windows, or a failed capture.
/// Only the caption rows are rendered into the capture bitmap. It blocks while the target paints, so `ColorCache` calls it from a worker thread.
/// The process must be per-monitor DPI aware so DWM bounds and GetWindowRect are physical pixels.
pub(crate) fn sample_title_bar(hwnd: Hwnd) -> Option<TitleSample> {
    let h = to_hwnd(hwnd);
    if !is_usable(h) {
        return None;
    }
    let plan = capture_plan(h)?;
    capture_title_bar(h, &plan)
}

/// Captures the title bar once: the color sample points and every column of the scan rows.
fn capture_title_bar(h: HWND, plan: &CapturePlan) -> Option<TitleSample> {
    let all_points = plan_points(plan);
    let pixels = capture_pixels(h, plan.width, plan.caption_rows, &all_points)?;
    let (color_px, rest) = pixels.split_at(plan.points.len());
    let (scan_px, ink_px) = rest.split_at(plan.scan_rows.len() * plan.width.max(1) as usize);
    let scan = build_scan(plan, scan_px, glyph_ink(&readable(ink_px))).map(Arc::new);
    // Custom title bars have no sample points: the scan's bar color stands in.
    let color = median_color(&readable(color_px)).or(scan.as_ref().map(|s| s.background));
    Some(TitleSample { color, scan })
}

/// The color sample points, every column of every scan row, then the pixels of the minimize and maximize buttons.
fn plan_points(plan: &CapturePlan) -> Vec<(i32, i32)> {
    let scan_points = plan
        .scan_rows
        .iter()
        .flat_map(|&y| (0..plan.width).map(move |x| (x, y)));
    plan.points
        .iter()
        .copied()
        .chain(scan_points)
        .chain(plan.ink_points())
        .collect()
}

/// Builds the scan from the pixels read for `plan.scan_rows` (row-major, `plan.width` per row).
/// Pixels that could not be read count as sentinel, which is never background.
fn build_scan(
    plan: &CapturePlan,
    pixels: &[Option<[u8; 3]>],
    caption_ink: Option<[u8; 3]>,
) -> Option<TitleBarScan> {
    let width = plan.width.max(1) as usize;
    let rows: Vec<Vec<[u8; 3]>> = pixels
        .chunks(width)
        .map(|row| row.iter().map(|p| p.unwrap_or(SENTINEL)).collect())
        .collect();
    let background = dominant_cluster(pixels.iter().flatten().copied(), BACKGROUND_TOLERANCE)?;
    let accent = dominant_accent(
        &rows,
        &AccentContext {
            bar: background,
            ink: caption_ink,
            dpi: plan.dpi,
        },
    );
    Some(TitleBarScan {
        window_size: plan.window_size,
        caption_px: plan.caption_px,
        background,
        caption_ink,
        accent,
        rows,
    })
}

/// The pixels that could be read.
fn readable(pixels: &[Option<[u8; 3]>]) -> Vec<[u8; 3]> {
    pixels.iter().flatten().copied().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scan_background_is_the_densest_cluster_not_the_most_frequent_exact_color() {
        let width = 40;
        let plan = CapturePlan {
            width,
            caption_rows: 10,
            points: vec![],
            scan_rows: vec![1],
            window_size: (width, 10),
            caption_px: 30,
            dpi: 96,
            ink_area: None,
        };
        // 10 columns of a flat tab fill, 30 columns of a strip whose color changes a little every column.
        let px: Vec<Option<[u8; 3]>> = (0..width)
            .map(|x| {
                Some(if x < 10 {
                    [23, 21, 25]
                } else {
                    [50 + (x % 5) as u8, 43, 68]
                })
            })
            .collect();
        let scan = build_scan(&plan, &px, None).unwrap();
        assert!(scan.background[0] >= 50, "{:?}", scan.background);
    }

    #[test]
    fn scan_marks_unreadable_pixels_as_sentinel() {
        let plan = CapturePlan {
            width: 3,
            caption_rows: 10,
            points: vec![],
            scan_rows: vec![1, 2],
            window_size: (3, 10),
            caption_px: 30,
            dpi: 96,
            ink_area: None,
        };
        let px = [
            Some([7, 7, 7]),
            None,
            Some([7, 7, 7]),
            Some([7, 7, 7]),
            Some([7, 7, 7]),
            Some([7, 7, 7]),
        ];
        let scan = build_scan(&plan, &px, Some([1, 2, 3])).unwrap();
        assert_eq!(scan.background, [7, 7, 7]);
        assert_eq!(scan.rows.len(), 2);
        assert_eq!(scan.rows[0][1], SENTINEL);
        assert_eq!(scan.window_size, (3, 10));
        assert_eq!(scan.caption_px, 30);
        assert_eq!(scan.caption_ink, Some([1, 2, 3]));
        assert_eq!(scan.accent, None);
    }
}
