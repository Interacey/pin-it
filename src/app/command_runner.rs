//! Tray commands: what choosing a menu entry does to the app, and what the menu shows.

use super::host::{WM_DIAL_CHANGED, WM_EXCLUSIONS_CHANGED, post_quit};
use super::logging::{info, warn};
use super::state::App;
use crate::Hwnd;
use crate::config::Settings;
use crate::dial;
use crate::exclusions::{self, Action};
use crate::tray::{self, Effect, Foreground};

impl App {
    /// Settings and foreground window for the next menu. A hand edit of the settings file is
    /// adopted first, so a relative command ("toggle") acts on what the file says now.
    pub(super) fn menu_inputs(&mut self) -> (Settings, Foreground) {
        if self.reload_if_changed() {
            self.apply_settings();
        }
        let foreground = Foreground {
            exe: self.last_foreground_exe(),
            can_pin: self.tray_pin_target().is_some(),
        };
        (self.settings.clone(), foreground)
    }

    /// A command chosen in the tray menu. The log shows that it arrived and how it ended, so a
    /// click that had no visible effect can be told from one that never came.
    pub(super) fn run_tray_command(&mut self, cmd: &tray::TrayCommand) {
        info(&format!("tray command: {cmd:?}"));
        match self.execute(cmd) {
            Ok(()) => info("tray command done"),
            Err(e) => warn(&format!("tray command failed: {e}")),
        }
    }

    pub(super) fn run_command(&mut self, cmd: &tray::TrayCommand) {
        if let Err(e) = self.execute(cmd) {
            warn(&e);
        }
    }

    fn execute(&mut self, cmd: &tray::TrayCommand) -> Result<(), String> {
        let reloaded = self.reload_if_changed();
        let effect = tray::apply(cmd, &mut self.settings);
        if reloaded && !matches!(effect, Effect::SettingsChanged | Effect::Exit) {
            self.apply_settings();
        }
        let result = self.run_effect(effect);
        // Whatever changed the exclusions (the window, a hand edit), an open window shows the
        // list the settings hold.
        exclusions::refresh(&self.settings.excluded_apps);
        result
    }

    fn run_effect(&mut self, effect: Effect) -> Result<(), String> {
        match effect {
            Effect::None => {}
            Effect::SettingsChanged => self.apply_settings(),
            Effect::PinCurrentWindow => self.pin_last_foreground(),
            Effect::UnpinAll => self.unpin_all(),
            Effect::OpenSettingsFile => return self.request_open_settings_file(),
            Effect::OpenSizeDial => return self.open_size_dial(),
            Effect::OpenExclusions => return self.open_exclusions(),
            Effect::Exit => {
                info("exit requested from tray");
                post_quit();
            }
        }
        Ok(())
    }

    /// Opens the dial, or brings the open one to the front. The dial only posts
    /// `WM_DIAL_CHANGED` back and holds no reference to the `App`.
    fn open_size_dial(&mut self) -> Result<(), String> {
        let request = dial::Request {
            notify: self.hwnd,
            message: WM_DIAL_CHANGED,
            percent: self.settings.button_scale_percent,
            click_sound: self.settings.dial_click_sound,
        };
        dial::open(request)
            .map(drop)
            .map_err(|e| format!("button size dial could not be opened: {e}"))
    }

    /// Opens the excluded apps window, or brings the open one to the front. It queues what the
    /// user asks for and posts `WM_EXCLUSIONS_CHANGED`; it holds no reference to the `App`.
    fn open_exclusions(&mut self) -> Result<(), String> {
        let list = &self.settings.excluded_apps;
        exclusions::open(self.hwnd, WM_EXCLUSIONS_CHANGED, list)
            .map(drop)
            .map_err(|e| format!("excluded apps window could not be opened: {e}"))
    }

    /// The excluded apps window asked for changes: each one takes the same path as a menu
    /// command, so the settings are saved and announced to the observers.
    pub(super) fn on_exclusions_changed(&mut self) {
        for action in exclusions::drain_actions() {
            let command = match action {
                Action::Add(name) => tray::TrayCommand::AddExcludedApp(name),
                Action::Remove(name) => tray::TrayCommand::RemoveExcludedApp(name),
            };
            self.run_command(&command);
        }
    }

    /// The dial chose a size: the same path as the former size menu entries, so the settings are
    /// clamped, saved and announced to the observers.
    pub(super) fn on_dial_changed(&mut self, percent: u32) {
        self.run_command(&tray::TrayCommand::SetButtonScale(percent));
    }

    /// The window "Pin / unpin current window" acts on: the last foreground app window, if the
    /// hotkey rules (own process, excluded apps, elevated windows) allow it.
    fn tray_pin_target(&self) -> Option<Hwnd> {
        self.last_fg.filter(|&h| self.pin_target_ok(h))
    }

    fn pin_last_foreground(&mut self) {
        if let Some(h) = self.tray_pin_target() {
            self.toggle(h);
        }
    }
}
