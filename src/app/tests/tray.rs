//! Tray commands that need a running app: pin the current window, tray icon retry.

use super::support::{
    ForegroundOverride, Wins, eventually_swept, log_text, options, pump_ms, sweep, temp_dir,
};
use crate::app::Runtime;
use crate::config::{Settings, save_to};
use crate::tray::TrayCommand;
use crate::zorder::is_topmost;
use std::time::Duration;

fn own_exe_name() -> String {
    std::env::current_exe()
        .unwrap()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_lowercase()
}

#[test]
fn pin_current_window_toggles_the_last_foreground_window() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(2);
    let w = wins.0.clone();
    let dir = temp_dir("pin-current");
    let rt = Runtime::start(&options(&dir, vec![w[0]], &Settings::default())).unwrap();

    // No known foreground window: the entry is disabled and the command does nothing.
    let (_, foreground) = rt.menu_inputs();
    assert!(!foreground.can_pin && foreground.exe.is_none());
    rt.run_tray_command(&TrayCommand::PinCurrentWindow);
    assert!(rt.pinned().is_empty());

    let fg = ForegroundOverride::set(w[0]);
    sweep(&rt);
    let (_, foreground) = rt.menu_inputs();
    assert!(foreground.can_pin, "{foreground:?}");
    assert_eq!(foreground.exe.as_deref(), Some(own_exe_name().as_str()));
    rt.run_tray_command(&TrayCommand::PinCurrentWindow);
    pump_ms(100);
    assert_eq!(rt.pinned(), vec![w[0]]);
    eventually_swept(&rt, "w[0] topmost", || is_topmost(w[0]));
    rt.run_tray_command(&TrayCommand::PinCurrentWindow);
    pump_ms(100);
    assert!(rt.pinned().is_empty());
    assert!(!is_topmost(w[0]));

    // A foreground window outside the allowed set never becomes the target: the command keeps
    // acting on the last window that was one.
    drop(fg);
    let _other = ForegroundOverride::set(w[1]);
    sweep(&rt);
    rt.run_tray_command(&TrayCommand::PinCurrentWindow);
    pump_ms(100);
    assert_eq!(rt.pinned(), vec![w[0]]);
    assert!(!is_topmost(w[1]));
    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn pin_current_window_refuses_excluded_apps_and_a_disabled_app() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(1);
    let w = wins.0.clone();
    let dir = temp_dir("pin-current-excluded");
    let settings = Settings {
        excluded_apps: vec![own_exe_name()],
        ..Settings::default()
    };
    let opts = options(&dir, w.clone(), &settings);
    let rt = Runtime::start(&opts).unwrap();
    let _fg = ForegroundOverride::set(w[0]);
    sweep(&rt);
    let (_, foreground) = rt.menu_inputs();
    assert!(!foreground.can_pin, "excluded app offered for pinning");
    rt.run_tray_command(&TrayCommand::PinCurrentWindow);
    assert!(rt.pinned().is_empty());

    // Removing the app from the exclusion list by hand edit makes it a target again.
    std::thread::sleep(Duration::from_millis(50));
    save_to(&opts.settings_path, &Settings::default()).unwrap();
    assert!(
        rt.menu_inputs().1.can_pin,
        "included again must allow pinning"
    );
    rt.run_tray_command(&TrayCommand::ToggleEnabled);
    rt.run_tray_command(&TrayCommand::PinCurrentWindow);
    assert!(
        rt.pinned().is_empty(),
        "pinned while Pin It is switched off"
    );
    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn missing_tray_icon_is_added_again_by_the_sweep() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(1);
    let dir = temp_dir("tray-retry");
    let mut opts = options(&dir, wins.0.clone(), &Settings::default());
    opts.tray = true;
    let rt = Runtime::start(&opts).unwrap();
    assert!(rt.tray_added(), "no tray icon at start");

    // The shell refuses a second add: the first failure is logged, the next ones are not.
    rt.add_tray_icon_again();
    rt.add_tray_icon_again();
    assert!(!rt.tray_added());
    let text = log_text(&dir);
    assert_eq!(text.matches("tray icon not added").count(), 1, "{text}");

    // Once the old icon is out of the way, the sweep gets the icon back and says so once.
    rt.remove_tray_icon();
    sweep(&rt);
    assert!(rt.tray_added(), "sweep did not retry");
    sweep(&rt);
    let text = log_text(&dir);
    assert_eq!(text.matches("tray icon added").count(), 1, "{text}");
    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}
