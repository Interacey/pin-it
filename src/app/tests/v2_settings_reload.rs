//! A settings file that is edited by hand into invalid JSON stays where it is while the app runs.

use std::path::Path;
use std::time::Duration;

use super::support::{Wins, options, temp_dir};
use crate::app::Runtime;
use crate::config::{Settings, load_from};
use crate::tray::TrayCommand;

const BROKEN: &str = "{ \"animate\": tru";

fn backups(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("settings.json.bad-"))
        .collect();
    names.sort();
    names
}

#[test]
fn a_hand_edit_that_does_not_parse_is_kept_in_place_and_not_overwritten() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(1);
    let dir = temp_dir("v2-invalid-edit");
    let opts = options(&dir, wins.0.clone(), &Settings::default());
    let path = opts.settings_path.clone();
    let rt = Runtime::start(&opts).unwrap();
    std::thread::sleep(Duration::from_millis(50));

    std::fs::write(&path, BROKEN).unwrap();
    let (menu_settings, _) = rt.menu_inputs();
    assert!(menu_settings.animate, "memory settings changed");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        BROKEN,
        "file was moved or rewritten"
    );
    let copies = backups(&dir);
    assert_eq!(copies.len(), 1, "expected one copy: {copies:?}");
    assert_eq!(
        std::fs::read_to_string(dir.join(&copies[0])).unwrap(),
        BROKEN
    );

    // A command while the file is invalid changes memory but must not write the file.
    rt.run_tray_command(&TrayCommand::ToggleAnimation);
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        BROKEN,
        "file was overwritten"
    );
    assert!(!rt.settings().animate);
    assert_eq!(backups(&dir).len(), 1, "an unchanged file was copied again");

    // The user fixes the file; the next menu picks it up.
    std::thread::sleep(Duration::from_millis(50));
    let fixed = Settings {
        button_scale_percent: 150,
        ..Settings::default()
    };
    crate::config::save_to(&path, &fixed).unwrap();
    let (menu_settings, _) = rt.menu_inputs();
    assert_eq!(menu_settings.button_scale_percent, 150);
    assert_eq!(load_from(&path).button_scale_percent, 150);
    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}
