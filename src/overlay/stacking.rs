//! Z-order of the overlay: directly above its target, in the same topmost band.

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    GW_HWNDPREV, GWL_EXSTYLE, GetWindow, GetWindowLongW, HWND_NOTOPMOST, HWND_TOP, HWND_TOPMOST,
    IsWindow, IsWindowVisible, KillTimer, SET_WINDOW_POS_FLAGS, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOREDRAW, SWP_NOSIZE, SetTimer, SetWindowPos, WS_EX_TOPMOST,
};

use std::time::Instant;

use super::PinButton;
use super::pulse::{ACTIVATION_BURST, Pulse, left_button_down, pulse_wanted};
use super::state::State;

use crate::win32::to_hwnd;

/// A delayed restack after an activation: timer id and delay. The animation timer has id 1. The early one
/// catches the common case of a late raise before the eye notices, the late one a slow activation.
pub(super) const RESTACK_TIMERS: [(usize, u32); 2] = [(2, 100), (3, 30)];
/// Repeating z-order check while the target is the foreground window.
pub(super) const ORDER_WATCH_TIMER_ID: usize = 4;
/// The shortest interval `SetTimer` keeps; the watch only has to notice a pressed mouse button, the
/// 1 ms pulse then does the quick restacking.
const ORDER_WATCH_MS: u32 = 10;

/// Changes z-order only: no activation, move, resize or redraw.
const Z_ORDER_ONLY: SET_WINDOW_POS_FLAGS =
    SET_WINDOW_POS_FLAGS(SWP_NOACTIVATE.0 | SWP_NOMOVE.0 | SWP_NOSIZE.0 | SWP_NOREDRAW.0);

pub(super) fn is_topmost(h: HWND) -> bool {
    // SAFETY: reads the extended style of any window handle; a stale one yields 0.
    unsafe { GetWindowLongW(h, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 != 0 }
}

fn exists(h: HWND) -> bool {
    // SAFETY: IsWindow accepts any handle value.
    unsafe { IsWindow(Some(h)).as_bool() }
}

fn is_visible(h: HWND) -> bool {
    // SAFETY: IsWindowVisible accepts any handle value.
    unsafe { IsWindowVisible(h).as_bool() }
}

/// Moves `overlay` into the topmost band or out of it, when it is not already there.
fn match_topmost_band(overlay: HWND, want_topmost: bool) {
    if is_topmost(overlay) == want_topmost {
        return;
    }
    let group = if want_topmost {
        HWND_TOPMOST
    } else {
        HWND_NOTOPMOST
    };
    place_after(overlay, group);
}

/// Inserts `overlay` behind `after`; a failed move is repaired by the next restack.
fn place_after(overlay: HWND, after: HWND) {
    // SAFETY: repositions our own window; no `&mut State` is live across this call.
    let _ = unsafe { SetWindowPos(overlay, Some(after), 0, 0, 0, 0, Z_ORDER_ONLY) };
}

/// Puts `overlay` directly in front of `target` unless it already is.
///
/// Inserting behind the window above the target puts the overlay between them. That window may be
/// another overlay; its own restack sorts itself out. A topmost window is never the anchor for a
/// non-topmost overlay: when the target is the highest window of its band, the overlay goes to the
/// top of that band instead.
fn place_directly_above(overlay: HWND, target: HWND, target_topmost: bool) {
    // SAFETY: reads the z-order neighbour of a foreign window.
    let above = unsafe { GetWindow(target, GW_HWNDPREV) }.ok();
    if above == Some(overlay) {
        return;
    }
    let top_of_band = if target_topmost {
        HWND_TOPMOST
    } else {
        HWND_TOP
    };
    let anchor = above.filter(|&a| is_topmost(a) == target_topmost);
    place_after(overlay, anchor.unwrap_or(top_of_band));
}

impl PinButton {
    /// Keeps the overlay directly above its target in the z-order, topmost when the target is.
    ///
    /// The overlay has no owner, so Windows does not do this. The check is a few
    /// reads and `SetWindowPos` runs only when the overlay is out of place, so it is cheap
    /// to call on every event and sweep. Our own events are not hooked, so it cannot loop.
    /// Hides the overlay when the target window no longer exists.
    pub(crate) fn restack(&mut self) {
        let (overlay, target) = {
            let st = self.st();
            (st.hwnd, to_hwnd(st.target))
        };
        if overlay.0.is_null() {
            return;
        }
        if !restack_pair(overlay, target) && is_visible(overlay) {
            self.hide();
        }
    }

    /// Restacks once more after 30 ms and after 100 ms, and every millisecond for the first 120 ms
    /// when this is the overlay of the foreground window. The system raises a window that is being activated
    /// possibly after its foreground event reached us, which would leave the overlay behind it until
    /// the next sweep. Calling it again before a timer fires restarts its delay.
    pub(crate) fn restack_soon(&mut self) {
        let hwnd = self.st().hwnd;
        if hwnd.0.is_null() {
            return;
        }
        self.st().start_burst();
        for (id, delay_ms) in RESTACK_TIMERS {
            // SAFETY: timer on our own window. A failed SetTimer only drops this extra restack; the sweep is the net.
            let _ = unsafe { SetTimer(Some(hwnd), id, delay_ms, None) };
        }
    }
}

impl PinButton {
    /// Whether the z-order check runs.
    #[cfg(test)]
    pub(crate) fn watches_order(&mut self) -> bool {
        self.st().order_watch
    }

    /// Starts or stops a 10 ms check that the overlay is still directly above its target. A press on the
    /// title bar of the window the user works in raises it over the overlay without any event that reaches
    /// us (it is the foreground window already), so the overlay would stay buried until the next sweep.
    pub(crate) fn watch_order(&mut self, on: bool) {
        let st = self.st();
        if st.hwnd.0.is_null() || st.order_watch == on {
            return;
        }
        let hwnd = st.hwnd;
        // SAFETY: timer on our own window; a failed SetTimer leaves the sweep as the net.
        let running = unsafe {
            if on {
                SetTimer(Some(hwnd), ORDER_WATCH_TIMER_ID, ORDER_WATCH_MS, None) != 0
            } else {
                let _ = KillTimer(Some(hwnd), ORDER_WATCH_TIMER_ID);
                false
            }
        };
        let st = self.st();
        st.order_watch = running;
        if !running {
            st.pulse = None;
        }
    }
}

impl State {
    /// An activation just happened: the pulse restacks for a short while (foreground overlay only).
    fn start_burst(&mut self) {
        self.burst_until = Instant::now() + ACTIVATION_BURST;
        self.ensure_pulse();
    }

    /// Starts the pulse for the overlay of the foreground window; a no-op when it already runs.
    pub(super) fn ensure_pulse(&mut self) {
        if self.order_watch && self.pulse.is_none() && !self.hwnd.0.is_null() {
            self.pulse = Pulse::start(self.hwnd);
        }
    }

    /// Notes a held mouse button and starts the pulse when one is down.
    pub(super) fn watch_mouse(&mut self) {
        if left_button_down() {
            self.button_seen = Instant::now();
            self.ensure_pulse();
        }
    }

    /// One pulse message handled: ends the pulse when nothing asks for it any more.
    pub(super) fn pulse_handled(&mut self) {
        if let Some(pulse) = &self.pulse {
            pulse.acknowledge();
        }
        if left_button_down() {
            self.button_seen = Instant::now();
        }
        let wanted = pulse_wanted(Instant::now(), self.button_seen, self.burst_until);
        if !(wanted && self.order_watch) {
            self.pulse = None;
        }
    }
}

/// Puts `overlay` back above `target`; false when `target` no longer exists.
pub(super) fn restack_pair(overlay: HWND, target: HWND) -> bool {
    if !exists(target) {
        return false;
    }
    let target_topmost = is_topmost(target);
    match_topmost_band(overlay, target_topmost);
    place_directly_above(overlay, target, target_topmost);
    true
}
