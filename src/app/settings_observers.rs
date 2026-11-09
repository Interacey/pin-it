//! `SettingsEvent` and its observers: what has to follow when the settings were changed.

use super::logging::info;
use super::observer::Subject;
use super::state::App;
use crate::Hwnd;
use crate::windows_filter::exe_name;

/// The settings were changed (tray command or edited file) and sanitized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SettingsEvent {
    Changed,
}

/// Subscribes the settings observers in the order they must run (`Subject` calls them in the
/// order they were subscribed):
/// 1. the hotkey first: a registration that fails reverts `settings.hotkey` to what is registered,
/// 2. the file, so it holds that corrected value,
/// 3. autostart, unpin-when-disabled, excluded windows and button styles, which read the final
///    values,
/// 4. the sweep last, so it sees the buttons and pins as they are after steps 1 to 3.
pub(super) fn subscribe_all(events: &Subject<SettingsEvent, App>) {
    events.subscribe_fn(|_, app| app.sync_hotkey());
    events.subscribe_fn(|_, app| app.save_settings());
    events.subscribe_fn(|_, app| app.sync_autostart());
    events.subscribe_fn(|_, app| app.unpin_all_when_disabled());
    events.subscribe_fn(|_, app| app.remove_excluded_windows());
    events.subscribe_fn(|_, app| app.restyle_buttons());
    events.subscribe_fn(|_, app| app.sweep());
}

impl App {
    pub(super) fn unpin_all_when_disabled(&mut self) {
        if !self.settings.enabled {
            self.unpin_all();
        }
    }

    /// Windows of apps that are excluded now lose their button (and their pin). The exe name is
    /// read again from the window: the one stored when the button was made can be stale (a UWP
    /// frame whose app window did not exist yet reports the frame host) or missing.
    pub(super) fn remove_excluded_windows(&mut self) {
        let excluded: Vec<Hwnd> = self
            .entries
            .iter()
            .filter(|(h, entry)| self.is_excluded_window(**h, entry.exe.as_deref()))
            .map(|(h, _)| *h)
            .collect();
        for h in excluded {
            info(&format!("excluded app: button of {h:#x} removed"));
            self.remove_window(h);
        }
    }

    fn is_excluded_window(&self, h: Hwnd, stored_exe: Option<&str>) -> bool {
        let current = exe_name(h);
        [current.as_deref(), stored_exe]
            .into_iter()
            .flatten()
            .any(|exe| self.settings.is_excluded(exe))
    }

    /// Re-renders every button; the scale may have changed, so the slots are recomputed too.
    pub(super) fn restyle_buttons(&mut self) {
        for h in self.entry_handles() {
            if let Some(entry) = self.entries.get_mut(&h) {
                entry.slot = None;
            }
            self.refresh_style(h);
        }
    }
}
