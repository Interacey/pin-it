//! Keeping the tray icon in the notification area: a failed add is retried, not forgotten.

use super::logging::{info, warn};
use super::state::App;

impl App {
    /// Adds the icon and logs the first failure and the later success once each.
    pub(super) fn add_tray_icon(&mut self) {
        let Some(tray) = self.tray.clone() else {
            return;
        };
        match tray.add_icon() {
            Ok(()) if self.tray_add_failing => {
                self.tray_add_failing = false;
                info("tray icon added");
            }
            Ok(()) => {}
            Err(e) if !self.tray_add_failing => {
                self.tray_add_failing = true;
                warn(&format!("tray icon not added, retrying every sweep: {e}"));
            }
            Err(_) => {}
        }
    }

    /// The shell forgot its icons (explorer restarted): add ours again.
    pub(super) fn on_taskbar_created(&mut self) {
        self.add_tray_icon();
    }

    /// Part of the sweep: tries again while the icon is missing.
    pub(super) fn retry_tray_icon(&mut self) {
        if self.tray.as_ref().is_some_and(|t| !t.is_added()) {
            self.add_tray_icon();
        }
    }
}
