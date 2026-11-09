//! The excluded apps window and the app together: the window's actions travel as
//! `WM_EXCLUSIONS_CHANGED` through the normal command path, the observers react, and the window
//! shows the settings afterwards.

use std::cell::RefCell;
use std::rc::Rc;

use windows::Win32::Foundation::WPARAM;
use windows::Win32::UI::Input::KeyboardAndMouse::VK_RETURN;
use windows::Win32::UI::WindowsAndMessaging::{
    IsWindow, SendMessageW, SetWindowTextW, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP,
};
use windows::core::PCWSTR;

use super::support::{CHANGED, Recorder, Wins, eventually_swept, options, post, pump_ms, temp_dir};
use crate::Hwnd;
use crate::app::Runtime;
use crate::app::host::WM_EXCLUSIONS_CHANGED;
use crate::app::settings_observers::SettingsEvent;
use crate::config::{Settings, load_from};
use crate::exclusions::{self, drain_actions, probe};
use crate::tray::TrayCommand;
use crate::win32::{to_hwnd, wide};

fn own_exe_name() -> String {
    std::env::current_exe()
        .unwrap()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_lowercase()
}

/// Closes the window when a test ends, also when it fails.
struct CloseWindow;

impl Drop for CloseWindow {
    fn drop(&mut self) {
        let _ = exclusions::close();
        drain_actions();
        pump_ms(20);
    }
}

fn is_open(h: Hwnd) -> bool {
    unsafe { IsWindow(Some(to_hwnd(h))).as_bool() }
}

fn type_and_enter(text: &str) {
    let edit = to_hwnd(probe::edit().unwrap());
    let wide = wide(text);
    unsafe { SetWindowTextW(edit, PCWSTR(wide.as_ptr())).unwrap() };
    pump_ms(10);
    post(probe::edit().unwrap(), WM_KEYDOWN, VK_RETURN.0 as usize, 0);
}

fn click_trash(h: Hwnd, row: usize) {
    let (x, y) = probe::trash_point(row).unwrap();
    let lparam = ((y as isize & 0xFFFF) << 16) | (x as isize & 0xFFFF);
    post(h, WM_LBUTTONDOWN, 0, lparam);
    pump_ms(10);
    post(h, WM_LBUTTONUP, 0, lparam);
}

#[test]
fn adding_the_own_exe_through_the_window_removes_the_buttons_and_removing_it_brings_them_back() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(2);
    let dir = temp_dir("exclusions-flow");
    let opts = options(&dir, wins.0.clone(), &Settings::default());
    let path = opts.settings_path.clone();
    let rt = Runtime::start(&opts).unwrap();
    let _close = CloseWindow;
    eventually_swept(&rt, "both windows have a button", || rt.button_count() == 2);
    let seen: Rc<RefCell<Vec<SettingsEvent>>> = Rc::default();
    let sink = Rc::clone(&seen);
    rt.settings_events()
        .subscribe_fn(move |event, _| sink.borrow_mut().push(*event));

    let h = exclusions::open(rt.hwnd(), WM_EXCLUSIONS_CHANGED, &[]).unwrap();
    type_and_enter(&own_exe_name().to_uppercase());
    eventually_swept(&rt, "buttons removed", || rt.button_count() == 0);

    assert_eq!(*seen.borrow(), [SettingsEvent::Changed]);
    assert_eq!(rt.settings().excluded_apps, [own_exe_name()]);
    assert_eq!(
        load_from(&path).excluded_apps,
        [own_exe_name()],
        "not saved"
    );
    assert_eq!(
        probe::entries(),
        Some(vec![own_exe_name()]),
        "window shows the truth"
    );
    pump_ms(700);
    assert_eq!(probe::flashing(0), Some(false), "the flash ended");

    click_trash(h, 0);
    eventually_swept(&rt, "buttons are back", || rt.button_count() == 2);
    assert!(rt.settings().excluded_apps.is_empty());
    assert!(load_from(&path).excluded_apps.is_empty());
    assert_eq!(probe::entries(), Some(vec![]));
    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_message_that_arrives_while_the_app_is_busy_is_posted_again_and_applied() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(1);
    let dir = temp_dir("exclusions-busy");
    let rt = Runtime::start(&options(&dir, wins.0.clone(), &Settings::default())).unwrap();
    let _close = CloseWindow;
    // The window announces to a recorder, so the action waits in the queue.
    let rec = Recorder::new();
    drain_actions();
    exclusions::open(rec.0, CHANGED, &[]).unwrap();
    type_and_enter("calc");
    pump_ms(30);

    rt.while_busy(|| {
        // Dispatched synchronously, like a sent message during a cross-thread call.
        unsafe {
            SendMessageW(
                to_hwnd(rt.hwnd()),
                WM_EXCLUSIONS_CHANGED,
                Some(WPARAM(0)),
                None,
            )
        };
    });
    assert!(
        rt.settings().excluded_apps.is_empty(),
        "handled inside a running App method"
    );
    pump_ms(100);

    assert_eq!(
        rt.settings().excluded_apps,
        ["calc.exe"],
        "message was lost"
    );
    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_message_without_queued_actions_changes_nothing() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(1);
    let dir = temp_dir("exclusions-empty");
    let rt = Runtime::start(&options(&dir, wins.0.clone(), &Settings::default())).unwrap();
    drain_actions();
    let seen: Rc<RefCell<Vec<SettingsEvent>>> = Rc::default();
    let sink = Rc::clone(&seen);
    rt.settings_events()
        .subscribe_fn(move |event, _| sink.borrow_mut().push(*event));
    post(rt.hwnd(), WM_EXCLUSIONS_CHANGED, 12345, 678);
    pump_ms(100);
    assert!(seen.borrow().is_empty());
    assert_eq!(rt.settings(), Settings::default());
    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_tray_command_opens_the_window_with_the_current_list_once_and_shutdown_closes_it() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(1);
    let dir = temp_dir("exclusions-open");
    let settings = Settings {
        excluded_apps: vec!["a.exe".into(), "b.exe".into()],
        ..Settings::default()
    };
    let rt = Runtime::start(&options(&dir, wins.0.clone(), &settings)).unwrap();
    let _close = CloseWindow;
    assert_eq!(probe::entries(), None);
    rt.run_tray_command(&TrayCommand::OpenExclusions);
    let h = probe::window();
    assert!(is_open(h));
    assert_eq!(probe::entries(), Some(vec!["a.exe".into(), "b.exe".into()]));
    rt.run_tray_command(&TrayCommand::OpenExclusions);
    assert_eq!(probe::window(), h, "a second request uses the same window");

    // Any change of the exclusions, whoever made it, shows in the open window.
    rt.run_tray_command(&TrayCommand::AddExcludedApp("C".into()));
    assert_eq!(
        probe::entries(),
        Some(vec!["a.exe".into(), "b.exe".into(), "c.exe".into()])
    );
    rt.run_tray_command(&TrayCommand::RemoveExcludedApp("a.exe".into()));
    assert_eq!(probe::entries(), Some(vec!["b.exe".into(), "c.exe".into()]));

    drop(rt);
    assert!(!is_open(h), "the app's shutdown must destroy the window");
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}
