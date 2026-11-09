//! Test-only fault injection: makes `pin` or `unpin` fail for one window, as a refusing or
//! failing target would.

use std::sync::atomic::{AtomicIsize, Ordering};

use crate::Hwnd;

static REFUSE_PIN: AtomicIsize = AtomicIsize::new(0);
static FAIL_UNPIN: AtomicIsize = AtomicIsize::new(0);
static REFUSE_NEXT_PIN: AtomicIsize = AtomicIsize::new(0);
static DEFER_UNPIN: AtomicIsize = AtomicIsize::new(0);

/// Clears its fault on drop.
pub(crate) struct Fault(&'static AtomicIsize);

impl Drop for Fault {
    fn drop(&mut self) {
        self.0.store(0, Ordering::Relaxed);
    }
}

fn inject(slot: &'static AtomicIsize, hwnd: Hwnd) -> Fault {
    slot.store(hwnd, Ordering::Relaxed);
    Fault(slot)
}

/// `pin` of `hwnd` reports `Refused` while the returned guard lives.
pub(crate) fn refuse_pin_of(hwnd: Hwnd) -> Fault {
    inject(&REFUSE_PIN, hwnd)
}

/// `unpin` of `hwnd` fails with `Failed` while the returned guard lives.
pub(crate) fn fail_unpin_of(hwnd: Hwnd) -> Fault {
    inject(&FAIL_UNPIN, hwnd)
}

/// `unpin` of `hwnd` returns `Ok` without touching the window while the returned guard lives, as a
/// queued request that the busy target has not applied yet. The test applies it later by calling
/// `unpin` after the guard is dropped.
pub(crate) fn defer_unpin_of(hwnd: Hwnd) -> Fault {
    inject(&DEFER_UNPIN, hwnd)
}

/// The next `pin` of `hwnd` reports `Refused`; later ones go through.
pub(crate) fn refuse_next_pin_of(hwnd: Hwnd) {
    REFUSE_NEXT_PIN.store(hwnd, Ordering::Relaxed);
}

pub(super) fn pin_refused(hwnd: Hwnd) -> bool {
    let once = REFUSE_NEXT_PIN
        .compare_exchange(hwnd, 0, Ordering::Relaxed, Ordering::Relaxed)
        .is_ok();
    once || REFUSE_PIN.load(Ordering::Relaxed) == hwnd
}

pub(super) fn unpin_deferred(hwnd: Hwnd) -> bool {
    DEFER_UNPIN.load(Ordering::Relaxed) == hwnd
}

pub(super) fn unpin_fails(hwnd: Hwnd) -> bool {
    FAIL_UNPIN.load(Ordering::Relaxed) == hwnd
}
