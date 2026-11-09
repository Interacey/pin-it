//! The global hotkey: making the one from the settings the registered one.

use super::host::HOTKEY_ID;
use super::logging::{info, warn};
use super::state::App;
use crate::hotkey::{self, Hotkey};

impl App {
    /// Makes the hotkey from the settings the active one, or registers nothing when there is none.
    /// When registering fails, the settings go back to what is actually registered (nothing, or
    /// the previous hotkey), so the file and the menu never claim a hotkey that does not work.
    pub(super) fn sync_hotkey(&mut self) {
        if !self.hotkey.wanted {
            return;
        }
        match self.settings.hotkey.filter(Hotkey::is_valid) {
            None => {
                if self.unregister_hotkey() {
                    info("hotkey: none");
                }
            }
            Some(want) if self.hotkey.active == Some(want) => {}
            Some(want) => self.switch_hotkey(want),
        }
    }

    /// Releases the active registration; `false` when there was none.
    pub(super) fn unregister_hotkey(&mut self) -> bool {
        let was_active = self.hotkey.active.take().is_some();
        if was_active {
            hotkey::unregister(self.hwnd, HOTKEY_ID);
        }
        was_active
    }

    fn switch_hotkey(&mut self, want: Hotkey) {
        let previous = self.hotkey.active;
        self.unregister_hotkey();
        match hotkey::register(self.hwnd, HOTKEY_ID, &want) {
            Ok(()) => {
                self.hotkey.active = Some(want);
                info(&format!("hotkey {}", want.label()));
            }
            Err(e) => {
                warn(&format!("hotkey {} not available: {e}", want.label()));
                self.restore_hotkey(previous);
            }
        }
    }

    /// Registers `previous` again and makes the settings say what is registered afterwards.
    fn restore_hotkey(&mut self, previous: Option<Hotkey>) {
        self.hotkey.active = previous.filter(|old| {
            hotkey::register(self.hwnd, HOTKEY_ID, old)
                .inspect_err(|e| warn(&format!("hotkey {} lost: {e}", old.label())))
                .is_ok()
        });
        self.settings.hotkey = self.hotkey.active;
    }
}
