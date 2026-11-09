//! A window with a custom title bar (no DWM caption-button bounds, like Firefox, Chromium or VS Code): the
//! button must not show before a scan of the painted window is there, and must stand where the scan says.
//! The scans come from an injected sampler that the test lets through when it wants.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetSystemMetrics, RegisterClassW, SM_CXSCREEN,
    SM_CYSCREEN, SW_SHOWNA, ShowWindow, WNDCLASSW, WS_POPUP, WS_THICKFRAME,
};
use windows::core::w;

use super::pb_common::shown_slot;
use super::support::{options, post, pump_ms, temp_dir};
use crate::Hwnd;
use crate::app::Runtime;
use crate::app::entry::Slot;
use crate::app::host::{WM_SWEEP_NOW, with_app};
use crate::color::{ColorCache, TitleSample, WM_COLOR_READY};
use crate::config::Settings;
use crate::placement::firefox_strip::FirefoxStrip;
use crate::win32::to_hwnd;
use crate::windows_filter::{DefaultPlacement, default_placement};

const WIDTH: i32 = 800;
const HEIGHT: i32 = 200;
const CORNER_MARGIN: i32 = 20;
/// Longest time the button may take to show once a usable scan is released (the sweep alone would take 2 s).
const SHOW_WITHIN: Duration = Duration::from_millis(1000);
const NO_SCAN_LOG_LINE: &str = "button hidden: no title bar scan";
const TIMED_OUT: Duration = Duration::from_millis(4500);
/// A failed first scan is retried by the cache after 5 s, picked up by the next 2 s sweep.
const RETRY_WINDOW: Duration = Duration::from_secs(8);
const BEFORE_SCAN: Duration = Duration::from_millis(600);
const BUTTON_GAP_PX: i32 = 4;

unsafe extern "system" fn proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(h, m, w, l) }
}

/// A popup with a resize frame and no caption: DWM reports no caption buttons, so the slot is estimated.
struct CustomWin(Hwnd);

impl CustomWin {
    fn new() -> CustomWin {
        // SAFETY: plain window creation on the test thread.
        unsafe {
            let hinst = GetModuleHandleW(None).unwrap();
            let _ = RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(proc),
                hInstance: hinst.into(),
                lpszClassName: w!("PinItF3Custom"),
                ..Default::default()
            });
            let (sw, sh) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
            let h = CreateWindowExW(
                Default::default(),
                w!("PinItF3Custom"),
                w!("pin-it-f3-custom"),
                WS_POPUP | WS_THICKFRAME,
                sw - WIDTH - CORNER_MARGIN,
                sh - HEIGHT - 2 * CORNER_MARGIN,
                WIDTH,
                HEIGHT,
                None,
                None,
                Some(hinst.into()),
                None,
            )
            .unwrap();
            let _ = ShowWindow(h, SW_SHOWNA);
            CustomWin(h.0 as Hwnd)
        }
    }

    fn placement(&self) -> DefaultPlacement {
        let placement = default_placement(self.0).expect("placement");
        assert!(
            placement.estimated,
            "the fixture must have an estimated slot"
        );
        placement
    }
}

impl Drop for CustomWin {
    fn drop(&mut self) {
        // SAFETY: own window.
        let _ = unsafe { DestroyWindow(to_hwnd(self.0)) };
        pump_ms(30);
    }
}

/// The Firefox-like strip of `win`, with its real window size and caption-button edge.
fn strip_of(win: &CustomWin) -> FirefoxStrip {
    let placement = win.placement();
    let (l, t, r, b) = placement.window_rect;
    FirefoxStrip {
        group_right: placement.group_right as usize,
        height: b - t,
        ..FirefoxStrip::new((r - l) as usize, true, 3)
    }
}

/// Where the button must stand: `BUTTON_GAP_PX` left of the chevron, at the default size.
fn expected_x(win: &CustomWin, strip: &FirefoxStrip) -> i32 {
    let placement = win.placement();
    placement.window_rect.0 + strip.content_left() as i32 - BUTTON_GAP_PX - placement.slot.size
}

/// Starts the runtime on `win` with a cache whose worker calls `sampler`.
fn start(
    win: &CustomWin,
    sampler: impl Fn(Hwnd) -> Option<TitleSample> + Send + Sync + 'static,
    tag: &str,
) -> (Runtime, std::path::PathBuf) {
    let dir = temp_dir(tag);
    let rt = Runtime::start(&options(&dir, vec![win.0], &Settings::default())).unwrap();
    with_app(|a| {
        a.colors = ColorCache::with_injected_sampler(sampler);
        a.colors.set_notify(a.hwnd, WM_COLOR_READY);
    });
    post(rt.hwnd(), WM_SWEEP_NOW, 0, 0);
    (rt, dir)
}

/// Every distinct state of the button over `span`, as (milliseconds since the start, slot).
fn watch(win: &CustomWin, span: Duration) -> Vec<(u128, Option<Slot>)> {
    let start = Instant::now();
    let mut seen: Vec<(u128, Option<Slot>)> = Vec::new();
    while start.elapsed() < span {
        pump_ms(2);
        let now = shown_slot(win.0);
        if seen.last().is_none_or(|&(_, last)| last != now) {
            seen.push((start.elapsed().as_millis(), now));
        }
    }
    seen
}

fn wait_for_slot(win: &CustomWin, within: Duration) -> Option<Slot> {
    let start = Instant::now();
    while start.elapsed() < within {
        pump_ms(5);
        if let Some(slot) = shown_slot(win.0) {
            return Some(slot);
        }
    }
    None
}

#[test]
fn a_custom_title_bar_shows_no_button_before_its_scan_and_then_the_right_one() {
    let _g = crate::window_test_guard();
    let win = CustomWin::new();
    let strip = strip_of(&win);
    let released = Arc::new(AtomicBool::new(false));
    let sample = FirefoxStrip::sample(strip.scan());
    let gate = released.clone();
    let (rt, dir) = start(
        &win,
        move |_| {
            while !gate.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(5));
            }
            Some(sample.clone())
        },
        "f3-delayed",
    );
    let before = watch(&win, BEFORE_SCAN);
    assert!(
        before.iter().all(|&(_, slot)| slot.is_none()),
        "shown before the scan: {before:?}"
    );
    released.store(true, Ordering::SeqCst);
    let slot = wait_for_slot(&win, SHOW_WITHIN).expect("shown after the scan");
    assert_eq!(slot.0, expected_x(&win, &strip));
    drop(rt);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_unpainted_scan_is_asked_for_again_until_the_window_has_painted() {
    let _g = crate::window_test_guard();
    let win = CustomWin::new();
    let strip = strip_of(&win);
    let calls = Arc::new(AtomicU32::new(0));
    let (unpainted, painted) = (
        FirefoxStrip::sample(strip.unpainted_scan()),
        FirefoxStrip::sample(strip.scan()),
    );
    let counter = calls.clone();
    let (rt, dir) = start(
        &win,
        move |_| {
            let call = counter.fetch_add(1, Ordering::SeqCst);
            Some(if call < 4 {
                unpainted.clone()
            } else {
                painted.clone()
            })
        },
        "f3-unpainted",
    );
    let seen = watch(&win, Duration::from_millis(1500));
    let shown: Vec<_> = seen.iter().filter_map(|&(_, slot)| slot).collect();
    assert_eq!(shown.len(), 1, "one slot only, the right one: {seen:?}");
    assert_eq!(shown[0].0, expected_x(&win, &strip));
    assert!(calls.load(Ordering::SeqCst) >= 5);
    drop(rt);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_scan_that_never_arrives_keeps_the_button_hidden_and_logs_once() {
    let _g = crate::window_test_guard();
    let win = CustomWin::new();
    let (rt, dir) = start(&win, |_| None, "f3-noscan");
    let seen = watch(&win, TIMED_OUT);
    assert!(
        seen.iter().all(|&(_, slot)| slot.is_none()),
        "shown without a scan: {seen:?}"
    );
    let log = std::fs::read_to_string(dir.join("pinit.log")).unwrap_or_default();
    let lines = log.lines().filter(|l| l.contains(NO_SCAN_LOG_LINE)).count();
    assert_eq!(lines, 1, "log:\n{log}");
    drop(rt);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_scan_that_arrives_after_the_wait_still_shows_the_button() {
    let _g = crate::window_test_guard();
    let win = CustomWin::new();
    let strip = strip_of(&win);
    let late = Arc::new(AtomicBool::new(false));
    let sample = FirefoxStrip::sample(strip.scan());
    let gate = late.clone();
    let (rt, dir) = start(
        &win,
        move |_| gate.load(Ordering::SeqCst).then(|| sample.clone()),
        "f3-late",
    );
    assert!(
        watch(&win, TIMED_OUT)
            .iter()
            .all(|&(_, slot)| slot.is_none())
    );
    late.store(true, Ordering::SeqCst);
    post(rt.hwnd(), WM_SWEEP_NOW, 0, 0);
    let slot = wait_for_slot(&win, RETRY_WINDOW).expect("shown after a late scan");
    assert_eq!(slot.0, expected_x(&win, &strip));
    drop(rt);
    let _ = std::fs::remove_dir_all(dir);
}
