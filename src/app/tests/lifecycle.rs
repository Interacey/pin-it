//! Start, shutdown, session end, single instance and re-entrancy.

use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::Threading::{CreateMutexW, GetCurrentThreadId};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, IsWindowVisible, MSG, PM_REMOVE, PeekMessageW, PostThreadMessageW, SendMessageW,
    WM_ENDSESSION, WM_QUERYENDSESSION, WM_QUIT,
};
use windows::core::w;

use super::support::{Wins, click, eventually_swept, options, post, pump_ms, temp_dir};
use crate::app::host::{WM_PIN_CLICK, WM_SWEEP_NOW};
use crate::app::{Runtime, run_with};
use crate::config::Settings;
use crate::win32::to_hwnd;
use crate::zorder::is_topmost;

#[test]
fn disabled_hides_buttons_and_ignores_clicks() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(1);
    let w = wins.0.clone();
    let dir = temp_dir("disabled");
    let settings = Settings {
        enabled: false,
        ..Settings::default()
    };
    let rt = Runtime::start(&options(&dir, w.clone(), &settings)).unwrap();
    pump_ms(100);
    let o = rt.button_hwnd(w[0]).unwrap();
    assert!(!unsafe { IsWindowVisible(to_hwnd(o)) }.as_bool());
    post(rt.hwnd(), WM_PIN_CLICK, w[0] as usize, 0);
    pump_ms(100);
    assert!(rt.pinned().is_empty());
    assert!(!is_topmost(w[0]));
    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn run_with_quits_on_wm_quit_and_cleans_up() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(1);
    let dir = temp_dir("runwith");
    let opts = options(&dir, wins.0.clone(), &Settings::default());
    let tid = unsafe { GetCurrentThreadId() };
    unsafe { PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0)).unwrap() };
    run_with(opts);
    assert!(
        unsafe { FindWindowW(w!("PinItMain"), None) }.is_err(),
        "main window left behind"
    );
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn second_instance_exits_silently() {
    let _g = crate::window_test_guard();
    let dir = temp_dir("single");
    let held = unsafe { CreateMutexW(None, false, w!("Local\\PinIt-single-instance")) }.unwrap();
    let mut opts = options(&dir, vec![], &Settings::default());
    opts.single_instance = true;
    run_with(opts); // must return at once, without a message loop
    assert!(unsafe { FindWindowW(w!("PinItMain"), None) }.is_err());
    let log = std::fs::read_to_string(dir.join("pinit.log")).unwrap_or_default();
    assert!(
        log.contains("another instance is already running"),
        "exit was not caused by the mutex; log: {log:?}"
    );
    unsafe {
        let _ = windows::Win32::Foundation::CloseHandle(held);
    }
    let _ = std::fs::remove_dir_all(dir);
}

fn quit_posted() -> bool {
    let mut msg = MSG::default();
    unsafe { PeekMessageW(&mut msg, None, WM_QUIT, WM_QUIT, PM_REMOVE) }.as_bool()
}

#[test]
fn end_session_shuts_down_only_after_a_query() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(1);
    let w = wins.0.clone();
    let dir = temp_dir("endsession");
    let rt = Runtime::start(&options(&dir, w.clone(), &Settings::default())).unwrap();
    pump_ms(100);
    click(rt.button_hwnd(w[0]).unwrap());
    eventually_swept(&rt, "w[0] topmost", || is_topmost(w[0]));
    let main = to_hwnd(rt.hwnd());

    // A spoofed WM_ENDSESSION without a query does nothing.
    unsafe { SendMessageW(main, WM_ENDSESSION, Some(WPARAM(1)), Some(LPARAM(0))) };
    assert_eq!(
        rt.pinned(),
        vec![w[0]],
        "ENDSESSION without query shut down"
    );
    assert!(!quit_posted());

    // A query followed by "cancelled" resets the flag.
    let r = unsafe { SendMessageW(main, WM_QUERYENDSESSION, None, None) };
    assert_eq!(r.0, 1);
    unsafe { SendMessageW(main, WM_ENDSESSION, Some(WPARAM(0)), Some(LPARAM(0))) };
    unsafe { SendMessageW(main, WM_ENDSESSION, Some(WPARAM(1)), Some(LPARAM(0))) };
    assert_eq!(rt.pinned(), vec![w[0]], "cancelled session end shut down");

    // Query, then the real end of the session.
    unsafe { SendMessageW(main, WM_QUERYENDSESSION, None, None) };
    unsafe { SendMessageW(main, WM_ENDSESSION, Some(WPARAM(1)), Some(LPARAM(0))) };
    assert!(rt.pinned().is_empty());
    assert!(!is_topmost(w[0]));
    assert!(quit_posted(), "no WM_QUIT after shutdown");

    // After shutdown nothing mutates state any more.
    post(rt.hwnd(), WM_PIN_CLICK, w[0] as usize, 0);
    post(rt.hwnd(), WM_SWEEP_NOW, 0, 0);
    pump_ms(100);
    assert!(rt.pinned().is_empty(), "pinned again after shutdown");
    assert!(!is_topmost(w[0]));
    assert_eq!(rt.button_count(), 0);
    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn message_during_an_app_method_is_posted_again_not_nested() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(1);
    let w = wins.0.clone();
    let dir = temp_dir("reentry");
    let rt = Runtime::start(&options(&dir, w.clone(), &Settings::default())).unwrap();
    pump_ms(100);
    rt.while_busy(|| {
        // Dispatched synchronously, like a sent message during a cross-thread call.
        unsafe {
            SendMessageW(
                to_hwnd(rt.hwnd()),
                WM_PIN_CLICK,
                Some(WPARAM(w[0] as usize)),
                None,
            )
        };
    });
    assert!(
        rt.pinned().is_empty(),
        "message was handled inside a running App method"
    );
    pump_ms(100);
    assert_eq!(rt.pinned(), vec![w[0]], "re-posted message was lost");
    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}
