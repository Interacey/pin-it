//! Helpers shared by the app tests: own test windows, pumping, clicking, z-order probes,
//! and test-only hooks on `Runtime`.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GW_HWNDPREV,
    GetSystemMetrics, GetWindow, GetWindowRect, GetWindowThreadProcessId, IsWindowVisible, MSG,
    PM_REMOVE, PeekMessageW, PostMessageW, RegisterClassW, SM_CXSCREEN, SM_CYSCREEN, SW_SHOWNA,
    ShowWindow, TranslateMessage, WM_APP, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WNDCLASSW,
    WS_OVERLAPPEDWINDOW, WS_POPUP,
};
use windows::core::w;

use crate::Hwnd;
use crate::app::host::WM_SWEEP_NOW;
use crate::app::host::with_app;
use crate::app::observer::Subject;
use crate::app::pin_observers::PinEvent;
use crate::app::settings_observers::SettingsEvent;
use crate::app::state::App;
use crate::app::{FOREGROUND_OVERRIDE, RunOptions, Runtime};
use crate::config::{Settings, save_to};
use crate::events::WinEvent;
use crate::hotkey::{self, Hotkey};
use crate::pin_stack::PinStack;
use crate::tray::{Foreground, TrayCommand};
use crate::win32::to_hwnd;
use crate::zorder::{is_topmost, z_order_snapshot};

unsafe extern "system" fn test_proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(h, m, w, l) }
}

pub(super) fn pump_ms(ms: u64) {
    let end = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < end {
        pump_pending();
        std::thread::sleep(Duration::from_millis(4));
    }
}

/// Dispatches the messages that are queued right now.
fn pump_pending() {
    let mut msg = MSG::default();
    unsafe {
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Longest wait for a state the system settles on its own (z-order, visibility, pins).
const SETTLE_TIMEOUT: Duration = Duration::from_secs(3);
const SETTLE_STEP: Duration = Duration::from_millis(25);

/// Pumps messages until `condition` holds or `SETTLE_TIMEOUT` has passed; then fails with `what`
/// and the foreground window, which decides whether the system honours a pin.
pub(super) fn eventually(what: &str, condition: impl FnMut() -> bool) {
    poll(what, condition, || {});
}

/// Like `eventually`, and sweeps between the checks. The system may refuse the first pin of a test
/// window; the product keeps the pin and the sweep applies it again, so a pin counts as settled
/// only when this returns.
pub(super) fn eventually_swept(rt: &Runtime, what: &str, condition: impl FnMut() -> bool) {
    poll(what, condition, || post(rt.hwnd(), WM_SWEEP_NOW, 0, 0));
}

fn poll(what: &str, mut condition: impl FnMut() -> bool, mut between_checks: impl FnMut()) {
    let deadline = Instant::now() + SETTLE_TIMEOUT;
    loop {
        pump_pending();
        if condition() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "not settled within {SETTLE_TIMEOUT:?}: {what}; {}",
            crate::describe_foreground()
        );
        between_checks();
        std::thread::sleep(SETTLE_STEP);
    }
}

/// Small test windows at the lower right, destroyed on drop.
pub(super) struct Wins(pub(super) Vec<Hwnd>);

impl Wins {
    pub(super) fn new(n: usize) -> Wins {
        unsafe {
            let hinst = GetModuleHandleW(None).unwrap();
            let _ = RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(test_proc),
                hInstance: hinst.into(),
                lpszClassName: w!("PinItAppTest"),
                ..Default::default()
            });
            let (sw, sh) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
            let mut v = Wins(Vec::new());
            for i in 0..n as i32 {
                let h = CreateWindowExW(
                    Default::default(),
                    w!("PinItAppTest"),
                    w!("pin-it-app-test"),
                    WS_OVERLAPPEDWINDOW,
                    sw - 500 + i * 40,
                    sh - 330 + i * 24,
                    440,
                    110,
                    None,
                    None,
                    Some(hinst.into()),
                    None,
                )
                .unwrap();
                v.0.push(h.0 as isize);
                // SW_SHOWNOACTIVATE on the first window of a thread made the system ignore a later
                // HWND_TOPMOST for it in about half of the runs (measured); SW_SHOWNA does not.
                let _ = ShowWindow(h, SW_SHOWNA);
            }
            eventually("test windows visible", || {
                v.0.iter().all(|&h| IsWindowVisible(to_hwnd(h)).as_bool())
            });
            v
        }
    }
}

impl Drop for Wins {
    fn drop(&mut self) {
        for &h in &self.0 {
            unsafe {
                let _ = DestroyWindow(to_hwnd(h));
            }
        }
        pump_ms(30);
    }
}

/// Message the recorder window receives from a popup (the app uses its own message ids).
pub(super) const CHANGED: u32 = WM_APP + 77;

static RECEIVED: Mutex<Vec<usize>> = Mutex::new(Vec::new());

unsafe extern "system" fn recorder_proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if m == CHANGED {
        RECEIVED.lock().unwrap_or_else(|e| e.into_inner()).push(w.0);
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(h, m, w, l) }
}

pub(super) fn received() -> Vec<usize> {
    RECEIVED.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// A hidden window that records the wparam of each `CHANGED` message.
pub(super) struct Recorder(pub(super) Hwnd);

impl Recorder {
    pub(super) fn new() -> Self {
        RECEIVED.lock().unwrap_or_else(|e| e.into_inner()).clear();
        unsafe {
            let hinst = GetModuleHandleW(None).unwrap();
            let _ = RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(recorder_proc),
                hInstance: hinst.into(),
                lpszClassName: w!("PinItRecorder"),
                ..Default::default()
            });
            let h = CreateWindowExW(
                Default::default(),
                w!("PinItRecorder"),
                w!("recorder"),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(hinst.into()),
                None,
            )
            .unwrap();
            Recorder(h.0 as Hwnd)
        }
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(to_hwnd(self.0));
        }
    }
}

/// Ctrl+Alt+Shift+F23, which no program uses; tests that need a hotkey take this one.
pub(super) const RARE_HOTKEY: Hotkey = Hotkey {
    ctrl: true,
    alt: true,
    shift: true,
    win: false,
    vk: 0x86,
};

/// Ctrl+Alt+Shift+F22, the second combination tests use when they need a conflict.
pub(super) const OTHER_RARE_HOTKEY: Hotkey = Hotkey {
    vk: 0x85,
    ..RARE_HOTKEY
};

/// Content of `pinit.log` next to the settings file of `dir`; empty when there is none.
pub(super) fn log_text(dir: &std::path::Path) -> String {
    std::fs::read_to_string(dir.join("pinit.log")).unwrap_or_default()
}

pub(super) fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pinit-app-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

pub(super) fn options(dir: &std::path::Path, only: Vec<Hwnd>, settings: &Settings) -> RunOptions {
    let path = dir.join("settings.json");
    save_to(&path, settings).unwrap();
    RunOptions {
        settings_path: path,
        tray: false,
        only: Some(only),
        single_instance: false,
        hotkey: false,
        autostart: false,
    }
}

/// Z-order snapshot reduced to this process's windows.
pub(super) fn own_snapshot() -> Vec<Hwnd> {
    let pid = std::process::id();
    z_order_snapshot()
        .into_iter()
        .filter(|&h| {
            let mut p = 0u32;
            unsafe { GetWindowThreadProcessId(to_hwnd(h), Some(&mut p)) };
            p == pid
        })
        .collect()
}

pub(super) fn pos(h: Hwnd) -> usize {
    own_snapshot()
        .iter()
        .position(|&x| x == h)
        .expect("window in z-order")
}

pub(super) fn post(h: Hwnd, m: u32, wp: usize, lp: isize) {
    unsafe { PostMessageW(Some(to_hwnd(h)), m, WPARAM(wp), LPARAM(lp)).unwrap() };
}

pub(super) fn click(overlay: Hwnd) {
    let mut r = RECT::default();
    unsafe { GetWindowRect(to_hwnd(overlay), &mut r).unwrap() };
    let (cx, cy) = ((r.right - r.left) / 2, (r.bottom - r.top) / 2);
    let lp = ((cy as isize) << 16) | cx as isize;
    for m in [WM_MOUSEMOVE, WM_LBUTTONDOWN, WM_LBUTTONUP] {
        post(overlay, m, 0, lp);
        pump_ms(10);
    }
    pump_ms(80);
}

/// The overlay is unowned and must sit immediately above its target in the global z-order.
pub(super) fn overlay_directly_above(overlay: Hwnd, target: Hwnd) -> bool {
    unsafe { GetWindow(to_hwnd(target), GW_HWNDPREV) }.ok() == Some(to_hwnd(overlay))
}

pub(super) fn dump(tag: &str, w: &[Hwnd], ov: &[Hwnd]) {
    eprintln!(
        "{tag}: z A={} B={} C={} | oA={} oB={} oC={} | topmost {} {} {}",
        pos(w[0]),
        pos(w[1]),
        pos(w[2]),
        pos(ov[0]),
        pos(ov[1]),
        pos(ov[2]),
        is_topmost(w[0]),
        is_topmost(w[1]),
        is_topmost(w[2])
    );
}

pub(super) fn sweep(rt: &Runtime) {
    post(rt.hwnd(), WM_SWEEP_NOW, 0, 0);
    pump_ms(100);
}

pub(super) fn button_visible(rt: &Runtime, target: Hwnd) -> bool {
    unsafe { IsWindowVisible(to_hwnd(rt.button_hwnd(target).unwrap())) }.as_bool()
}

/// Clears the foreground override on drop.
pub(super) struct ForegroundOverride;

impl ForegroundOverride {
    pub(super) fn set(h: Hwnd) -> Self {
        FOREGROUND_OVERRIDE.store(h, std::sync::atomic::Ordering::Relaxed);
        ForegroundOverride
    }
}

impl Drop for ForegroundOverride {
    fn drop(&mut self) {
        FOREGROUND_OVERRIDE.store(0, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Test-only hooks into a running `Runtime`.
impl Runtime {
    /// Feeds an event as if a hook had delivered it (hooks skip this process's own windows).
    pub(super) fn inject_event(&self, ev: WinEvent) {
        with_app(|a| a.handle_event(ev));
    }

    /// Runs a tray command as if it had been chosen in the menu.
    pub(super) fn run_tray_command(&self, cmd: &TrayCommand) {
        with_app(|a| a.run_command(cmd));
    }

    /// Runs `f` while an `App` method counts as running, to probe the re-entrancy guard.
    pub(super) fn while_busy(&self, f: impl FnOnce()) {
        with_app(|_| f());
    }

    pub(super) fn hotkey_registered(&self) -> bool {
        // SAFETY: short-lived read on the owning thread, outside any message dispatch.
        unsafe { (*self.ptr).hotkey.active.is_some() }
    }

    /// What the next tray menu would be built from.
    pub(super) fn menu_inputs(&self) -> (Settings, Foreground) {
        with_app(|a| a.menu_inputs()).expect("app is idle")
    }

    pub(super) fn tray_added(&self) -> bool {
        // SAFETY: see `hotkey_registered`.
        unsafe { (*self.ptr).tray.as_ref().is_some_and(|t| t.is_added()) }
    }

    /// Takes the icon out of the notification area, like an explorer restart does.
    pub(super) fn remove_tray_icon(&self) {
        // SAFETY: see `hotkey_registered`.
        if let Some(tray) = unsafe { (*self.ptr).tray.as_ref() } {
            tray.remove_icon();
        }
    }

    /// Runs the add step again while the icon is in place, which the shell refuses.
    pub(super) fn add_tray_icon_again(&self) {
        with_app(|a| a.add_tray_icon());
    }

    pub(super) fn settings(&self) -> Settings {
        // SAFETY: see `hotkey_registered`.
        unsafe { (*self.ptr).settings.clone() }
    }

    /// The subject that announces pin changes; tests subscribe to watch the events.
    pub(super) fn pin_events(&self) -> Subject<PinEvent, App> {
        // SAFETY: see `hotkey_registered`.
        unsafe { (*self.ptr).pin_events.clone() }
    }

    /// The subject that announces applied settings.
    pub(super) fn settings_events(&self) -> Subject<SettingsEvent, App> {
        // SAFETY: see `hotkey_registered`.
        unsafe { (*self.ptr).settings_events.clone() }
    }

    /// Simulates a crash: tears the runtime down without unpinning anything or touching
    /// `pins.json`, like a killed process would leave things.
    pub(super) fn crash(self) {
        // SAFETY: see `Drop`; pins are only forgotten, never released.
        unsafe {
            let app = &mut *self.ptr;
            app.pins = PinStack::new();
            app.entries.clear();
            app.hooks = None;
            app.tray = None;
            if app.hotkey.active.take().is_some() {
                hotkey::unregister(app.hwnd, crate::app::host::HOTKEY_ID);
            }
            crate::app::host::clear_app();
            drop(Box::from_raw(self.ptr));
            let _ = DestroyWindow(to_hwnd(self.hwnd));
        }
        std::mem::forget(self);
    }
}
