//! Timeline of the pin on a real Firefox window: when it first appears and where, against the chevron.
//! Needs the only Firefox on the desktop being the one under test and `PLACE_OUT`.
//! Run with `cargo test f3_firefox -- --ignored --nocapture`.

use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowRect, IsWindowVisible};
use windows::core::BOOL;

use super::pb_common::shown_slot;
use super::support::{options, pump_ms, temp_dir};
use crate::Hwnd;
use crate::app::Runtime;
use crate::app::entry::Slot;
use crate::app::host::with_app;
use crate::color::ScanLookup;
use crate::config::Settings;
use crate::placement::real_windows::save_screen_strip;
use crate::win32::class_name;
use crate::windows_filter::{PinSlot, default_placement};

const FIREFOX_CLASS: &str = "MozillaWindowClass";
const WINDOW_WAIT: Duration = Duration::from_secs(30);
const TEST_WIDTH: i32 = 1100;
const TEST_HEIGHT: i32 = 500;
const WATCH: Duration = Duration::from_millis(9000);
const FIRST_COLOR: [u8; 3] = [255, 160, 0];
const FINAL_COLOR: [u8; 3] = [255, 0, 0];

/// The helper script starts the Firefox under test at 1100 x 500; other Firefox windows (the user's) differ.
fn is_test_size(rect: &RECT) -> bool {
    let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
    (TEST_WIDTH..TEST_WIDTH + 40).contains(&w) && (TEST_HEIGHT..TEST_HEIGHT + 40).contains(&h)
}

struct Search {
    found: Option<Hwnd>,
}

fn firefox_window() -> Option<Hwnd> {
    unsafe extern "system" fn visit(h: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: lparam is the &mut Search passed below.
        let search = unsafe { &mut *(lparam.0 as *mut Search) };
        // SAFETY: plain query.
        let visible = unsafe { IsWindowVisible(h) }.as_bool();
        let mut rect = RECT::default();
        // SAFETY: plain query with a valid out pointer.
        let sized = unsafe { GetWindowRect(h, &mut rect) }.is_ok() && is_test_size(&rect);
        if visible && sized && class_name(h) == FIREFOX_CLASS {
            search.found = Some(h.0 as Hwnd);
        }
        true.into()
    }
    let mut search = Search { found: None };
    // SAFETY: the callback only uses lparam as the Search above.
    let _ = unsafe { EnumWindows(Some(visit), LPARAM(&mut search as *mut _ as isize)) };
    search.found
}

fn pin_slot(slot: Slot) -> PinSlot {
    PinSlot {
        x: slot.0,
        y: slot.1,
        size: slot.2 as i32,
    }
}

/// Polls until the process shows its main window, so that a run can start right at the startup.
fn wait_for_window() -> Hwnd {
    let deadline = Instant::now() + WINDOW_WAIT;
    loop {
        if let Some(h) = firefox_window() {
            return h;
        }
        assert!(
            Instant::now() < deadline,
            "no visible Firefox window of the Firefox window"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
#[ignore]
fn f3_firefox_timeline() {
    let _g = crate::window_test_guard();
    // SAFETY: process-wide DPI mode, set before the window is measured.
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let out = std::path::PathBuf::from(std::env::var("PLACE_OUT").unwrap());
    std::fs::create_dir_all(&out).unwrap();
    let h = wait_for_window();
    let default = default_placement(h).unwrap();
    println!(
        "window {:?} dpi={} estimated={} default slot={:?} group_right={}",
        default.window_rect, default.dpi, default.estimated, default.slot, default.group_right
    );
    let dir = temp_dir("f3-firefox");
    let t0 = Instant::now();
    let rt = Runtime::start(&options(&dir, vec![h], &Settings::default())).unwrap();
    println!("runtime started after {} ms", t0.elapsed().as_millis());
    let mut last: Option<Option<Slot>> = None;
    let mut first = None;
    while t0.elapsed() < WATCH {
        pump_ms(2);
        let now = shown_slot(h);
        if last != Some(now) {
            println!("t={:>5} ms shown slot={now:?}", t0.elapsed().as_millis());
            first = first.or(now);
            describe_scan(h);
            last = Some(now);
        }
    }
    let marks: Vec<(PinSlot, [u8; 3])> = [(first, FIRST_COLOR), (last.flatten(), FINAL_COLOR)]
        .into_iter()
        .filter_map(|(slot, color)| Some((pin_slot(slot?), color)))
        .collect();
    let name = std::env::var("PNG_NAME").unwrap_or_else(|_| "ff.png".into());
    println!(
        "png saved: {}",
        save_screen_strip(default.window_rect, &marks, &out.join(name))
    );
    drop(rt);
    let _ = std::fs::remove_dir_all(dir);
}

/// Prints the occupied column runs of the cached scan of `h` (diagnostic).
fn describe_scan(h: Hwnd) {
    let lookup = with_app(|a| a.colors.scan_for(h, Instant::now()));
    let Some(ScanLookup::Ready(scan)) = lookup else {
        println!("   scan: {lookup:?}");
        return;
    };
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for x in 0..scan.rows.iter().map(Vec::len).min().unwrap_or(0) {
        let differs = scan.rows.iter().any(|row| {
            row[x]
                .iter()
                .zip(scan.background)
                .any(|(&a, b)| a.abs_diff(b) > 12)
        });
        match runs.last_mut() {
            Some(last) if differs && x <= last.1 + 3 => last.1 = x + 1,
            _ if differs => runs.push((x, x + 1)),
            _ => {}
        }
    }
    let width = scan.rows.iter().map(Vec::len).min().unwrap_or(0);
    for (name, row) in [
        ("first", 0),
        ("mid", scan.rows.len() / 2),
        ("last", scan.rows.len() - 1),
    ] {
        let colors: Vec<_> = (0..width).step_by(48).map(|x| scan.rows[row][x]).collect();
        println!("   row {name}: {colors:?}");
    }
    let tail: Vec<_> = runs.iter().rev().take(12).rev().collect();
    println!(
        "   scan: size={:?} caption_px={} bg={:?} ink={:?} rows={} runs={} last runs={tail:?}",
        scan.window_size,
        scan.caption_px,
        scan.background,
        scan.caption_ink,
        scan.rows.len(),
        runs.len()
    );
}
