//! Application core: hidden owner window, message loop and wiring of all modules.
//!
//! Threading: everything runs on one thread. A hidden top-level popup window (not message-only,
//! because `TaskbarCreated` is only broadcast to top-level windows) owns the tray icon, the
//! WinEvent notifications, the hotkey and the pin button notifications. The state lives in one
//! `App` reached through a thread-local pointer from the window procedure.
//!
//! Layout:
//! - `host`: owner window, window procedure, re-entrancy guard
//! - `runtime`: startup, shutdown, message loop
//! - `registry` and `entry`: windows that have a button, and keeping the buttons current
//! - `pin_controller`: pin, unpin, order, hung windows; announces `PinEvent`s
//! - `pin_observers`: what happens when the pin list changes
//! - `settings_service`, `settings_observers`: load, save and apply settings; announces
//!   `SettingsEvent`s
//! - `command_runner`, `open_settings`, `hotkey_binding`, `autostart_sync`, `tray_icon`: what the
//!   tray commands, the hotkey, the Run entry and the tray icon need from the `App`
//! - `window_events`: WinEvent handling, hotkey, sweep
//! - `crash_recovery`, `panic_registry`, `logging`, `winutil`: support
//! - `observer`: the generic subject/observer mechanism
//!
//! Decisions:
//! - Pins do not persist across restarts. On exit every pinned window is unpinned, so a closed Pin
//!   It never leaves windows stuck on top without a way to undo it.
//!   The pinned list is written to `pins.json` next to the settings on every change only so that a
//!   crashed run (Task Manager kill, abort) can be cleaned up by the next start; see
//!   `crash_recovery::recover_pins`. A panic also tries to unpin from a global list before the
//!   process aborts.
//! - A window that was already topmost when it was pinned (its own "always on top") stays topmost
//!   when it is unpinned.
//! - A pinned window is forgotten only when it is destroyed. While it is hidden, minimized, cloaked
//!   (other virtual desktop) or has lost its caption (borderless fullscreen) only its button is
//!   hidden; the pin and its place in the stack stay. Unpinned windows are dropped when they stop
//!   being eligible.
//! - `enabled = false` (tray switch) unpins every pinned window, hides all
//!   buttons, ignores the hotkey and stops order enforcement. Enabling shows the buttons again;
//!   no pin is restored.
//! - The only periodic work is a 2 s sweep (WM_TIMER); the button animation timer belongs to the
//!   overlay and runs only while an animation plays.
//! - Windows of this process are never pinnable. Tests use `RunOptions::only` to name their own
//!   windows explicitly.
//! - Re-entrancy: `host::with_app` is the only way to get `&mut App`, and it refuses to nest. A
//!   message that arrives while an `App` method runs (sent messages are dispatched while it waits
//!   on a cross-thread call) is posted again instead of being handled inside it. The tray menu
//!   runs a nested message loop, so the tray path is a free function that holds no `App`
//!   reference while the menu is open.
//! - Observers get the `App` as their context while a notification runs: it is the same `&mut App`
//!   the emitting method holds, passed down, so no second reference exists.

mod autostart_sync;
mod command_runner;
mod crash_recovery;
mod entry;
mod host;
mod hotkey_binding;
mod logging;
pub(crate) mod observer;
mod open_settings;
mod panic_registry;
mod pin_controller;
mod pin_observers;
mod registry;
mod runtime;
mod settings_observers;
mod settings_service;
mod state;
mod system_change;
mod tray_icon;
mod window_events;
mod winutil;

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) use panic_registry::release_pinned_after_panic;
pub use runtime::run;
#[cfg(test)]
pub(crate) use runtime::{RunOptions, Runtime, run_with};
#[cfg(test)]
pub(crate) use winutil::FOREGROUND_OVERRIDE;
