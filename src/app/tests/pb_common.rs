//! Helpers for the pass-2 tests: own native windows that can be "maximized" into a small area at the
//! lower right (WS_MAXIMIZE set by hand, so the test neither covers the user's screen nor takes the
//! foreground), with an optional DWM caption color.

use std::sync::atomic::{AtomicI32, Ordering};
use std::time::Instant;

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CAPTION_BUTTON_BOUNDS, DWMWA_CAPTION_COLOR, DWMWA_EXTENDED_FRAME_BOUNDS,
    DwmSetWindowAttribute,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GWL_STYLE, GetSystemMetrics, GetWindowLongPtrW,
    GetWindowRect, IsZoomed, RegisterClassW, SM_CXSCREEN, SM_CYSCREEN, SW_SHOWNA, SWP_FRAMECHANGED,
    SWP_NOACTIVATE, SWP_NOZORDER, SetWindowLongPtrW, SetWindowPos, ShowWindow, WNDCLASSW,
    WS_MAXIMIZE, WS_OVERLAPPEDWINDOW,
};
use windows::core::w;

use crate::Hwnd;
use crate::app::entry::Slot;
use crate::app::host::with_app;
use crate::color::ScanLookup;
use crate::placement::scale_dp;
use crate::win32::{dwm_rect, to_hwnd, window_dpi};
use crate::windows_filter::{CAPTION_BUTTON_COUNT, FALLBACK_BUTTON_WIDTH_DP};

/// Size of the area a "maximized" test window fills.
const MAXIMIZED_WIDTH: i32 = 900;
const MAXIMIZED_HEIGHT: i32 = 300;
/// Distance of the test windows from the lower right screen corner.
const CORNER_MARGIN: i32 = 20;
const RESTORED_WIDTH: i32 = 440;
const RESTORED_HEIGHT: i32 = 110;

static MAX_LEFT: AtomicI32 = AtomicI32::new(0);
static MAX_TOP: AtomicI32 = AtomicI32::new(0);

unsafe extern "system" fn native_proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(h, m, w, l) }
}

/// The window handle, and where and how large the window is when it is not maximized (x, y, width, height).
pub(super) struct NativeWin(pub(super) Hwnd, (i32, i32, i32, i32));

impl NativeWin {
    /// `caption_color` sets DWMWA_CAPTION_COLOR: the window then has a colored bar.
    pub(super) fn new(index: i32, caption_color: Option<[u8; 3]>) -> NativeWin {
        // SAFETY: plain window creation on the test thread.
        unsafe {
            let hinst = GetModuleHandleW(None).unwrap();
            let _ = RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(native_proc),
                hInstance: hinst.into(),
                lpszClassName: w!("PinItPbNative"),
                ..Default::default()
            });
            let (sw, sh) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
            MAX_LEFT.store(sw - MAXIMIZED_WIDTH - CORNER_MARGIN, Ordering::Relaxed);
            MAX_TOP.store(sh - MAXIMIZED_HEIGHT - CORNER_MARGIN, Ordering::Relaxed);
            let restored = (
                sw - RESTORED_WIDTH - 2 * CORNER_MARGIN - index * 30,
                sh - RESTORED_HEIGHT - 2 * CORNER_MARGIN - index * 20,
                RESTORED_WIDTH,
                RESTORED_HEIGHT,
            );
            let h = CreateWindowExW(
                Default::default(),
                w!("PinItPbNative"),
                w!("pin-it-pb-test"),
                WS_OVERLAPPEDWINDOW,
                restored.0,
                restored.1,
                restored.2,
                restored.3,
                None,
                None,
                Some(hinst.into()),
                None,
            )
            .unwrap();
            if let Some([r, g, b]) = caption_color {
                let color = COLORREF(u32::from(r) | u32::from(g) << 8 | u32::from(b) << 16);
                let _ = DwmSetWindowAttribute(
                    h,
                    DWMWA_CAPTION_COLOR,
                    &color as *const COLORREF as *const _,
                    size_of::<COLORREF>() as u32,
                );
            }
            let _ = ShowWindow(h, SW_SHOWNA);
            NativeWin(h.0 as Hwnd, restored)
        }
    }

    /// Maximizes into the small area without activating the window: `ShowWindow(SW_MAXIMIZE)` would
    /// make it the foreground window, and the pin tests that run later depend on the foreground state.
    pub(super) fn maximize(&self) {
        self.set_zoomed(true);
    }

    pub(super) fn restore(&self) {
        self.set_zoomed(false);
    }

    fn set_zoomed(&self, zoomed: bool) {
        let h = to_hwnd(self.0);
        let (x, y, width, height) = if zoomed {
            (
                MAX_LEFT.load(Ordering::Relaxed),
                MAX_TOP.load(Ordering::Relaxed),
                MAXIMIZED_WIDTH,
                MAXIMIZED_HEIGHT,
            )
        } else {
            self.1
        };
        // SAFETY: own test window.
        unsafe {
            let style = GetWindowLongPtrW(h, GWL_STYLE);
            let wanted = if zoomed {
                style | WS_MAXIMIZE.0 as isize
            } else {
                style & !(WS_MAXIMIZE.0 as isize)
            };
            SetWindowLongPtrW(h, GWL_STYLE, wanted);
            SetWindowPos(
                h,
                None,
                x,
                y,
                width,
                height,
                SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOZORDER,
            )
            .unwrap();
        }
    }

    pub(super) fn is_zoomed(&self) -> bool {
        // SAFETY: plain query.
        unsafe { IsZoomed(to_hwnd(self.0)).as_bool() }
    }

    pub(super) fn rect(&self) -> RECT {
        let mut r = RECT::default();
        // SAFETY: own window, valid out pointer.
        unsafe { GetWindowRect(to_hwnd(self.0), &mut r).unwrap() };
        r
    }
}

impl Drop for NativeWin {
    fn drop(&mut self) {
        // SAFETY: own test window.
        unsafe { _ = DestroyWindow(to_hwnd(self.0)) };
    }
}

pub(super) fn rect_text(r: &RECT) -> String {
    format!("[{},{},{},{}]", r.left, r.top, r.right, r.bottom)
}

/// One line with everything the placement depends on, for failure messages and the probe output.
pub(super) fn snapshot(w: &NativeWin) -> String {
    let h = to_hwnd(w.0);
    let caption = dwm_rect(h, DWMWA_CAPTION_BUTTON_BOUNDS);
    let frame = dwm_rect(h, DWMWA_EXTENDED_FRAME_BOUNDS);
    let (entry, scan) = with_app(|a| {
        let entry = a.entries.get(&w.0).map(|e| (e.slot, e.shown));
        let scan = match a.colors.scan_for(w.0, Instant::now()) {
            ScanLookup::Ready(s) => {
                format!("scan size {:?} caption {}", s.window_size, s.caption_px)
            }
            ScanLookup::Pending => "scan pending".into(),
            ScanLookup::Unavailable => "scan unavailable".into(),
        };
        (entry, scan)
    })
    .unwrap();
    format!(
        "zoomed {} win {} caption {} frame {} | entry {:?} | {}",
        w.is_zoomed(),
        rect_text(&w.rect()),
        caption.map_or("none".into(), |r| rect_text(&r)),
        frame.map_or("none".into(), |r| rect_text(&r)),
        entry,
        scan
    )
}

/// The slot of a button that is shown.
pub(super) fn shown_slot(h: Hwnd) -> Option<Slot> {
    with_app(|a| a.entries.get(&h).filter(|e| e.shown).and_then(|e| e.slot)).flatten()
}

/// Gap between the pin and the caption-button group (4 dp at 96 dpi).
pub(super) const GAP_PX: i32 = 4;

impl NativeWin {
    /// Moves and resizes the window without activating it.
    pub(super) fn resize_to(&self, width: i32) {
        use windows::Win32::UI::WindowsAndMessaging::SWP_NOMOVE;
        let height = self.rect().bottom - self.rect().top;
        // SAFETY: own test window.
        unsafe {
            SetWindowPos(
                to_hwnd(self.0),
                None,
                0,
                0,
                width,
                height,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
            .unwrap();
        }
    }

    /// Screen position of the visible caption: left edge of the caption-button group, top and bottom row.
    /// DWM reports the bounds from the window rect top, of which a maximized window hides the first `invisible` rows.
    pub(super) fn visible_caption(&self) -> (i32, i32, i32) {
        let r = self.rect();
        let h = to_hwnd(self.0);
        let bounds = dwm_rect(h, DWMWA_CAPTION_BUTTON_BOUNDS).unwrap();
        let invisible = dwm_rect(h, DWMWA_EXTENDED_FRAME_BOUNDS).map_or(0, |f| f.top - r.top);
        (
            r.left + bounds.left,
            r.top + bounds.top.max(invisible),
            r.top + bounds.bottom,
        )
    }

    /// Screen x of the left edge of the caption buttons as drawn: DWM reports the group bounds wider than the
    /// buttons are drawn, so the edge is counted from the visible frame (minus its 1 px border unless maximized).
    fn drawn_group_left(&self) -> i32 {
        let h = to_hwnd(self.0);
        let frame = dwm_rect(h, DWMWA_EXTENDED_FRAME_BOUNDS).unwrap();
        let border = if self.is_zoomed() { 0 } else { 1 };
        let button = scale_dp(FALLBACK_BUTTON_WIDTH_DP, window_dpi(h));
        frame.right - border - CAPTION_BUTTON_COUNT * button
    }

    /// Whether the pin sits right next to the caption buttons, inside the caption rows; the reason if not.
    pub(super) fn pin_beside_buttons(&self) -> Result<(), String> {
        let (_, top, bottom) = self.visible_caption();
        let group_left = self.drawn_group_left();
        let Some((x, y, size)) = shown_slot(self.0) else {
            return Err("no shown slot".into());
        };
        let size = size as i32;
        if x + size != group_left - GAP_PX {
            return Err(format!(
                "pin ends at {} but the group starts at {group_left}",
                x + size
            ));
        }
        if y < top || y + size > bottom {
            return Err(format!(
                "pin rows {y}..{} outside the caption {top}..{bottom}",
                y + size
            ));
        }
        Ok(())
    }
}
