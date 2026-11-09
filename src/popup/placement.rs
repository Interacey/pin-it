//! Where a popup opens: on the monitor under the cursor (where the tray icon was clicked), in
//! the bottom-right corner of its work area.

use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

use crate::win32::BASE_DPI;

/// Space between a popup and the screen edge, in dp.
const EDGE_MARGIN_DP: i32 = 12;

/// The dpi and work area of the monitor under the cursor.
pub(crate) fn monitor_under_cursor() -> windows::core::Result<(u32, RECT)> {
    let mut cursor = POINT::default();
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let (mut dpi_x, mut dpi_y) = (BASE_DPI, BASE_DPI);
    // SAFETY: out parameters are valid for the calls.
    unsafe {
        GetCursorPos(&mut cursor)?;
        let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
        GetMonitorInfoW(monitor, &mut info).ok()?;
        GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y)?;
    }
    Ok((dpi_x, info.rcWork))
}

/// Top-left corner that puts a window of `size` pixels into the bottom-right corner of `work`,
/// kept inside the work area when the window is larger than it.
pub(crate) fn bottom_right(work: &RECT, size: (u32, u32), dpi: u32) -> (i32, i32) {
    let margin = EDGE_MARGIN_DP * dpi as i32 / BASE_DPI as i32;
    let x = (work.right - margin - size.0 as i32).max(work.left);
    let y = (work.bottom - margin - size.1 as i32).max(work.top);
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: RECT = RECT {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1040,
    };

    #[test]
    fn the_window_sits_in_the_bottom_right_with_the_scaled_margin() {
        assert_eq!(bottom_right(&WORK, (320, 400), 96), (1588, 628));
        assert_eq!(bottom_right(&WORK, (480, 600), 144), (1422, 422));
    }

    #[test]
    fn a_window_larger_than_the_work_area_stays_at_its_top_left() {
        let small = RECT {
            left: 100,
            top: 50,
            right: 300,
            bottom: 200,
        };
        assert_eq!(bottom_right(&small, (320, 400), 96), (100, 50));
    }
}
