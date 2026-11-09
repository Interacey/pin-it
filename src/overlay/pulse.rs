//! A 1 ms pulse that makes the overlay of the foreground window check its z-order right after a
//! mouse press or an activation. `SetTimer` cannot go below 10 ms, so a multimedia timer posts the
//! messages; the system raises a window that is clicked within about one display frame (4 ms
//! measured), and the check has to be quicker than that to keep the overlay from flickering.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Media::{
    TIME_CALLBACK_FUNCTION, TIME_KILL_SYNCHRONOUS, TIME_PERIODIC, timeBeginPeriod, timeEndPeriod,
    timeKillEvent, timeSetEvent,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

use crate::win32::to_hwnd;

/// Message the pulse posts to the overlay.
pub(super) const WM_PULSE: u32 = WM_APP + 1;
const PULSE_MS: u32 = 1;
const TIMER_RESOLUTION_MS: u32 = 1;
/// The pulse keeps running this long after the mouse button went up: the system raises the window
/// on the press or on the release, and a drag ends later.
pub(super) const BUTTON_AFTERGLOW: Duration = Duration::from_millis(200);
/// Length of a pulse that an activation starts.
pub(super) const ACTIVATION_BURST: Duration = Duration::from_millis(120);

/// Shared with the timer thread, which only reads it.
struct Shared {
    hwnd: isize,
    /// A message is queued and not yet handled; the timer posts no second one, so a busy message
    /// loop cannot be flooded.
    queued: AtomicBool,
}

/// The running multimedia timer; dropping it stops the timer and waits for its last callback.
pub(super) struct Pulse {
    id: u32,
    shared: Box<Shared>,
}

impl Pulse {
    pub(super) fn start(hwnd: HWND) -> Option<Self> {
        let shared = Box::new(Shared {
            hwnd: hwnd.0 as isize,
            queued: AtomicBool::new(false),
        });
        let user = &*shared as *const Shared as usize;
        // SAFETY: `shared` lives until the timer is killed in `Drop`, which waits for the callback.
        // The matching `timeEndPeriod` is in `Drop`, or right here when the timer did not start.
        let id = unsafe {
            timeBeginPeriod(TIMER_RESOLUTION_MS);
            let id = timeSetEvent(
                PULSE_MS,
                0,
                Some(on_tick),
                user,
                TIME_CALLBACK_FUNCTION | TIME_PERIODIC | TIME_KILL_SYNCHRONOUS,
            );
            if id == 0 {
                timeEndPeriod(TIMER_RESOLUTION_MS);
            }
            id
        };
        (id != 0).then_some(Self { id, shared })
    }

    /// The message was taken off the queue; the next tick may post again.
    pub(super) fn acknowledge(&self) {
        self.shared.queued.store(false, Ordering::Release);
    }
}

impl Drop for Pulse {
    fn drop(&mut self) {
        // SAFETY: kills our own timer; the synchronous kill returns after the last callback.
        unsafe {
            timeKillEvent(self.id);
            timeEndPeriod(TIMER_RESOLUTION_MS);
        }
    }
}

unsafe extern "system" fn on_tick(_id: u32, _msg: u32, user: usize, _dw1: usize, _dw2: usize) {
    // SAFETY: `user` is the `Shared` of a live `Pulse`, see `Pulse::start`.
    let shared = unsafe { &*(user as *const Shared) };
    if shared.queued.swap(true, Ordering::AcqRel) {
        return;
    }
    // SAFETY: posting to a window handle is safe even if it no longer exists.
    // A failed post means the window is gone; the pulse ends with it.
    let _ = unsafe { PostMessageW(Some(to_hwnd(shared.hwnd)), WM_PULSE, WPARAM(0), LPARAM(0)) };
}

/// Whether the left mouse button is held down right now.
pub(super) fn left_button_down() -> bool {
    // SAFETY: plain query of the global key state.
    let state = unsafe { GetAsyncKeyState(i32::from(VK_LBUTTON.0)) };
    state < 0
}

/// Whether the pulse has a reason to go on: the button is down or went up recently, or an
/// activation burst is still running.
pub(super) fn pulse_wanted(now: Instant, button_seen: Instant, burst_until: Instant) -> bool {
    now < burst_until || now.duration_since(button_seen) < BUTTON_AFTERGLOW
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pulse_runs_while_the_button_is_recent_or_the_burst_lasts() {
        let t0 = Instant::now();
        let later = |ms| t0 + Duration::from_millis(ms);
        assert!(pulse_wanted(later(150), t0, t0), "button up 150 ms ago");
        assert!(!pulse_wanted(later(250), t0, t0), "button up 250 ms ago");
        assert!(pulse_wanted(later(250), t0, later(300)), "burst running");
        assert!(!pulse_wanted(later(300), t0, later(300)), "burst over");
    }
}
