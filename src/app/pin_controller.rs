//! The pin controller: pin, unpin, order enforcement and hung-window handling.
//!
//! It changes the pin list and the topmost flags, then announces the result as a `PinEvent`.
//! Buttons, the log and the crash-recovery files are the business of the observers.
//!
//! Crash safety: a window is announced (`Pinning`) before its topmost flag is set, and a release
//! that the window did not take stays in `pending_release` (and in `pins.json`) until it does.

use std::sync::atomic::{AtomicBool, Ordering};

use super::logging::{info, log, warn};
use super::pin_observers::PinEvent;
use super::state::App;
use super::winutil::window_alive;
use crate::Hwnd;
use crate::zorder::{self, ApplyError};

/// Set after the first refused activation has been logged; the system refuses it by policy at times.
static ACTIVATION_DENIED_LOGGED: AtomicBool = AtomicBool::new(false);

/// How a release attempt ended.
enum Release {
    Done,
    /// The window did not take it; it is in `pending_release`.
    Deferred,
    Gone,
}

impl App {
    /// Pinned windows, oldest first.
    pub(super) fn pinned_order(&self) -> Vec<Hwnd> {
        self.pins.bottom_to_top().copied().collect()
    }

    /// Windows that may be topmost because of us, with the flag "was topmost before we pinned
    /// it": the pinned ones and those whose release is pending.
    pub(super) fn pinned_with_prior_topmost(&self) -> Vec<(Hwnd, bool)> {
        let pinned = self
            .pins
            .bottom_to_top()
            .map(|h| (*h, self.prior_topmost.contains(h)));
        let pending = self.pending_releases().into_iter().map(|h| (h, false));
        pinned.chain(pending).collect()
    }

    /// Windows whose release is pending, in a stable order.
    pub(super) fn pending_releases(&self) -> Vec<Hwnd> {
        let mut list: Vec<Hwnd> = self.pending_release.iter().copied().collect();
        list.sort_unstable();
        list
    }

    pub(super) fn emit_pin_event(&mut self, event: PinEvent) {
        let subject = self.pin_events.clone();
        subject.notify(&event, self);
    }

    /// A button was clicked. Only windows that have a button can be clicked; other values are spoofed.
    pub(super) fn on_pin_click(&mut self, target: Hwnd) {
        if self.entries.contains_key(&target) {
            self.toggle(target);
        }
    }

    /// Pins or unpins `h`, called for a click, the hotkey and the tray command. A new pin also takes
    /// the foreground once, whether the first attempt or the order enforcement made it topmost.
    /// A pin that is still refused or hung afterwards is not activated, and neither is it when the
    /// sweep settles it later. The caller has checked that the window may be pinned.
    pub(super) fn toggle(&mut self, h: Hwnd) {
        if !self.settings.enabled || self.down {
            return;
        }
        if !self.entries.contains_key(&h) {
            self.add_window(h);
        }
        let pinning = !self.pins.is_pinned(&h);
        if pinning {
            self.pin_window(h);
        } else {
            self.unpin_window(h);
        }
        self.ensure_order();
        if pinning && self.is_settled_topmost_pin(h) {
            activate_for_user(h);
        }
    }

    fn is_settled_topmost_pin(&self, h: Hwnd) -> bool {
        self.pins.is_pinned(&h) && !self.hung.contains(&h) && zorder::is_topmost(h)
    }

    fn pin_window(&mut self, h: Hwnd) {
        self.note_prior_topmost(h);
        self.pins.pin(h);
        // Write-ahead: the panic list and pins.json know the window before it can be left topmost.
        self.emit_pin_event(PinEvent::Pinning(h));
        match zorder::pin(h) {
            Ok(()) => self.emit_pin_event(PinEvent::Pinned(h)),
            Err(e) => self.pin_failed(h, e),
        }
    }

    /// Remembers an own "always on top". A window that is topmost only because our earlier release
    /// has not gone through yet (failed, or queued in a busy target) was not topmost before.
    fn note_prior_topmost(&mut self, h: Hwnd) {
        let still_ours = self.pending_release.remove(&h) | self.queued_release.remove(&h);
        if zorder::is_topmost(h) && !still_ours {
            self.prior_topmost.insert(h);
        }
    }

    fn unpin_window(&mut self, h: Hwnd) {
        self.hung.remove(&h);
        self.pins.unpin(&h);
        if let Release::Gone = self.release_or_defer(h) {
            warn(&format!("window {h:#x} is gone"));
            self.drop_entry(h);
        }
        self.emit_pin_event(PinEvent::Unpinned(h));
    }

    pub(super) fn unpin_all(&mut self) {
        for h in self.pinned_order() {
            self.pins.unpin(&h);
            self.hung.remove(&h);
            self.release_or_defer(h);
            self.prior_topmost.remove(&h);
            self.emit_pin_event(PinEvent::Unpinned(h));
        }
    }

    /// Unpins the window if it is pinned, and forgets it.
    pub(super) fn remove_window(&mut self, h: Hwnd) {
        if self.pins.is_pinned(&h) {
            self.release_or_defer(h);
        }
        self.drop_entry(h);
    }

    /// Forgets the window without touching it: pin, button and cached color.
    pub(super) fn drop_entry(&mut self, h: Hwnd) {
        let was_pinned = self.pins.unpin(&h);
        self.prior_topmost.remove(&h);
        self.hung.remove(&h);
        self.entries.remove(&h);
        self.colors.forget(h);
        if was_pinned {
            self.emit_pin_event(PinEvent::Unpinned(h));
        }
    }

    /// Takes our topmost flag away (unless the window was topmost before). A failure other than
    /// "gone" goes to `pending_release`, so the window is not forgotten while it may still be
    /// topmost; the sweep retries it. One log line per window until it is settled.
    fn release_or_defer(&mut self, h: Hwnd) -> Release {
        match self.release(h) {
            Ok(()) => Release::Done,
            Err(ApplyError::Gone) => Release::Gone,
            Err(e) => {
                if self.pending_release.insert(h) {
                    warn(&format!("cannot release {h:#x}, retried later: {e:?}"));
                }
                Release::Deferred
            }
        }
    }

    /// The unpin is queued in the target, so `Ok` only means "accepted". Until the flag is seen gone
    /// the window counts as still ours.
    fn note_queued_release(&mut self, h: Hwnd) {
        if zorder::is_topmost(h) {
            self.queued_release.insert(h);
        }
    }

    /// Forgets queued releases the target has applied, and those of windows that are gone.
    fn settle_queued_releases(&mut self) {
        self.queued_release
            .retain(|&h| window_alive(h) && zorder::is_topmost(h));
    }

    /// Called by the sweep: tries again every release that did not go through.
    pub(crate) fn retry_pending_releases(&mut self) {
        self.settle_queued_releases();
        for h in self.pending_releases() {
            let settled = matches!(self.release(h), Ok(()) | Err(ApplyError::Gone));
            if settled {
                self.pending_release.remove(&h);
                self.emit_pin_event(PinEvent::ReleaseSettled(h));
            }
        }
    }

    /// `prior_topmost` is cleared only on success, so a failed attempt can be retried.
    fn release(&mut self, h: Hwnd) -> Result<(), ApplyError> {
        if !window_alive(h) {
            return Err(ApplyError::Gone);
        }
        if !self.prior_topmost.contains(&h) {
            zorder::unpin(h)?;
            self.note_queued_release(h);
        }
        self.prior_topmost.remove(&h);
        Ok(())
    }

    /// Re-applies the pin order when it was disturbed, then puts every overlay back above its target.
    pub(super) fn ensure_order(&mut self) {
        if self.settings.enabled && !self.pins.is_empty() {
            self.enforce_pin_order();
        }
        self.emit_pin_event(PinEvent::OrderChanged);
    }

    fn enforce_pin_order(&mut self) {
        let outcome = zorder::apply_order(&self.pinned_order());
        if let Some(line) = &outcome.anomaly {
            info(line);
        }
        for (h, err) in outcome.failed {
            self.pin_failed(h, err);
        }
        for h in outcome.applied {
            if self.hung.remove(&h) {
                self.emit_pin_event(PinEvent::Recovered(h));
            }
        }
    }

    /// Undoes a pin that could not be applied or kept. A hung or refusing window is the exception:
    /// it stays pinned and the sweep retries it, with one log line per episode.
    fn pin_failed(&mut self, h: Hwnd, err: ApplyError) {
        if err.is_soft() && self.pins.is_pinned(&h) {
            self.note_soft_failure(h, &err);
            return;
        }
        self.hung.remove(&h);
        warn(&format!("pin failed for {h:#x}: {err:?}"));
        if err == ApplyError::Gone || !window_alive(h) {
            self.drop_entry(h);
            return;
        }
        self.pins.unpin(&h);
        // The window never became topmost through us, so a failing release is not kept.
        if let Err(e) = self.release(h) {
            warn(&format!("release after failed pin of {h:#x}: {e:?}"));
        }
        self.prior_topmost.remove(&h);
        self.emit_pin_event(PinEvent::Unpinned(h));
    }

    fn note_soft_failure(&mut self, h: Hwnd, err: &ApplyError) {
        if !self.hung.insert(h) {
            return;
        }
        let event = if *err == ApplyError::Refused {
            PinEvent::Refused(h)
        } else {
            PinEvent::Hung(h)
        };
        self.emit_pin_event(event);
    }
}

/// Brings a window the user just pinned to the foreground. The system may refuse (foreground
/// lock); that is logged once and does not matter for the pin.
fn activate_for_user(h: Hwnd) {
    if !zorder::activate(h) && !ACTIVATION_DENIED_LOGGED.swap(true, Ordering::Relaxed) {
        log("DEBUG", &format!("activation of {h:#x} was not granted"));
    }
}
