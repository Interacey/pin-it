//! Default placement of the pin button: pure slot geometry plus the window queries that feed it.

use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Dwm::{DWMWA_CAPTION_BUTTON_BOUNDS, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, IsIconic, IsZoomed};

use crate::Hwnd;
use crate::placement::{GAP_TO_CAPTION_BUTTONS_DP, TopBorder, default_size_px, scale_dp};
use crate::win32::{dwm_rect, to_hwnd, window_dpi};

/// Width of one Windows 11 caption button at 96 dpi.
pub(crate) const FALLBACK_BUTTON_WIDTH_DP: i32 = 46;
/// Minimize, maximize and close.
pub(crate) const CAPTION_BUTTON_COUNT: i32 = 3;
/// Height of the caption button row at 96 dpi (VS Code and Chrome draw 32 px buttons; DWM reports 30 for native windows).
pub(crate) const FALLBACK_CAPTION_HEIGHT_DP: i32 = 32;
/// Border of a non-maximized window.
pub(crate) const WINDOW_BORDER_DP: i32 = 1;
/// Width the title must keep free left of the pin in the fallback case.
const MIN_TITLE_WIDTH_DP: i32 = 64;

/// Screen-space position and square size of a pin button, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PinSlot {
    /// Left edge.
    pub(crate) x: i32,
    /// Top edge.
    pub(crate) y: i32,
    /// Width and height.
    pub(crate) size: i32,
}

/// Default pin slot of a window plus what is needed to refine it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DefaultPlacement {
    pub(crate) slot: PinSlot,
    /// True when the slot is an estimate (no native caption-button bounds: custom title bars, some maximized windows).
    pub(crate) estimated: bool,
    pub(crate) dpi: u32,
    /// Window rect in screen pixels (l, t, r, b); scan columns are relative to its left edge.
    pub(crate) window_rect: (i32, i32, i32, i32),
    /// Window-relative column where the caption buttons visibly end; 0 without a visible frame.
    pub(crate) group_right: i32,
}

/// What the placement needs to know about one window.
struct WindowGeometry {
    handle: HWND,
    rect: RECT,
    /// Visible frame (DWMWA_EXTENDED_FRAME_BOUNDS), when DWM reports it.
    frame: Option<RECT>,
    maximized: bool,
    dpi: u32,
}

impl WindowGeometry {
    fn query(w: HWND) -> Option<Self> {
        // SAFETY: plain query.
        let maximized = unsafe { IsZoomed(w).as_bool() };
        let frame = dwm_rect(w, DWMWA_EXTENDED_FRAME_BOUNDS);
        let mut rect = RECT::default();
        // SAFETY: rect is a valid out pointer.
        unsafe { GetWindowRect(w, &mut rect).ok()? };
        Some(Self {
            handle: w,
            rect,
            frame,
            maximized,
            dpi: window_dpi(w),
        })
    }

    fn window_rect(&self) -> (i32, i32, i32, i32) {
        let r = self.rect;
        (r.left, r.top, r.right, r.bottom)
    }

    /// Window-relative column where the caption buttons visibly end: the visible frame's right edge, minus the border
    /// a non-maximized window draws inside it. 0 without a frame.
    fn group_right(&self) -> i32 {
        let border = if self.maximized {
            0
        } else {
            scale_dp(WINDOW_BORDER_DP, self.dpi)
        };
        self.frame.map_or(0, |f| f.right - self.rect.left - border)
    }

    /// Slot from the DWM caption-button bounds; `None` when they are missing or empty.
    fn native_slot(&self) -> Option<PinSlot> {
        let b = dwm_rect(self.handle, DWMWA_CAPTION_BUTTON_BOUNDS)?;
        let origin = (self.rect.left, self.rect.top);
        let top = top_border(
            self.handle,
            &self.rect,
            self.frame.filter(|_| self.maximized),
        );
        compute_slot(
            origin,
            top.visible_caption((b.left, b.top, b.right, b.bottom)),
            self.dpi,
        )
    }

    /// Estimated slot from the visible frame; `None` without a frame or when it is too small.
    fn fallback_slot(&self) -> Option<PinSlot> {
        let f = self.frame?;
        compute_fallback_slot((f.left, f.top, f.right, f.bottom), self.dpi, self.maximized)
    }
}

/// Pin button slot left of the minimize button, with the data `placement::decide` needs. Windows without usable
/// caption-button bounds (custom title bars, maximized Chromium/Electron windows) get a slot left of where the
/// standard three caption buttons would be; `None` only for minimized or tiny windows.
pub(crate) fn default_placement(hwnd: Hwnd) -> Option<DefaultPlacement> {
    let w = to_hwnd(hwnd);
    // SAFETY: plain query.
    if unsafe { IsIconic(w).as_bool() } {
        return None;
    }
    let geometry = WindowGeometry::query(w)?;
    let (slot, estimated) = match geometry.native_slot() {
        Some(slot) => (slot, false),
        None => (geometry.fallback_slot()?, true),
    };
    Some(DefaultPlacement {
        slot,
        estimated,
        dpi: geometry.dpi,
        window_rect: geometry.window_rect(),
        group_right: geometry.group_right(),
    })
}

/// The invisible top border of the window `h` with `frame`, its visible frame, and where its client area starts;
/// `TopBorder::NONE` without a frame (not maximized, or DWM reports none).
pub(crate) fn top_border(h: HWND, window: &RECT, frame: Option<RECT>) -> TopBorder {
    let Some(frame) = frame else {
        return TopBorder::NONE;
    };
    let mut client_origin = POINT::default();
    // SAFETY: client_origin is a valid out pointer; a failure leaves the origin at 0, which reads as "no system caption".
    let _ = unsafe { ClientToScreen(h, &mut client_origin) };
    TopBorder {
        invisible: (frame.top - window.top).max(0),
        client_top: client_origin.y - window.top,
    }
}

/// Pure geometry: slot for the caption-button `bounds` (window-relative l,t,r,b) of a window at `window_origin`.
pub(crate) fn compute_slot(
    window_origin: (i32, i32),
    bounds: (i32, i32, i32, i32),
    dpi: u32,
) -> Option<PinSlot> {
    let (l, t, r, b) = bounds;
    if r <= l || b <= t {
        return None;
    }
    let height = b - t;
    let size = default_size_px(height, dpi);
    let gap = scale_dp(GAP_TO_CAPTION_BUTTONS_DP, dpi);
    Some(PinSlot {
        x: window_origin.0 + l - gap - size,
        y: window_origin.1 + t + (height - size) / 2,
        size,
    })
}

/// Pure geometry for windows without caption-button bounds: the slot left of the three standard caption buttons
/// (3 x 46 px at 96 dpi) at the top right of `frame`, the visible frame (DWMWA_EXTENDED_FRAME_BOUNDS, screen l,t,r,b).
/// A maximized window's visible frame already starts at the monitor work area; other windows have a 1 px border.
/// `None` when the frame is too small to hold the buttons plus a pin.
pub(crate) fn compute_fallback_slot(
    frame: (i32, i32, i32, i32),
    dpi: u32,
    maximized: bool,
) -> Option<PinSlot> {
    let (l, t, r, b) = frame;
    let scale = |v: i32| scale_dp(v, dpi);
    let buttons = CAPTION_BUTTON_COUNT * scale(FALLBACK_BUTTON_WIDTH_DP);
    let caption = scale(FALLBACK_CAPTION_HEIGHT_DP);
    let size = default_size_px(caption, dpi);
    let gap = scale(GAP_TO_CAPTION_BUTTONS_DP);
    let border = if maximized {
        0
    } else {
        scale(WINDOW_BORDER_DP)
    };
    let too_narrow = r - l < buttons + gap + size + scale(MIN_TITLE_WIDTH_DP);
    if too_narrow || b - t < caption + border {
        return None;
    }
    Some(PinSlot {
        x: r - border - buttons - gap - size,
        y: t + border + (caption - size) / 2,
        size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_96_dpi() {
        let s = compute_slot((100, 50), (800, 0, 946, 30), 96).unwrap();
        assert_eq!(
            s,
            PinSlot {
                x: 876,
                y: 55,
                size: 20
            }
        );
    }

    #[test]
    fn slot_144_dpi() {
        let s = compute_slot((0, 0), (1200, 0, 1419, 45), 144).unwrap();
        assert_eq!(
            s,
            PinSlot {
                x: 1200 - 6 - 30,
                y: 7,
                size: 30
            }
        );
    }

    #[test]
    fn slot_192_dpi() {
        let s = compute_slot((10, 20), (1000, 0, 1292, 60), 192).unwrap();
        assert_eq!(
            s,
            PinSlot {
                x: 10 + 1000 - 8 - 40,
                y: 20 + 10,
                size: 40
            }
        );
    }

    #[test]
    fn slot_size_is_clamped() {
        assert_eq!(
            compute_slot((0, 0), (100, 0, 150, 10), 96).unwrap().size,
            16
        );
        assert_eq!(
            compute_slot((0, 0), (100, 0, 150, 100), 96).unwrap().size,
            32
        );
    }

    #[test]
    fn empty_bounds_give_no_slot() {
        assert_eq!(compute_slot((0, 0), (0, 0, 0, 0), 96), None);
        assert_eq!(compute_slot((0, 0), (10, 0, 5, 30), 96), None);
        assert_eq!(compute_slot((0, 0), (0, 5, 100, 5), 96), None);
    }

    #[test]
    fn fallback_slot_96_dpi_maximized() {
        let s = compute_fallback_slot((0, 0, 2560, 1392), 96, true).unwrap();
        assert_eq!(
            s,
            PinSlot {
                x: 2560 - 138 - 4 - 24,
                y: 4,
                size: 24
            }
        );
    }

    #[test]
    fn fallback_slot_96_dpi_normal_window_has_a_border() {
        let s = compute_fallback_slot((334, 231, 2244, 1263), 96, false).unwrap();
        assert_eq!(
            s,
            PinSlot {
                x: 2244 - 1 - 138 - 4 - 24,
                y: 231 + 1 + 4,
                size: 24
            }
        );
    }

    #[test]
    fn fallback_slot_144_and_192_dpi() {
        let s = compute_fallback_slot((0, 0, 3000, 1600), 144, true).unwrap();
        assert_eq!(
            s,
            PinSlot {
                x: 3000 - 207 - 6 - 36,
                y: 6,
                size: 36
            }
        );
        let s = compute_fallback_slot((100, 50, 2100, 1050), 192, false).unwrap();
        assert_eq!(
            s,
            PinSlot {
                x: 2100 - 2 - 276 - 8 - 48,
                y: 50 + 2 + 8,
                size: 48
            }
        );
    }

    #[test]
    fn fallback_slot_stays_inside_and_left_of_the_buttons() {
        for dpi in [96, 120, 144, 192, 288] {
            let s = compute_fallback_slot((-1920, 100, 0, 900), dpi, false).unwrap();
            let scale = |v: i32| (v * dpi as i32 + 48) / 96;
            assert!(
                s.x > -1920 && s.x + s.size <= -3 * scale(46),
                "dpi {dpi}: {s:?}"
            );
            assert!(
                s.y >= 100 && s.y + s.size <= 100 + scale(32) + 1,
                "dpi {dpi}: {s:?}"
            );
        }
    }

    #[test]
    fn fallback_slot_none_for_tiny_windows() {
        assert_eq!(compute_fallback_slot((0, 0, 150, 20), 96, false), None);
        assert_eq!(compute_fallback_slot((0, 0, 1000, 25), 96, false), None);
        assert_eq!(compute_fallback_slot((0, 0, 200, 600), 96, true), None);
        assert_eq!(compute_fallback_slot((10, 10, 5, 5), 96, false), None);
        assert!(compute_fallback_slot((0, 0, 300, 40), 96, false).is_some());
    }
}

#[cfg(test)]
mod centering_tests {
    use super::*;
    use crate::placement::apply_user_scale;

    const DPIS: [u32; 4] = [96, 120, 144, 192];
    /// Largest distance between slot center and caption-row center, in half pixels (1 = 0.5 px).
    const MAX_OFF_HALF_PX: i32 = 1;

    /// Twice the vertical distance between the slot center and the row center `row_top + row_height / 2`.
    fn off_half_px(slot: PinSlot, row_top: i32, row_height: i32) -> i32 {
        (2 * slot.y + slot.size - (2 * row_top + row_height)).abs()
    }

    #[test]
    fn native_slot_is_centered_in_the_caption_button_bounds() {
        for dpi in DPIS {
            for height in [24, 30, 31, 32, 36, 40, 45, 48, 60] {
                let top = 1;
                let slot = compute_slot((100, 50), (800, top, 946, top + height), dpi).unwrap();
                let off = off_half_px(slot, 50 + top, height);
                assert!(
                    off <= MAX_OFF_HALF_PX,
                    "dpi {dpi} height {height}: {slot:?} off {off}"
                );
            }
        }
    }

    #[test]
    fn estimated_slot_is_centered_in_the_fallback_caption_row() {
        for dpi in DPIS {
            let scale = |v: i32| scale_dp(v, dpi);
            let caption = scale(FALLBACK_CAPTION_HEIGHT_DP);
            for (maximized, border) in [(false, scale(WINDOW_BORDER_DP)), (true, 0)] {
                let frame = (0, 200, 3000, 1600);
                let slot = compute_fallback_slot(frame, dpi, maximized).unwrap();
                let off = off_half_px(slot, frame.1 + border, caption);
                assert!(
                    off <= MAX_OFF_HALF_PX,
                    "dpi {dpi} maximized {maximized}: {slot:?} off {off}"
                );
            }
        }
    }

    #[test]
    fn user_scale_keeps_the_slot_centered() {
        for dpi in DPIS {
            let caption = scale_dp(FALLBACK_CAPTION_HEIGHT_DP, dpi);
            let slot = compute_fallback_slot((0, 0, 3000, 1600), dpi, true).unwrap();
            for percent in [75, 100, 125, 150] {
                let scaled = apply_user_scale(slot, percent, dpi);
                let off = off_half_px(scaled, 0, caption);
                assert!(
                    off <= MAX_OFF_HALF_PX,
                    "dpi {dpi} {percent}%: {scaled:?} off {off}"
                );
            }
        }
    }
}
