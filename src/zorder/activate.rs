//! Brings a window to the foreground, for the pin a user has just asked for.

use windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow;

use super::guard::guard_responsive;
use crate::Hwnd;
use crate::win32::to_hwnd;

/// Activates the window and gives it the keyboard focus. Returns whether it became the
/// foreground window; the system refuses when the foreground lock applies, which is not an error.
/// Never calls into a window whose thread is hung or busy.
pub(crate) fn activate(hwnd: Hwnd) -> bool {
    #[cfg(test)]
    if !test_hooks::record_and_allow() {
        return false;
    }
    if guard_responsive(hwnd).is_err() {
        return false;
    }
    // SAFETY: plain call on a handle that may be stale; failure comes back as `false`.
    unsafe { SetForegroundWindow(to_hwnd(hwnd)) }.as_bool()
}

/// Tests count the calls and, by default, do not take the foreground from the user's desktop.
#[cfg(test)]
pub(crate) mod test_hooks {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    static CALLS: AtomicUsize = AtomicUsize::new(0);
    static REAL: AtomicBool = AtomicBool::new(false);

    pub(crate) fn calls() -> usize {
        CALLS.load(Ordering::Relaxed)
    }

    /// Lets `activate` reach `SetForegroundWindow` while the guard is alive.
    pub(crate) struct RealActivation;

    impl RealActivation {
        pub(crate) fn enable() -> Self {
            REAL.store(true, Ordering::Relaxed);
            RealActivation
        }
    }

    impl Drop for RealActivation {
        fn drop(&mut self) {
            REAL.store(false, Ordering::Relaxed);
        }
    }

    pub(super) fn record_and_allow() -> bool {
        CALLS.fetch_add(1, Ordering::Relaxed);
        REAL.load(Ordering::Relaxed)
    }
}
