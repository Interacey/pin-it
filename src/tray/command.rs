use crate::config::{Settings, clamp_button_scale};
use crate::exclusions::normalize_entry;
use crate::hotkey::Hotkey;

/// A user action chosen in the tray menu.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TrayCommand {
    /// Switch the pin buttons on or off.
    ToggleEnabled,
    /// Switch between window color and fixed color.
    ToggleWindowColor,
    /// Switch animations on or off.
    ToggleAnimation,
    /// Switch start with Windows on or off.
    ToggleAutostart,
    /// Use this global hotkey.
    SetHotkey(Hotkey),
    /// Register no global hotkey.
    ClearHotkey,
    /// Use this fixed accent color.
    SetFixedColor([u8; 3]),
    /// Forget the fixed accent color.
    ClearFixedColor,
    /// Set the button size in percent.
    SetButtonScale(u32),
    /// Open the button size dial.
    OpenSizeDial,
    /// Open the excluded apps window.
    OpenExclusions,
    /// Add a program to the exclusion list. The name is checked and normalized again here.
    AddExcludedApp(String),
    /// Remove a program from the exclusion list.
    RemoveExcludedApp(String),
    /// Pin or unpin the last app window that was in front, for windows without a visible button.
    PinCurrentWindow,
    /// Unpin every pinned window.
    UnpinAll,
    /// Open the settings file in the default editor.
    OpenSettingsFile,
    /// Quit the program.
    Exit,
}

/// What the caller has to do after `apply`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Effect {
    /// Nothing changed.
    None,
    /// Settings changed: save them, sync autostart, re-register the hotkey.
    SettingsChanged,
    /// Pin or unpin the last foreground window.
    PinCurrentWindow,
    /// Unpin all windows.
    UnpinAll,
    /// Open the settings file.
    OpenSettingsFile,
    /// Open the button size dial.
    OpenSizeDial,
    /// Open the excluded apps window.
    OpenExclusions,
    /// Quit.
    Exit,
}

/// Applies a command to the settings struct only; the caller does the registry, file and hotkey work.
pub(crate) fn apply(cmd: &TrayCommand, settings: &mut Settings) -> Effect {
    match cmd {
        TrayCommand::ToggleEnabled => settings.enabled = !settings.enabled,
        TrayCommand::ToggleWindowColor => settings.use_window_color = !settings.use_window_color,
        TrayCommand::ToggleAnimation => settings.animate = !settings.animate,
        TrayCommand::ToggleAutostart => settings.autostart = !settings.autostart,
        TrayCommand::SetHotkey(hotkey) => settings.hotkey = Some(*hotkey),
        TrayCommand::ClearHotkey => settings.hotkey = None,
        TrayCommand::SetFixedColor(color) => use_fixed_color(settings, *color),
        TrayCommand::ClearFixedColor => settings.fixed_accent = None,
        TrayCommand::SetButtonScale(percent) => {
            settings.button_scale_percent = clamp_button_scale(*percent);
        }
        TrayCommand::AddExcludedApp(name) => return add_exclusion(settings, name),
        TrayCommand::RemoveExcludedApp(name) => return remove_exclusion(settings, name),
        TrayCommand::PinCurrentWindow => return Effect::PinCurrentWindow,
        TrayCommand::UnpinAll => return Effect::UnpinAll,
        TrayCommand::OpenSettingsFile => return Effect::OpenSettingsFile,
        TrayCommand::OpenSizeDial => return Effect::OpenSizeDial,
        TrayCommand::OpenExclusions => return Effect::OpenExclusions,
        TrayCommand::Exit => return Effect::Exit,
    }
    Effect::SettingsChanged
}

fn use_fixed_color(settings: &mut Settings, color: [u8; 3]) {
    settings.fixed_accent = Some(color);
    settings.use_window_color = false;
}

/// Adds the normalized name; an invalid or known name changes nothing. The list stays sorted.
fn add_exclusion(settings: &mut Settings, name: &str) -> Effect {
    let Ok(name) = normalize_entry(name) else {
        return Effect::None;
    };
    if settings.is_excluded(&name) {
        return Effect::None;
    }
    settings.excluded_apps.push(name);
    settings.excluded_apps.sort();
    Effect::SettingsChanged
}

fn remove_exclusion(settings: &mut Settings, name: &str) -> Effect {
    let before = settings.excluded_apps.len();
    settings
        .excluded_apps
        .retain(|e| !e.eq_ignore_ascii_case(name.trim()));
    if settings.excluded_apps.len() == before {
        Effect::None
    } else {
        Effect::SettingsChanged
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Getter = fn(&Settings) -> bool;

    #[test]
    fn apply_toggles() {
        let mut s = Settings::default();
        let d = Settings::default();
        let cases: [(TrayCommand, Getter); 4] = [
            (TrayCommand::ToggleEnabled, |s| s.enabled),
            (TrayCommand::ToggleWindowColor, |s| s.use_window_color),
            (TrayCommand::ToggleAnimation, |s| s.animate),
            (TrayCommand::ToggleAutostart, |s| s.autostart),
        ];
        for (cmd, get) in cases {
            assert_eq!(apply(&cmd, &mut s), Effect::SettingsChanged);
            assert_ne!(get(&s), get(&d), "{cmd:?} did not flip");
            assert_eq!(apply(&cmd, &mut s), Effect::SettingsChanged);
            assert_eq!(get(&s), get(&d), "{cmd:?} did not flip back");
        }
    }

    #[test]
    fn apply_values() {
        let mut s = Settings::default();
        let h = Hotkey {
            ctrl: false,
            alt: false,
            shift: true,
            win: true,
            vk: 0x50,
        };
        assert_eq!(
            apply(&TrayCommand::SetHotkey(h), &mut s),
            Effect::SettingsChanged
        );
        assert_eq!(s.hotkey, Some(h));
        assert_eq!(
            apply(&TrayCommand::ClearHotkey, &mut s),
            Effect::SettingsChanged
        );
        assert_eq!(s.hotkey, None);

        assert_eq!(
            apply(&TrayCommand::SetFixedColor([1, 2, 3]), &mut s),
            Effect::SettingsChanged
        );
        assert_eq!(s.fixed_accent, Some([1, 2, 3]));
        assert!(!s.use_window_color);
        assert_eq!(
            apply(&TrayCommand::ClearFixedColor, &mut s),
            Effect::SettingsChanged
        );
        assert_eq!(s.fixed_accent, None);

        apply(&TrayCommand::SetButtonScale(125), &mut s);
        assert_eq!(s.button_scale_percent, 125);
        apply(&TrayCommand::SetButtonScale(1000), &mut s);
        assert_eq!(s.button_scale_percent, 200);
    }

    fn excluded(s: &Settings) -> Vec<&str> {
        s.excluded_apps.iter().map(String::as_str).collect()
    }

    #[test]
    fn adding_an_excluded_app_normalizes_dedupes_and_keeps_the_list_sorted() {
        let mut s = Settings::default();
        for (name, effect) in [
            ("  Notepad.EXE ", Effect::SettingsChanged),
            ("calc", Effect::SettingsChanged),
            ("NOTEPAD.exe", Effect::None),
            ("calc.exe", Effect::None),
            ("a b.exe", Effect::SettingsChanged),
        ] {
            let cmd = TrayCommand::AddExcludedApp(name.into());
            assert_eq!(apply(&cmd, &mut s), effect, "{name:?}");
        }
        assert_eq!(excluded(&s), ["a b.exe", "calc.exe", "notepad.exe"]);
    }

    #[test]
    fn an_invalid_name_is_not_added() {
        let mut s = Settings::default();
        for name in ["", "  ", "a/b.exe", "x.", "con:", &"a".repeat(300)] {
            let cmd = TrayCommand::AddExcludedApp(name.into());
            assert_eq!(apply(&cmd, &mut s), Effect::None, "{name:?}");
        }
        assert!(s.excluded_apps.is_empty());
    }

    #[test]
    fn removing_an_excluded_app_ignores_case_and_reports_no_change_for_an_unknown_one() {
        let mut s = Settings {
            excluded_apps: vec!["a.exe".into(), "b.exe".into()],
            ..Settings::default()
        };
        let remove = |name: &str| TrayCommand::RemoveExcludedApp(name.into());
        assert_eq!(apply(&remove("c.exe"), &mut s), Effect::None);
        assert_eq!(apply(&remove("A.EXE"), &mut s), Effect::SettingsChanged);
        assert_eq!(excluded(&s), ["b.exe"]);
    }

    #[test]
    fn apply_effects() {
        let mut s = Settings::default();
        assert_eq!(apply(&TrayCommand::UnpinAll, &mut s), Effect::UnpinAll);
        assert_eq!(
            apply(&TrayCommand::PinCurrentWindow, &mut s),
            Effect::PinCurrentWindow
        );
        assert_eq!(
            apply(&TrayCommand::OpenSettingsFile, &mut s),
            Effect::OpenSettingsFile
        );
        assert_eq!(
            apply(&TrayCommand::OpenSizeDial, &mut s),
            Effect::OpenSizeDial
        );
        assert_eq!(
            apply(&TrayCommand::OpenExclusions, &mut s),
            Effect::OpenExclusions
        );
        assert_eq!(apply(&TrayCommand::Exit, &mut s), Effect::Exit);
        assert_eq!(s, Settings::default());
    }
}
