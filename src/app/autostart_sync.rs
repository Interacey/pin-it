//! The Run-key autostart entry follows the `autostart` setting.

use super::logging::warn;
use super::state::App;

/// `Some(on)` when the Run entry has to be written or removed to match the setting.
fn autostart_change(want: bool, registered: bool) -> Option<bool> {
    (want != registered).then_some(want)
}

impl App {
    /// Writes or removes the Run entry only when the setting differs from what is registered.
    /// A value that points to another exe is never removed.
    pub(super) fn sync_autostart(&mut self) {
        if !self.autostart.enabled {
            return;
        }
        let Some(on) = autostart_change(self.settings.autostart, self.autostart.registered) else {
            return;
        };
        match crate::autostart::set_enabled(on) {
            Ok(()) => self.autostart.registered = on,
            Err(e) => warn(&format!("cannot change autostart: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autostart_writes_only_on_a_difference() {
        assert_eq!(autostart_change(true, false), Some(true));
        assert_eq!(autostart_change(false, true), Some(false));
        assert_eq!(autostart_change(true, true), None);
        assert_eq!(autostart_change(false, false), None);
    }
}
