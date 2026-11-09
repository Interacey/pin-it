//! The pin must not cover content when a window is made smaller. The test window paints its own
//! "title bar": a menu at the left, icons at fixed distances from the right edge (so they stay put
//! while the window shrinks and the menu gets closer to them).

use std::time::Instant;

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, DeleteObject, EndPaint, FillRect, HDC, PAINTSTRUCT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetSystemMetrics,
    RegisterClassW, SM_CXSCREEN, SM_CYSCREEN, SW_SHOWNA, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER,
    SetWindowPos, ShowWindow, WM_ERASEBKGND, WM_PAINT, WM_PRINTCLIENT, WNDCLASSW, WS_POPUP,
};
use windows::core::w;

use super::support::{eventually, options, pump_ms, temp_dir};
use crate::Hwnd;
use crate::app::Runtime;
use crate::app::host::with_app;
use crate::color::ScanLookup;
use crate::config::Settings;
use crate::events::WinEvent;
use crate::win32::to_hwnd;

const BACKGROUND: [u8; 3] = [32, 32, 32];
const CONTENT: [u8; 3] = [230, 230, 230];
/// Rows of the painted "icons", inside the caption row of a 32 px title bar.
const ICON_TOP: i32 = 8;
const ICON_BOTTOM: i32 = 24;
/// The menu at the left edge: x range.
const MENU: (i32, i32) = (10, 250);
/// The icons: x ranges measured from the right edge of the window.
const ICONS_FROM_RIGHT: [(i32, i32); 2] = [(260, 230), (220, 190)];
const START_WIDTH: i32 = 700;
const HEIGHT: i32 = 200;

/// Content rectangles (x ranges, window-relative) of a window `width` px wide.
fn content(width: i32) -> Vec<(i32, i32)> {
    let icons = ICONS_FROM_RIGHT
        .iter()
        .map(|&(a, b)| (width - a, width - b));
    std::iter::once(MENU).chain(icons).collect()
}

fn fill(dc: HDC, rect: RECT, [r, g, b]: [u8; 3]) {
    // SAFETY: a solid brush made and deleted here; `dc` is valid for the paint message.
    unsafe {
        let brush = CreateSolidBrush(COLORREF(
            u32::from(r) | u32::from(g) << 8 | u32::from(b) << 16,
        ));
        FillRect(dc, &rect, brush);
        let _ = DeleteObject(brush.into());
    }
}

fn paint(hwnd: HWND, dc: HDC) {
    let mut client = RECT::default();
    // SAFETY: own window.
    unsafe { GetClientRect(hwnd, &mut client) }.unwrap();
    fill(dc, client, BACKGROUND);
    for (from, to) in content(client.right) {
        fill(
            dc,
            RECT {
                left: from,
                top: ICON_TOP,
                right: to,
                bottom: ICON_BOTTOM,
            },
            CONTENT,
        );
    }
}

unsafe extern "system" fn paint_proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match m {
        WM_ERASEBKGND => return LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            // SAFETY: standard paint cycle on our own window.
            unsafe {
                let dc = BeginPaint(h, &mut ps);
                paint(h, dc);
                let _ = EndPaint(h, &ps);
            }
            return LRESULT(0);
        }
        WM_PRINTCLIENT => {
            paint(h, HDC(w.0 as *mut _));
            return LRESULT(0);
        }
        _ => {}
    }
    // SAFETY: forwards the unchanged arguments.
    unsafe { DefWindowProcW(h, m, w, l) }
}

/// A caption-less window that paints its own title bar, so the app treats it as a custom one.
struct PaintedWindow(Hwnd);

impl PaintedWindow {
    fn new() -> Self {
        // SAFETY: plain class registration and window creation on the test thread.
        unsafe {
            let hinst = GetModuleHandleW(None).unwrap();
            let _ = RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(paint_proc),
                hInstance: hinst.into(),
                lpszClassName: w!("PinItP2Paint"),
                ..Default::default()
            });
            let (sw, sh) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
            let h = CreateWindowExW(
                Default::default(),
                w!("PinItP2Paint"),
                w!("pin-it-p2-paint"),
                WS_POPUP,
                sw - START_WIDTH - 20,
                sh - HEIGHT - 80,
                START_WIDTH,
                HEIGHT,
                None,
                None,
                Some(hinst.into()),
                None,
            )
            .unwrap();
            let _ = ShowWindow(h, SW_SHOWNA);
            PaintedWindow(h.0 as Hwnd)
        }
    }

    fn resize_to(&self, width: i32) {
        // SAFETY: own test window.
        unsafe {
            SetWindowPos(
                to_hwnd(self.0),
                None,
                0,
                0,
                width,
                HEIGHT,
                SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOZORDER,
            )
            .unwrap();
        }
    }
}

impl Drop for PaintedWindow {
    fn drop(&mut self) {
        // SAFETY: own test window.
        unsafe {
            let _ = DestroyWindow(to_hwnd(self.0));
        }
        pump_ms(30);
    }
}

/// Scan width (when there is a scan), shown flag and slot of one entry.
type State = (Option<i32>, bool, Option<(i32, i32, u32)>);

/// What the app has for `h` now: the scan's window width (when there is one), the shown flag and the slot.
fn state(h: Hwnd) -> Option<State> {
    with_app(|a| {
        let scan_width = match a.colors.scan_for(h, Instant::now()) {
            ScanLookup::Ready(scan) => Some(scan.window_size.0),
            ScanLookup::Pending | ScanLookup::Unavailable => None,
        };
        let entry = a.entries.get(&h)?;
        Some((scan_width, entry.shown, entry.slot))
    })
    .flatten()
}

fn window_left(h: Hwnd) -> i32 {
    let mut r = RECT::default();
    // SAFETY: own test window.
    unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowRect(to_hwnd(h), &mut r) }.unwrap();
    r.left
}

fn overlaps(slot: (i32, i32, u32), left: i32, width: i32) -> Option<(i32, i32)> {
    let (x, _, size) = slot;
    let (from, to) = (x - left, x - left + size as i32);
    content(width)
        .into_iter()
        .find(|&(a, b)| from < b && to > a)
}

#[test]
fn shrinking_a_window_never_leaves_the_pin_on_its_content_once_the_new_scan_is_in() {
    let _g = crate::window_test_guard();
    let window = PaintedWindow::new();
    let h = window.0;
    let dir = temp_dir("p2-overlap");
    let rt = Runtime::start(&options(&dir, vec![h], &Settings::default())).unwrap();
    pump_ms(100);

    for width in [700, 640, 560, 500, 440, 400] {
        window.resize_to(width);
        rt.inject_event(WinEvent::Moved(h));
        eventually(
            &format!("scan of the new width {width}"),
            || matches!(state(h), Some((Some(w), _, _)) if w == width),
        );
        pump_ms(50);
        let (_, shown, slot) = state(h).expect("entry exists");
        match (shown, slot) {
            (true, Some(slot)) => {
                let hit = overlaps(slot, window_left(h), width);
                assert!(
                    hit.is_none(),
                    "width {width}: pin {slot:?} covers content {hit:?}"
                );
            }
            (false, _) => assert_eq!(width, 400, "hidden at width {width} although there is room"),
            (true, None) => panic!("width {width}: shown without a slot"),
        }
    }
    let (_, shown, _) = state(h).unwrap();
    assert!(
        !shown,
        "no free run at 400 px: the fresh scan hides the button"
    );
    drop(rt);
    let _ = std::fs::remove_dir_all(dir);
}
