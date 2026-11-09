//! Settings service: loading, saving and applying settings. Applying announces a
//! `SettingsEvent`; the observers do the rest (hotkey, autostart, buttons).
//!
//! Related parts live next to it: `command_runner` (tray commands), `hotkey_binding`,
//! `autostart_sync` and `open_settings`.

use super::logging::{info, warn};
use super::settings_observers::SettingsEvent;
use super::state::App;
use crate::config::{self, LoadStatus};

pub(super) fn log_load_status(status: &LoadStatus) {
    match status {
        LoadStatus::Missing | LoadStatus::Ok => {}
        LoadStatus::IoError(e) => warn(&format!("settings file unreadable: {e}")),
        LoadStatus::Corrupt { backup } => warn(&format!(
            "settings file is corrupt, defaults used (backup: {})",
            backup_text(backup.as_deref())
        )),
        LoadStatus::Lenient {
            fixed_fields,
            backup,
        } => warn(&format!(
            "settings file had invalid values, dropped: {} (copy: {})",
            fixed_fields.join(", "),
            backup_text(backup.as_deref())
        )),
    }
}

fn backup_text(backup: Option<&std::path::Path>) -> String {
    backup.map_or_else(|| "none".to_string(), |b| b.display().to_string())
}

impl App {
    pub(super) fn save_settings(&mut self) {
        let file = &mut self.settings_file;
        match config::save_to(&file.path, &self.settings) {
            Ok(()) => file.mtime = config::modified_time(&file.path),
            Err(e) => warn(&format!("cannot save settings: {e}")),
        }
    }

    /// Adopts the settings file when it was edited since the last load or save, so a tray command
    /// does not overwrite hand edits. Returns whether the settings were replaced.
    pub(super) fn reload_if_changed(&mut self) -> bool {
        let now = config::modified_time(&self.settings_file.path);
        if now == self.settings_file.mtime {
            return false;
        }
        let (settings, status) = config::load_report_from(&self.settings_file.path);
        log_load_status(&status);
        match status {
            LoadStatus::Ok | LoadStatus::Lenient { .. } => {
                info("settings file changed on disk, reloaded");
                self.settings = settings;
                self.settings.sanitize();
                self.settings_file.mtime = now;
                true
            }
            // Deleted, unreadable or corrupt: keep what is in memory; the next save rewrites it.
            _ => {
                self.settings_file.mtime = config::modified_time(&self.settings_file.path);
                false
            }
        }
    }

    /// Sanitizes the settings and tells the observers they changed.
    pub(super) fn apply_settings(&mut self) {
        self.settings.sanitize();
        let events = self.settings_events.clone();
        events.notify(&SettingsEvent::Changed, self);
    }
}
