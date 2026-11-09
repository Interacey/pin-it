//! The tray menu as plain data, independent of any Win32 handle.

use super::command::TrayCommand;
use crate::config::Settings;
use crate::hotkey::Hotkey;

const DEFAULT_PICKER_COLOR: [u8; 3] = [0, 120, 215];
const VK_P: u32 = 0x50;
/// The title line of the tray menu.
const MENU_HEADING: &str = "Pin It";

/// The app window the tray acts on: the last real app window that was in front before the menu.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Foreground {
    /// Lowercase exe name, `None` when no such window is known.
    pub(crate) exe: Option<String>,
    /// The window may be pinned or unpinned from the menu (same rules as the hotkey).
    pub(crate) can_pin: bool,
}

#[cfg(test)]
impl Foreground {
    /// A pinnable foreground app, for tests.
    pub(crate) fn app(exe: &str) -> Self {
        Foreground {
            exe: Some(exe.to_string()),
            can_pin: true,
        }
    }
}

/// One entry of the tray menu as plain data.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MenuItem {
    /// Text shown in the menu.
    pub(crate) label: String,
    /// Draw a check mark.
    pub(crate) checked: bool,
    /// False greys the entry out.
    pub(crate) enabled: bool,
    /// Command returned when chosen; `None` for separators and submenu parents.
    pub(crate) command: Option<TrayCommand>,
    /// Child entries; non-empty makes this a submenu.
    pub(crate) submenu: Vec<MenuItem>,
    /// True for a separator line.
    pub(crate) separator: bool,
    /// True for the title line at the top: readable text that cannot be chosen and has no command.
    pub(crate) heading: bool,
}

impl MenuItem {
    fn plain(label: String) -> Self {
        MenuItem {
            label,
            checked: false,
            enabled: true,
            command: None,
            submenu: Vec::new(),
            separator: false,
            heading: false,
        }
    }

    fn leaf(label: impl Into<String>, checked: bool, command: TrayCommand) -> Self {
        MenuItem {
            checked,
            command: Some(command),
            ..MenuItem::plain(label.into())
        }
    }

    fn sub(label: &str, submenu: Vec<MenuItem>) -> Self {
        MenuItem {
            submenu,
            ..MenuItem::plain(label.into())
        }
    }

    fn heading(label: &str) -> Self {
        MenuItem {
            heading: true,
            ..MenuItem::plain(label.into())
        }
    }

    fn separator() -> Self {
        MenuItem {
            separator: true,
            ..MenuItem::plain(String::new())
        }
    }
}

/// The hotkey choices offered in the menu.
fn hotkey_presets() -> [Hotkey; 4] {
    let preset = |ctrl, alt, shift, win| Hotkey {
        ctrl,
        alt,
        shift,
        win,
        vk: VK_P,
    };
    [
        preset(true, true, false, false),
        preset(true, false, true, false),
        preset(false, false, true, true),
        preset(true, true, true, false),
    ]
}

/// Builds the menu structure from the current settings.
///
/// The "Fixed accent color..." entry carries `SetFixedColor(<current or default color>)`; `Tray::on_message`
/// treats that command as "open the color picker" and replaces the color with the picked one.
pub(crate) fn menu_model(settings: &Settings, foreground: &Foreground) -> Vec<MenuItem> {
    vec![
        MenuItem::heading(MENU_HEADING),
        MenuItem::separator(),
        MenuItem::leaf("Enabled", settings.enabled, TrayCommand::ToggleEnabled),
        pin_item(foreground),
        colors_submenu(settings),
        MenuItem::leaf("Animation", settings.animate, TrayCommand::ToggleAnimation),
        MenuItem::leaf("Button size...", false, TrayCommand::OpenSizeDial),
        MenuItem::sub("Hotkey", hotkey_items(settings)),
        MenuItem::leaf("Excluded apps...", false, TrayCommand::OpenExclusions),
        MenuItem::separator(),
        MenuItem::leaf(
            "Start with Windows",
            settings.autostart,
            TrayCommand::ToggleAutostart,
        ),
        MenuItem::leaf("Unpin all", false, TrayCommand::UnpinAll),
        MenuItem::leaf("Open settings file", false, TrayCommand::OpenSettingsFile),
        MenuItem::separator(),
        MenuItem::leaf("Exit", false, TrayCommand::Exit),
    ]
}

fn colors_submenu(settings: &Settings) -> MenuItem {
    let picker_color = settings.fixed_accent.unwrap_or(DEFAULT_PICKER_COLOR);
    let fixed = MenuItem::leaf(
        "Fixed accent color...",
        !settings.use_window_color && settings.fixed_accent.is_some(),
        TrayCommand::SetFixedColor(picker_color),
    );
    let mut reset = MenuItem::leaf("Reset fixed color", false, TrayCommand::ClearFixedColor);
    reset.enabled = settings.fixed_accent.is_some();
    MenuItem::sub(
        "Colors",
        vec![
            MenuItem::leaf(
                "Use window color",
                settings.use_window_color,
                TrayCommand::ToggleWindowColor,
            ),
            fixed,
            reset,
        ],
    )
}

fn hotkey_items(settings: &Settings) -> Vec<MenuItem> {
    let none = MenuItem::leaf(
        "None (off)",
        settings.hotkey.is_none(),
        TrayCommand::ClearHotkey,
    );
    let presets = hotkey_presets().into_iter().map(|hotkey| {
        MenuItem::leaf(
            hotkey.label(),
            settings.hotkey == Some(hotkey),
            TrayCommand::SetHotkey(hotkey),
        )
    });
    std::iter::once(none).chain(presets).collect()
}

fn pin_item(foreground: &Foreground) -> MenuItem {
    let label = match &foreground.exe {
        Some(exe) => format!("Pin / unpin {exe}"),
        None => "Pin / unpin current window".to_string(),
    };
    let mut item = MenuItem::leaf(label, false, TrayCommand::PinCurrentWindow);
    item.enabled = foreground.can_pin;
    item
}

/// Commands in menu-id order: entry `i` of the result has id `i + 1` (0 means "nothing chosen").
pub(crate) fn command_ids(model: &[MenuItem]) -> Vec<TrayCommand> {
    let mut out = Vec::new();
    for item in model {
        if let Some(command) = &item.command {
            out.push(command.clone());
        }
        out.extend(command_ids(&item.submenu));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find<'a>(items: &'a [MenuItem], label: &str) -> &'a MenuItem {
        for i in items {
            if i.label == label {
                return i;
            }
            if let Some(x) = i.submenu.iter().find(|c| c.label == label) {
                return x;
            }
        }
        panic!("no item {label}");
    }

    #[test]
    fn model_checkmarks_follow_settings() {
        let mut s = Settings::default();
        let m = menu_model(&s, &Foreground::app("code.exe"));
        assert!(find(&m, "Enabled").checked);
        assert!(find(&m, "Use window color").checked);
        assert!(find(&m, "None (off)").checked);
        assert!(!find(&m, "Ctrl+Alt+P").checked);
        assert!(!find(&m, "Ctrl+Shift+P").checked);
        assert!(!find(&m, "Reset fixed color").enabled);

        s.enabled = false;
        s.button_scale_percent = 150;
        s.hotkey = Some(hotkey_presets()[2]);
        s.fixed_accent = Some([5, 6, 7]);
        s.use_window_color = false;
        let m = menu_model(&s, &Foreground::app("Code.exe"));
        assert!(!find(&m, "Enabled").checked);

        assert!(find(&m, &hotkey_presets()[2].label()).checked);
        assert!(!find(&m, "None (off)").checked);
        assert!(find(&m, "Reset fixed color").enabled);
        let fixed = find(&m, "Fixed accent color...");
        assert!(fixed.checked);
        assert_eq!(fixed.command, Some(TrayCommand::SetFixedColor([5, 6, 7])));
    }

    #[test]
    fn button_size_is_one_item_that_opens_the_dial() {
        let m = menu_model(&Settings::default(), &Foreground::default());
        let item = find(&m, "Button size...");
        assert_eq!(item.command, Some(TrayCommand::OpenSizeDial));
        assert!(item.submenu.is_empty() && !item.checked && item.enabled);
        assert!(
            !command_ids(&m)
                .iter()
                .any(|c| matches!(c, TrayCommand::SetButtonScale(_)))
        );
    }

    #[test]
    fn excluded_apps_is_one_item_that_opens_the_window_and_the_old_toggle_is_gone() {
        for foreground in [Foreground::default(), Foreground::app("code.exe")] {
            let m = menu_model(&Settings::default(), &foreground);
            let item = find(&m, "Excluded apps...");
            assert_eq!(item.command, Some(TrayCommand::OpenExclusions));
            assert!(item.submenu.is_empty() && !item.checked && item.enabled);
            let labels: Vec<_> = m.iter().map(|i| i.label.as_str()).collect();
            assert!(
                !labels
                    .iter()
                    .any(|l| l.starts_with("Exclude ") || l.starts_with("Include "))
            );
        }
    }

    #[test]
    fn pin_item_follows_the_foreground_window() {
        let none = menu_model(&Settings::default(), &Foreground::default());
        let item = find(&none, "Pin / unpin current window");
        assert!(!item.enabled);
        assert_eq!(item.command, Some(TrayCommand::PinCurrentWindow));

        let known = menu_model(&Settings::default(), &Foreground::app("code.exe"));
        let item = find(&known, "Pin / unpin code.exe");
        assert!(item.enabled && !item.checked);

        let refused = Foreground {
            exe: Some("elevated.exe".into()),
            can_pin: false,
        };
        let m = menu_model(&Settings::default(), &refused);
        assert!(!find(&m, "Pin / unpin elevated.exe").enabled);
    }

    #[test]
    fn model_top_level_order() {
        let m = menu_model(&Settings::default(), &Foreground::default());
        let labels: Vec<_> = m.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "Pin It",
                "",
                "Enabled",
                "Pin / unpin current window",
                "Colors",
                "Animation",
                "Button size...",
                "Hotkey",
                "Excluded apps...",
                "",
                "Start with Windows",
                "Unpin all",
                "Open settings file",
                "",
                "Exit"
            ]
        );
        assert_eq!(m.iter().filter(|i| i.separator).count(), 3);
    }

    #[test]
    fn the_heading_comes_first_and_never_yields_a_command() {
        let m = menu_model(&Settings::default(), &Foreground::default());
        assert!(m[0].heading && m[0].label == "Pin It");
        assert!(m[0].command.is_none() && m[0].submenu.is_empty() && !m[0].separator);
        assert!(m[1].separator, "separator under the heading");
        assert_eq!(m.iter().filter(|i| i.heading).count(), 1);
        assert_eq!(command_ids(&m[..1]), []);
    }

    #[test]
    fn every_hotkey_preset_has_a_distinct_label() {
        let mut labels: Vec<_> = hotkey_presets().iter().map(|h| h.label()).collect();
        labels.sort();
        labels.dedup();
        assert_eq!(labels.len(), 4);
    }

    #[test]
    fn command_ids_map_one_to_one() {
        let m = menu_model(&Settings::default(), &Foreground::app("a.exe"));
        let ids = command_ids(&m);
        let n = {
            fn count(m: &[MenuItem]) -> usize {
                m.iter()
                    .map(|i| usize::from(i.command.is_some()) + count(&i.submenu))
                    .sum()
            }
            count(&m)
        };
        assert_eq!(ids.len(), n);
        assert_eq!(
            ids.first(),
            Some(&TrayCommand::ToggleEnabled),
            "heading has no id"
        );
        assert_eq!(ids.last(), Some(&TrayCommand::Exit));
        assert_eq!(ids[1], TrayCommand::PinCurrentWindow);
        // third entry is the first item of the Colors submenu
        assert_eq!(ids[2], TrayCommand::ToggleWindowColor);
    }
}
