//! Hotkey, tray commands and the enabled switch.

use std::time::Duration;

use super::support::{
    ForegroundOverride, OTHER_RARE_HOTKEY, RARE_HOTKEY, Wins, button_visible, click,
    eventually_swept, log_text, options, post, pump_ms, temp_dir,
};
use crate::Hwnd;
use crate::app::open_settings::recorder;
use crate::app::{FOREGROUND_OVERRIDE, Runtime};
use crate::config::{Settings, load_from, save_to};
use crate::hotkey::{self, Hotkey};
use crate::tray::TrayCommand;
use crate::zorder::is_topmost;
use windows::Win32::UI::WindowsAndMessaging::WM_HOTKEY;

#[test]
fn hotkey_toggles_the_foreground_window_only_when_allowed() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(2);
    let w = wins.0.clone();
    let dir = temp_dir("hotkey");
    let settings = Settings {
        hotkey: Some(RARE_HOTKEY),
        ..Settings::default()
    };
    let mut opts = options(&dir, vec![w[0]], &settings);
    opts.hotkey = true;
    let rt = Runtime::start(&opts).unwrap();
    if !rt.hotkey_registered() {
        eprintln!("SKIPPED hotkey test: Ctrl+Alt+Shift+F23 is taken by another program");
        drop(rt);
        drop(wins);
        let _ = std::fs::remove_dir_all(dir);
        return;
    }
    pump_ms(100);
    let _fg = ForegroundOverride::set(w[0]);
    post(rt.hwnd(), WM_HOTKEY, 1, 0);
    pump_ms(100);
    assert_eq!(rt.pinned(), vec![w[0]]);
    eventually_swept(&rt, "w[0] topmost", || is_topmost(w[0]));
    post(rt.hwnd(), WM_HOTKEY, 1, 0);
    pump_ms(100);
    assert!(rt.pinned().is_empty());

    // Another hotkey id, and a foreground window outside the allowed set, do nothing.
    post(rt.hwnd(), WM_HOTKEY, 2, 0);
    FOREGROUND_OVERRIDE.store(w[1], std::sync::atomic::Ordering::Relaxed);
    post(rt.hwnd(), WM_HOTKEY, 1, 0);
    pump_ms(100);
    assert!(rt.pinned().is_empty());
    assert!(!is_topmost(w[1]));
    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn hotkey_is_not_registered_until_one_is_chosen() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(1);
    let dir = temp_dir("hotkey-off");
    let mut opts = options(&dir, wins.0.clone(), &Settings::default());
    opts.hotkey = true;
    let rt = Runtime::start(&opts).unwrap();
    assert_eq!(rt.settings().hotkey, None);
    assert!(
        !rt.hotkey_registered(),
        "default settings registered a hotkey"
    );
    // a stray WM_HOTKEY does nothing while nothing is registered
    let _fg = ForegroundOverride::set(wins.0[0]);
    post(rt.hwnd(), WM_HOTKEY, 1, 0);
    pump_ms(100);
    assert!(rt.pinned().is_empty());

    rt.run_tray_command(&TrayCommand::SetHotkey(RARE_HOTKEY));
    if rt.hotkey_registered() {
        assert_eq!(rt.settings().hotkey, Some(RARE_HOTKEY));
        rt.run_tray_command(&TrayCommand::ClearHotkey);
        assert!(!rt.hotkey_registered(), "ClearHotkey kept the registration");
        assert_eq!(rt.settings().hotkey, None);
    } else {
        eprintln!("SKIPPED register part: Ctrl+Alt+Shift+F23 is taken by another program");
    }
    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn tray_command_keeps_hand_edits_of_the_settings_file() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(1);
    let dir = temp_dir("handedit");
    let opts = options(&dir, wins.0.clone(), &Settings::default());
    let path = opts.settings_path.clone();
    let rt = Runtime::start(&opts).unwrap();
    std::thread::sleep(Duration::from_millis(50));
    let edited = Settings {
        button_scale_percent: 150,
        ..Settings::default()
    };
    save_to(&path, &edited).unwrap();
    rt.run_tray_command(&TrayCommand::ToggleAnimation);
    let saved = load_from(&path);
    assert_eq!(saved.button_scale_percent, 150, "hand edit was overwritten");
    assert!(!saved.animate, "the command was not applied");
    assert_eq!(rt.settings().button_scale_percent, 150);
    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn switching_off_unpins_everything_and_hides_the_buttons() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(2);
    let w = wins.0.clone();
    let dir = temp_dir("switchoff");
    let rt = Runtime::start(&options(&dir, w.clone(), &Settings::default())).unwrap();
    pump_ms(100);
    click(rt.button_hwnd(w[0]).unwrap());
    click(rt.button_hwnd(w[1]).unwrap());
    eventually_swept(&rt, "both topmost", || is_topmost(w[0]) && is_topmost(w[1]));
    rt.run_tray_command(&TrayCommand::ToggleEnabled);
    pump_ms(50);
    assert!(rt.pinned().is_empty());
    assert!(!is_topmost(w[0]) && !is_topmost(w[1]));
    assert!(!button_visible(&rt, w[0]) && !button_visible(&rt, w[1]));
    rt.run_tray_command(&TrayCommand::ToggleEnabled);
    pump_ms(50);
    assert!(button_visible(&rt, w[0]), "buttons did not come back");
    assert!(rt.pinned().is_empty(), "pins were restored");
    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}

/// Registers `hotkey` on a window outside the app, so the app's own attempt must fail.
/// `None` (the test then skips) when another program holds the combination.
fn block_hotkey(hotkey: &Hotkey, blocker: &Wins) -> Option<Hwnd> {
    let owner = blocker.0[0];
    hotkey::register(owner, BLOCKER_HOTKEY_ID, hotkey)
        .is_ok()
        .then_some(owner)
}

const BLOCKER_HOTKEY_ID: i32 = 9;

#[test]
fn failed_hotkey_registration_is_not_kept_in_settings_file_or_menu() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(1);
    let blocker = Wins::new(1);
    let Some(owner) = block_hotkey(&OTHER_RARE_HOTKEY, &blocker) else {
        eprintln!("SKIPPED: Ctrl+Alt+Shift+F22 is taken by another program");
        return;
    };
    let dir = temp_dir("hotkey-conflict");
    let mut opts = options(&dir, wins.0.clone(), &Settings::default());
    opts.hotkey = true;
    let rt = Runtime::start(&opts).unwrap();

    rt.run_tray_command(&TrayCommand::SetHotkey(OTHER_RARE_HOTKEY));
    assert_eq!(rt.settings().hotkey, None, "failed hotkey stayed in memory");
    assert_eq!(
        load_from(&opts.settings_path).hotkey,
        None,
        "and in the file"
    );
    assert!(!rt.hotkey_registered());
    assert_eq!(
        rt.menu_inputs().0.hotkey,
        None,
        "menu would show it checked"
    );
    let warnings = |dir: &_| log_text(dir).matches("not available").count();
    assert_eq!(warnings(&dir), 1);

    // later settings changes do not repeat the warning
    rt.run_tray_command(&TrayCommand::ToggleAnimation);
    assert_eq!(warnings(&dir), 1, "{}", log_text(&dir));

    hotkey::unregister(owner, BLOCKER_HOTKEY_ID);
    drop(rt);
    drop(blocker);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn failed_hotkey_registration_goes_back_to_the_previous_hotkey() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(1);
    let blocker = Wins::new(1);
    let Some(owner) = block_hotkey(&OTHER_RARE_HOTKEY, &blocker) else {
        eprintln!("SKIPPED: Ctrl+Alt+Shift+F22 is taken by another program");
        return;
    };
    let dir = temp_dir("hotkey-previous");
    let settings = Settings {
        hotkey: Some(RARE_HOTKEY),
        ..Settings::default()
    };
    let mut opts = options(&dir, wins.0.clone(), &settings);
    opts.hotkey = true;
    let rt = Runtime::start(&opts).unwrap();
    if rt.hotkey_registered() {
        rt.run_tray_command(&TrayCommand::SetHotkey(OTHER_RARE_HOTKEY));
        assert_eq!(rt.settings().hotkey, Some(RARE_HOTKEY));
        assert_eq!(load_from(&opts.settings_path).hotkey, Some(RARE_HOTKEY));
        assert!(rt.hotkey_registered(), "previous registration was lost");
    } else {
        eprintln!("SKIPPED: Ctrl+Alt+Shift+F23 is taken by another program");
    }
    hotkey::unregister(owner, BLOCKER_HOTKEY_ID);
    drop(rt);
    drop(blocker);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn menu_is_built_from_the_file_as_it_is_now() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(1);
    let dir = temp_dir("menu-reload");
    let opts = options(&dir, wins.0.clone(), &Settings::default());
    let rt = Runtime::start(&opts).unwrap();
    std::thread::sleep(Duration::from_millis(50));
    let edited = Settings {
        enabled: false,
        ..Settings::default()
    };
    save_to(&opts.settings_path, &edited).unwrap();
    let (menu_settings, _) = rt.menu_inputs();
    assert!(
        !menu_settings.enabled,
        "menu used the settings from before the edit"
    );
    assert!(!rt.settings().enabled);
    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn open_settings_file_runs_outside_the_app_borrow() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(1);
    let dir = temp_dir("open-settings");
    let opts = options(&dir, wins.0.clone(), &Settings::default());
    let rt = Runtime::start(&opts).unwrap();
    recorder::start_recording();
    rt.run_tray_command(&TrayCommand::OpenSettingsFile);
    pump_ms(100);
    let calls = recorder::take_recorded();
    assert_eq!(calls.len(), 1, "settings file was not opened exactly once");
    assert_eq!(calls[0].path, opts.settings_path);
    assert!(!calls[0].app_busy, "ShellExecute ran inside an App method");
    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}
