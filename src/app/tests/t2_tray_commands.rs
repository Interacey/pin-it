//! Tray commands as the user meets them: the popups must survive the focus moving away right after
//! the menu closed, "Open settings file" falls back to Notepad, and the log tells what happened.
//! Own windows only; the focus change is simulated by sending `WM_ACTIVATE` to the popup.

use windows::Win32::Foundation::WPARAM;
use windows::Win32::UI::WindowsAndMessaging::{
    IsWindow, SendMessageW, WA_ACTIVE, WA_INACTIVE, WM_ACTIVATE,
};

use super::support::{Wins, eventually_swept, log_text, options, pump_ms, temp_dir};
use crate::Hwnd;
use crate::app::Runtime;
use crate::app::host::with_app;
use crate::app::open_settings::recorder::{self, Opener};
use crate::config::Settings;
use crate::dial;
use crate::exclusions;
use crate::tray::TrayCommand;
use crate::win32::to_hwnd;

/// Longer than the opening grace of the popups.
const PAST_GRACE_MS: u64 = 700;

/// Closes the popups when a test ends, also when it fails.
struct ClosePopups;

impl Drop for ClosePopups {
    fn drop(&mut self) {
        let _ = dial::close();
        let _ = exclusions::close();
        exclusions::drain_actions();
        pump_ms(20);
    }
}

fn is_open(h: Hwnd) -> bool {
    h != 0 && unsafe { IsWindow(Some(to_hwnd(h))).as_bool() }
}

fn send_activation(h: Hwnd, state: u32) {
    unsafe { SendMessageW(to_hwnd(h), WM_ACTIVATE, Some(WPARAM(state as usize)), None) };
}

/// The popup gets the focus and loses it again at once, like when Windows gives the foreground
/// back to the previous window after the tray menu closed.
fn flip_focus_away(popup: Hwnd) {
    send_activation(popup, WA_ACTIVE);
    send_activation(popup, WA_INACTIVE);
}

fn run_from_menu(command: TrayCommand) {
    with_app(|app| app.run_tray_command(&command)).expect("app is idle");
    pump_ms(20);
}

fn set_stored_exe(rt: &Runtime, target: Hwnd, exe: Option<&str>) {
    // SAFETY: short-lived access on the owning thread, outside any message dispatch.
    let app = unsafe { &mut *rt.ptr };
    app.entries
        .get_mut(&target)
        .expect("window has a button")
        .exe = exe.map(str::to_string);
}

#[test]
fn the_size_dial_stays_open_when_the_focus_moves_away_right_after_it_opened() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(1);
    let dir = temp_dir("t2-dial");
    let rt = Runtime::start(&options(&dir, wins.0.clone(), &Settings::default())).unwrap();
    let _close = ClosePopups;

    run_from_menu(TrayCommand::OpenSizeDial);
    let popup = dial::probe::window();
    assert!(is_open(popup), "the dial did not open");
    flip_focus_away(popup);
    pump_ms(PAST_GRACE_MS);
    assert!(is_open(popup), "the dial closed on the focus handed back");

    // The user clicks the dial (it gets the focus), then clicks away: now it closes.
    send_activation(popup, WA_ACTIVE);
    send_activation(popup, WA_INACTIVE);
    assert!(!is_open(popup), "leaving the focused dial must close it");
    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_excluded_apps_window_stays_open_when_the_focus_moves_away_right_after_it_opened() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(1);
    let dir = temp_dir("t2-exclusions");
    let rt = Runtime::start(&options(&dir, wins.0.clone(), &Settings::default())).unwrap();
    let _close = ClosePopups;

    run_from_menu(TrayCommand::OpenExclusions);
    let popup = exclusions::probe::window();
    assert!(is_open(popup), "the window did not open");
    flip_focus_away(popup);
    pump_ms(PAST_GRACE_MS);
    assert!(is_open(popup), "the window closed on the focus handed back");

    send_activation(popup, WA_ACTIVE);
    send_activation(popup, WA_INACTIVE);
    assert!(!is_open(popup), "leaving the focused window must close it");
    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_default_app_that_fails_makes_the_settings_file_open_in_notepad() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(1);
    let dir = temp_dir("t2-notepad");
    let opts = options(&dir, wins.0.clone(), &Settings::default());
    let rt = Runtime::start(&opts).unwrap();

    recorder::start_recording_with_failing_default_app();
    run_from_menu(TrayCommand::OpenSettingsFile);
    pump_ms(100);
    let calls = recorder::take_recorded();

    let path = &opts.settings_path;
    let openers: Vec<_> = calls.iter().map(|c| c.opener.clone()).collect();
    let notepad = Opener::Notepad(format!("notepad.exe \"{}\"", path.display()));
    assert_eq!(openers, [Opener::DefaultApp, notepad]);
    assert!(calls.iter().all(|c| &c.path == path && !c.app_busy));
    let log = log_text(&dir);
    assert!(
        log.contains(" INFO opening settings file with Notepad (no default app for .json"),
        "fallback not logged as INFO: {log}"
    );
    assert!(
        !log.contains(" WARN "),
        "the expected fallback must not warn: {log}"
    );
    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_working_default_app_does_not_start_notepad() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(1);
    let dir = temp_dir("t2-default-app");
    let opts = options(&dir, wins.0.clone(), &Settings::default());
    let rt = Runtime::start(&opts).unwrap();

    recorder::start_recording();
    run_from_menu(TrayCommand::OpenSettingsFile);
    pump_ms(100);
    let openers: Vec<_> = recorder::take_recorded()
        .into_iter()
        .map(|c| c.opener)
        .collect();

    assert_eq!(openers, [Opener::DefaultApp]);
    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn every_tray_command_is_logged_when_it_arrives_and_when_it_is_done() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(1);
    let dir = temp_dir("t2-log");
    let rt = Runtime::start(&options(&dir, wins.0.clone(), &Settings::default())).unwrap();
    let _close = ClosePopups;

    run_from_menu(TrayCommand::OpenSizeDial);
    run_from_menu(TrayCommand::ToggleAnimation);

    let log = log_text(&dir);
    let lines: Vec<&str> = log.lines().filter(|l| l.contains("tray command")).collect();
    assert_eq!(lines.len(), 4, "{log}");
    assert!(lines[0].ends_with("tray command: OpenSizeDial"), "{log}");
    assert!(lines[1].ends_with("tray command done"), "{log}");
    assert!(lines[2].ends_with("tray command: ToggleAnimation"), "{log}");
    assert!(lines[3].ends_with("tray command done"), "{log}");
    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_exclusion_also_removes_a_button_whose_stored_exe_is_stale_or_missing() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(2);
    let dir = temp_dir("t2-stale-exe");
    let rt = Runtime::start(&options(&dir, wins.0.clone(), &Settings::default())).unwrap();
    eventually_swept(&rt, "both windows have a button", || rt.button_count() == 2);
    // What a UWP frame reports before its app window exists, and what a failed lookup leaves.
    set_stored_exe(&rt, wins.0[0], Some("applicationframehost.exe"));
    set_stored_exe(&rt, wins.0[1], None);

    let own = std::env::current_exe().unwrap();
    let own = own.file_name().unwrap().to_string_lossy().into_owned();
    with_app(|app| app.run_tray_command(&TrayCommand::AddExcludedApp(own))).unwrap();

    assert_eq!(rt.button_count(), 0, "buttons of an excluded app stayed");
    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}
