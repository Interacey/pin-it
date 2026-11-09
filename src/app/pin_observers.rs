//! `PinEvent` and its observers. The pin controller only announces what happened to the pin
//! list; these observers decide what that means for buttons, the log and the files.

use std::path::PathBuf;

use super::crash_recovery::{SavedPin, window_identity, write_pins};
use super::logging::{info, warn};
use super::observer::{Observer, Subject};
use super::panic_registry;
use super::state::App;
use crate::Hwnd;

/// What happened to the pin list. Emitted after the state has changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PinEvent {
    /// The window entered the pin list; its topmost flag is applied next. Emitted first so the
    /// crash-recovery files know the window before it can be left topmost by a crash.
    Pinning(Hwnd),
    /// The window was pinned and its topmost flag applied.
    Pinned(Hwnd),
    /// The window left the pin list (unpinned, failed to pin, destroyed).
    Unpinned(Hwnd),
    /// The window is pinned but its thread is hung; the sweep retries it.
    Hung(Hwnd),
    /// The window is pinned but the system keeps it from becoming topmost; the sweep retries it.
    Refused(Hwnd),
    /// A window that was hung or refused takes its pin now.
    Recovered(Hwnd),
    /// A release that had failed went through (or the window is gone); it leaves `pins.json`.
    ReleaseSettled(Hwnd),
    /// The pin order was (re)applied; overlays have to be put back above their targets.
    OrderChanged,
}

/// Whether the event changed the pin list or the pending releases.
fn changes_pin_list(event: &PinEvent) -> bool {
    !matches!(
        event,
        PinEvent::Hung(_) | PinEvent::Refused(_) | PinEvent::Recovered(_) | PinEvent::OrderChanged
    )
}

/// Subscribes every pin observer. The order is the order in which they run: the button first, then
/// the log, then the lists that survive a crash.
pub(super) fn subscribe_all(events: &Subject<PinEvent, App>, pins_path: PathBuf) {
    events.subscribe(ButtonStyler);
    events.subscribe(PinLogger);
    events.subscribe(PanicRegistrySync);
    events.subscribe(PinPersistence {
        path: pins_path,
        synced: Vec::new(),
    });
    events.subscribe(OverlayRestacker);
}

/// Re-renders the pin button of the affected window.
struct ButtonStyler;

impl Observer<PinEvent, App> for ButtonStyler {
    fn on_event(&mut self, event: &PinEvent, app: &mut App) {
        match *event {
            PinEvent::Pinned(h)
            | PinEvent::Unpinned(h)
            | PinEvent::Hung(h)
            | PinEvent::Refused(h)
            | PinEvent::Recovered(h) => app.refresh_style(h),
            PinEvent::Pinning(_) | PinEvent::ReleaseSettled(_) | PinEvent::OrderChanged => {}
        }
    }
}

struct PinLogger;

impl Observer<PinEvent, App> for PinLogger {
    fn on_event(&mut self, event: &PinEvent, _app: &mut App) {
        match *event {
            PinEvent::Pinned(h) => info(&format!("pinned {h:#x}")),
            PinEvent::Unpinned(h) => info(&format!("unpinned {h:#x}")),
            PinEvent::Hung(h) => warn(&format!(
                "window {h:#x} is not responding, pin retried later"
            )),
            PinEvent::Refused(h) => warn(&format!(
                "window {h:#x} stays non-topmost, pin retried later"
            )),
            PinEvent::Recovered(h) => info(&format!("{h:#x} takes its pin again")),
            PinEvent::ReleaseSettled(h) => info(&format!("released {h:#x}")),
            PinEvent::Pinning(_) | PinEvent::OrderChanged => {}
        }
    }
}

/// Keeps the list the panic hook releases in step with the pin list.
struct PanicRegistrySync;

impl Observer<PinEvent, App> for PanicRegistrySync {
    fn on_event(&mut self, event: &PinEvent, app: &mut App) {
        if changes_pin_list(event) {
            panic_registry::replace(app.pinned_with_prior_topmost());
        }
    }
}

/// Writes the pin list and the pending releases to `pins.json` when they differ from what was
/// written last.
struct PinPersistence {
    path: PathBuf,
    synced: Vec<SavedPin>,
}

impl Observer<PinEvent, App> for PinPersistence {
    fn on_event(&mut self, event: &PinEvent, app: &mut App) {
        if !changes_pin_list(event) {
            return;
        }
        let saved = saved_pins(app);
        if saved == self.synced {
            return;
        }
        write_pins(&self.path, &saved);
        self.synced = saved;
    }
}

/// Pinned windows and windows whose release is still pending. Windows whose owning process cannot
/// be identified any more are left out.
fn saved_pins(app: &App) -> Vec<SavedPin> {
    let pinned = app.pinned_order().into_iter().map(|h| (h, false));
    let pending = app.pending_releases().into_iter().map(|h| (h, true));
    pinned
        .chain(pending)
        .filter_map(|(h, pending_release)| {
            let (pid, created) = window_identity(h)?;
            Some(SavedPin {
                hwnd: h,
                pid,
                created,
                was_topmost: !pending_release && app.prior_topmost.contains(&h),
                pending_release,
            })
        })
        .collect()
}

/// Puts the overlay windows back above their targets after the pin order was applied.
struct OverlayRestacker;

impl Observer<PinEvent, App> for OverlayRestacker {
    fn on_event(&mut self, event: &PinEvent, app: &mut App) {
        if *event == PinEvent::OrderChanged {
            app.restack_all();
        }
    }
}
