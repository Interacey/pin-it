use super::load::MAX_FILE_BYTES;
use super::*;
use crate::hotkey::Hotkey;
use std::path::{Path, PathBuf};

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pinit-cfg-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn round_trip() {
    let d = temp_dir("rt");
    let p = d.join("sub").join("settings.json");
    let mut s = Settings {
        enabled: false,
        fixed_accent: Some([1, 2, 3]),
        excluded_apps: vec!["a.exe".into()],
        ..Settings::default()
    };
    s.per_app_accent.insert("b.exe".into(), [9, 8, 7]);
    save_to(&p, &s).unwrap();
    assert_eq!(load_from(&p), s);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn missing_file_gives_defaults() {
    let d = temp_dir("missing");
    assert_eq!(load_from(&d.join("nope.json")), Settings::default());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn partial_json_fills_defaults() {
    let d = temp_dir("partial");
    let p = d.join("settings.json");
    std::fs::write(&p, r#"{"enabled": false, "animate": false}"#).unwrap();
    let s = load_from(&p);
    assert!(!s.enabled && !s.animate);
    assert!(s.use_window_color);
    assert_eq!(s.button_scale_percent, 100);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn unknown_fields_ignored() {
    let d = temp_dir("unknown");
    let p = d.join("settings.json");
    std::fs::write(&p, r#"{"future_thing": {"x": 1}}"#).unwrap();
    let s = load_from(&p);
    assert_eq!(s, Settings::default());
    assert!(!d.join("settings.json.bad").exists());
    let _ = std::fs::remove_dir_all(&d);
}

/// Settings files written before the border feature was removed still contain `"border"`.
#[test]
fn old_file_with_removed_border_field_loads_cleanly() {
    let d = temp_dir("old_border");
    let p = d.join("settings.json");
    std::fs::write(&p, r#"{"enabled": false, "border": true}"#).unwrap();
    let (s, st) = load_report_from(&p);
    assert!(!s.enabled);
    assert_eq!(st, LoadStatus::Ok);
    assert!(bad_files(&d).is_empty());
    let _ = std::fs::remove_dir_all(&d);
}
fn bad_files(d: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(d)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("settings.json.bad"))
        .collect();
    v.sort();
    v
}

#[test]
fn corrupt_file_is_backed_up_with_timestamp() {
    let d = temp_dir("corrupt");
    let p = d.join("settings.json");
    std::fs::write(&p, "{not json").unwrap();
    let (s, st) = load_report_from(&p);
    assert_eq!(s, Settings::default());
    let LoadStatus::Corrupt { backup: Some(b) } = st else {
        panic!("{st:?}");
    };
    assert!(!p.exists());
    assert_eq!(std::fs::read_to_string(&b).unwrap(), "{not json");
    assert!(
        b.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("settings.json.bad-")
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn only_three_newest_backups_kept() {
    let d = temp_dir("prune");
    let p = d.join("settings.json");
    for t in [100, 200, 300, 400] {
        std::fs::write(d.join(format!("settings.json.bad-{t}")), "x").unwrap();
    }
    std::fs::write(&p, "[1,2").unwrap();
    load_report_from(&p);
    let names = bad_files(&d);
    assert_eq!(names.len(), 3, "{names:?}");
    assert!(!names.contains(&"settings.json.bad-100".to_string()));
    assert!(!names.contains(&"settings.json.bad-200".to_string()));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn non_object_json_is_corrupt() {
    let d = temp_dir("array");
    let p = d.join("settings.json");
    std::fs::write(&p, "[1]").unwrap();
    assert!(matches!(load_report_from(&p).1, LoadStatus::Corrupt { .. }));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn oversized_file_is_corrupt() {
    let d = temp_dir("big");
    let p = d.join("settings.json");
    let mut text = String::from(r#"{"enabled": false, "pad": ""#);
    text.push_str(&"a".repeat(MAX_FILE_BYTES as usize));
    text.push_str("\"}");
    std::fs::write(&p, text).unwrap();
    let (s, st) = load_report_from(&p);
    assert!(s.enabled);
    assert!(matches!(st, LoadStatus::Corrupt { backup: Some(_) }));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn locked_file_is_io_error_and_untouched() {
    use std::os::windows::fs::OpenOptionsExt;
    let d = temp_dir("locked");
    let p = d.join("settings.json");
    std::fs::write(&p, r#"{"enabled": false}"#).unwrap();
    // share_mode(0) denies every other open, like an editor holding the file.
    let _guard = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&p)
        .unwrap();
    let (s, st) = load_report_from(&p);
    assert_eq!(s, Settings::default());
    assert!(matches!(st, LoadStatus::IoError(_)), "{st:?}");
    assert!(p.exists());
    assert!(bad_files(&d).is_empty());
    drop(_guard);
    assert!(!load_from(&p).enabled);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn bom_and_utf16_accepted() {
    let d = temp_dir("bom");
    let p = d.join("settings.json");
    let json = r#"{"enabled": false}"#;
    let mut b = vec![0xEF, 0xBB, 0xBF];
    b.extend_from_slice(json.as_bytes());
    std::fs::write(&p, &b).unwrap();
    assert_eq!(load_report_from(&p).1, LoadStatus::Ok);
    assert!(!load_from(&p).enabled);
    let mut le = vec![0xFF, 0xFE];
    le.extend(json.encode_utf16().flat_map(u16::to_le_bytes));
    std::fs::write(&p, &le).unwrap();
    assert!(!load_from(&p).enabled);
    let mut be = vec![0xFE, 0xFF];
    be.extend(json.encode_utf16().flat_map(u16::to_be_bytes));
    std::fs::write(&p, &be).unwrap();
    assert!(!load_from(&p).enabled);
    assert!(bad_files(&d).is_empty());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn one_bad_field_does_not_reset_the_rest() {
    let d = temp_dir("lenient");
    let p = d.join("settings.json");
    std::fs::write(
        &p,
        r#"{"enabled": false, "button_scale_percent": -1, "fixed_accent": [300, 0, 0],
            "animate": "yes", "hotkey": {"ctrl": true, "alt": false}}"#,
    )
    .unwrap();
    let (s, st) = load_report_from(&p);
    assert!(!s.enabled);
    assert_eq!(s.button_scale_percent, 100);
    assert_eq!(s.fixed_accent, None);
    assert!(s.animate);
    assert_eq!(s.hotkey, None);
    let LoadStatus::Lenient {
        mut fixed_fields,
        backup,
    } = st
    else {
        panic!("{st:?}");
    };
    fixed_fields.sort();
    assert_eq!(
        fixed_fields,
        ["animate", "button_scale_percent", "fixed_accent", "hotkey"]
    );
    // the original is copied before a save can overwrite it
    let backup = backup.expect("backup of the lenient file");
    assert!(p.exists() && backup.exists());
    assert_eq!(bad_files(&d).len(), 1);
    assert_eq!(
        std::fs::read_to_string(&backup).unwrap(),
        std::fs::read_to_string(&p).unwrap()
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn one_bad_list_element_keeps_the_others() {
    let d = temp_dir("lenient-lists");
    let p = d.join("settings.json");
    std::fs::write(
        &p,
        r#"{"excluded_apps": ["a.exe", 5, "b.exe", null],
            "per_app_accent": {"x.exe": [1, 2, 3], "y.exe": [300, 0, 0], "z.exe": "red"}}"#,
    )
    .unwrap();
    let (s, st) = load_report_from(&p);
    assert_eq!(s.excluded_apps, ["a.exe", "b.exe"]);
    assert_eq!(s.per_app_accent.len(), 1);
    assert_eq!(s.per_app_accent["x.exe"], [1, 2, 3]);
    let LoadStatus::Lenient {
        mut fixed_fields,
        backup,
    } = st
    else {
        panic!("{st:?}");
    };
    fixed_fields.sort();
    assert_eq!(
        fixed_fields,
        [
            "excluded_apps[1]",
            "excluded_apps[3]",
            "per_app_accent.y.exe",
            "per_app_accent.z.exe"
        ]
    );
    assert!(backup.is_some_and(|b| b.exists()));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn collection_of_the_wrong_type_is_reported_whole() {
    let d = temp_dir("lenient-type");
    let p = d.join("settings.json");
    std::fs::write(&p, r#"{"excluded_apps": "a.exe", "per_app_accent": [1]}"#).unwrap();
    let (s, st) = load_report_from(&p);
    assert!(s.excluded_apps.is_empty() && s.per_app_accent.is_empty());
    let LoadStatus::Lenient { fixed_fields, .. } = st else {
        panic!("{st:?}");
    };
    assert_eq!(fixed_fields, ["excluded_apps", "per_app_accent"]);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn clean_file_makes_no_backup_and_lenient_backups_are_pruned() {
    let d = temp_dir("lenient-prune");
    let p = d.join("settings.json");
    std::fs::write(&p, r#"{"excluded_apps": ["a.exe"]}"#).unwrap();
    assert_eq!(load_report_from(&p).1, LoadStatus::Ok);
    assert!(bad_files(&d).is_empty());
    std::fs::write(&p, r#"{"excluded_apps": [1]}"#).unwrap();
    for t in [100, 200, 300] {
        std::fs::write(d.join(format!("settings.json.bad-{t}")), "x").unwrap();
    }
    load_report_from(&p);
    let names = bad_files(&d);
    assert_eq!(names.len(), 3, "{names:?}");
    assert!(!names.contains(&"settings.json.bad-100".to_string()));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn old_shift_only_hotkey_from_a_settings_file_is_dropped() {
    let d = temp_dir("hk-shift");
    let p = d.join("settings.json");
    std::fs::write(
        &p,
        r#"{"hotkey": {"ctrl": false, "alt": false, "shift": true, "win": false, "vk": 80}}"#,
    )
    .unwrap();
    assert_eq!(load_from(&p).hotkey, None);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn guard_write_target_refuses_a_junction_directory() {
    let d = temp_dir("guard");
    let real = d.join("real");
    std::fs::create_dir_all(&real).unwrap();
    let link = d.join("link");
    assert!(make_junction(&link, &real), "mklink /J failed");
    assert!(guard_write_target(&real.join("pinit.log")).is_ok());
    let err = guard_write_target(&link.join("pinit.log")).unwrap_err();
    assert!(err.to_string().contains("reparse point"), "{err}");
    let _ = std::fs::remove_dir(&link);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn sanitize_resets_invalid_hotkey() {
    let mut s = Settings {
        hotkey: Some(Hotkey {
            ctrl: false,
            alt: false,
            shift: false,
            win: false,
            vk: 0x50,
        }),
        ..Settings::default()
    };
    s.sanitize();
    assert_eq!(s.hotkey, None);
    let d = temp_dir("hk");
    let p = d.join("settings.json");
    std::fs::write(
        &p,
        r#"{"hotkey": {"ctrl": false, "alt": false, "shift": false, "win": false, "vk": 80}}"#,
    )
    .unwrap();
    assert_eq!(load_from(&p).hotkey, None);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn hotkey_is_off_by_default_and_old_files_still_load() {
    assert_eq!(Settings::default().hotkey, None);
    let d = temp_dir("hk-old");
    let p = d.join("settings.json");
    std::fs::write(&p, r#"{"enabled": true}"#).unwrap();
    assert_eq!(load_from(&p).hotkey, None);
    std::fs::write(&p, r#"{"hotkey": null}"#).unwrap();
    assert_eq!(load_from(&p).hotkey, None);
    // written by an earlier build
    std::fs::write(
        &p,
        r#"{"hotkey": {"ctrl": true, "alt": true, "shift": false, "win": false, "vk": 80}}"#,
    )
    .unwrap();
    let old = Hotkey {
        ctrl: true,
        alt: true,
        shift: false,
        win: false,
        vk: 80,
    };
    assert_eq!(load_from(&p).hotkey, Some(old));
    // a wrong type is reported and falls back to none
    std::fs::write(&p, r#"{"hotkey": "ctrl+p"}"#).unwrap();
    let (s, st) = load_report_from(&p);
    assert_eq!(s.hotkey, None);
    assert!(matches!(st, LoadStatus::Lenient { .. }), "{st:?}");
    // a saved None round-trips
    save_to(&p, &Settings::default()).unwrap();
    assert_eq!(load_from(&p).hotkey, None);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn default_path_is_absolute() {
    let p = default_path();
    assert!(p.is_absolute(), "{p:?}");
    assert!(p.ends_with(r"PinIt\settings.json"));
}

#[test]
fn modified_time_changes_on_write() {
    let d = temp_dir("mtime");
    let p = d.join("settings.json");
    assert_eq!(modified_time(&p), None);
    std::fs::write(&p, "{}").unwrap();
    let t = modified_time(&p).unwrap();
    let f = std::fs::File::options().write(true).open(&p).unwrap();
    f.set_modified(t + std::time::Duration::from_secs(5))
        .unwrap();
    drop(f);
    assert!(modified_time(&p).unwrap() > t);
    let _ = std::fs::remove_dir_all(&d);
}

fn make_junction(link: &Path, target: &Path) -> bool {
    std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn save_refuses_junction_directory_and_file_symlink() {
    let d = temp_dir("junction");
    let real = d.join("real");
    std::fs::create_dir_all(&real).unwrap();
    let link = d.join("link");
    assert!(make_junction(&link, &real), "mklink /J failed");
    let err = save_to(&link.join("settings.json"), &Settings::default()).unwrap_err();
    assert!(err.to_string().contains("reparse point"), "{err}");
    assert!(!real.join("settings.json").exists());

    // a junction in place of the settings file itself
    let file_link = d.join("settings.json");
    assert!(make_junction(&file_link, &real));
    assert!(save_to(&file_link, &Settings::default()).is_err());
    let _ = std::fs::remove_dir(&link);
    let _ = std::fs::remove_dir(&file_link);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn clamping() {
    let mut s = Settings {
        button_scale_percent: 5,
        ..Settings::default()
    };
    s.sanitize();
    assert_eq!(s.button_scale_percent, 50);
    s.button_scale_percent = 900;
    s.sanitize();
    assert_eq!(s.button_scale_percent, 200);
    s.button_scale_percent = 125;
    s.sanitize();
    assert_eq!(s.button_scale_percent, 125);
}

#[test]
fn sanitize_lowercases_exe_names() {
    let mut s = Settings {
        excluded_apps: vec![" Foo.EXE ".into(), "foo.exe".into(), "".into()],
        ..Settings::default()
    };
    s.per_app_accent.insert("Bar.exe".into(), [1, 1, 1]);
    s.sanitize();
    assert_eq!(s.excluded_apps, vec!["foo.exe".to_string()]);
    assert!(s.per_app_accent.contains_key("bar.exe"));
}

#[test]
fn accent_priority() {
    let (w, sys, fixed, app) = ([1, 0, 0], [2, 0, 0], [3, 0, 0], [4, 0, 0]);
    let mut s = Settings::default();
    assert_eq!(s.accent_for(Some("x.exe"), w, sys), w);
    s.use_window_color = false;
    assert_eq!(s.accent_for(Some("x.exe"), w, sys), sys);
    s.fixed_accent = Some(fixed);
    assert_eq!(s.accent_for(None, w, sys), fixed);
    s.per_app_accent.insert("x.exe".into(), app);
    assert_eq!(s.accent_for(Some("X.exe"), w, sys), app);
    assert_eq!(s.accent_for(Some("y.exe"), w, sys), fixed);
    s.use_window_color = true;
    assert_eq!(s.accent_for(Some("x.exe"), w, sys), app);
}

#[test]
fn exclusion_case_insensitive() {
    let mut s = Settings::default();
    assert!(!s.is_excluded("a.exe"));
    s.excluded_apps.push("notepad.exe".into());
    assert!(s.is_excluded("Notepad.EXE"));
    assert!(!s.is_excluded("calc.exe"));
}

#[test]
fn atomic_save_leaves_no_temp_file() {
    let d = temp_dir("atomic");
    let p = d.join("settings.json");
    save_to(&p, &Settings::default()).unwrap();
    let s = Settings {
        enabled: false,
        ..Settings::default()
    };
    save_to(&p, &s).unwrap();
    let names: Vec<_> = std::fs::read_dir(&d)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["settings.json".to_string()]);
    assert!(!load_from(&p).enabled);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn dial_click_sound_defaults_to_on_and_a_bad_value_falls_back_leniently() {
    assert!(Settings::default().dial_click_sound);
    let d = temp_dir("click-sound");
    let p = d.join("settings.json");
    std::fs::write(&p, r#"{"dial_click_sound": false}"#).unwrap();
    assert!(!load_from(&p).dial_click_sound);
    std::fs::write(&p, r#"{"dial_click_sound": "loud", "enabled": false}"#).unwrap();
    let (s, st) = load_report_from(&p);
    assert!(s.dial_click_sound && !s.enabled);
    let LoadStatus::Lenient { fixed_fields, .. } = st else {
        panic!("{st:?}");
    };
    assert_eq!(fixed_fields, ["dial_click_sound"]);
    let _ = std::fs::remove_dir_all(&d);
}
