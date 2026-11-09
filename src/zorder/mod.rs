//! Applies HWND_TOPMOST ordering from `PinStack::bottom_to_top`.
//!
//! Hung targets: a cross-process `SetWindowPos` blocks the caller until the target's thread
//! handles it. Measured against a WinForms helper whose UI thread slept for 25 s: synchronous
//! `SetWindowPos(HWND_TOPMOST)` blocked for 22.8 s, `IsHungAppWindow` turned true after about
//! 5 s, and `SWP_ASYNCWINDOWPOS` returned in 10 us. Async does not keep the pin order: with
//! 3 windows in 3 threads of 2 processes, 24 and 28 of 60 shuffled pin sequences were still
//! wrong 150 ms later. So pinning stays synchronous and is guarded: `IsHungAppWindow`, plus a
//! short `WM_NULL` probe for a target that is busy but not yet reported hung. Either gives
//! `ApplyError::Hung` and `SetWindowPos` is not called. Unpinning does not depend on the order of
//! other windows, so it is queued with `SWP_ASYNCWINDOWPOS` and needs no guard: a busy target loses
//! the topmost flag when it resumes, and Pin It never waits for it.

mod activate;
mod anomaly;
mod error;
#[cfg(test)]
mod faults;
mod guard;
mod order;
mod primitives;
mod snapshot;

pub(crate) use activate::activate;
pub(crate) use error::ApplyError;
pub(crate) use order::apply_order;
pub(crate) use primitives::{is_topmost, pin, unpin};
#[cfg(test)]
pub(crate) use {
    activate::test_hooks as activate_hooks,
    faults::{defer_unpin_of, fail_unpin_of, refuse_next_pin_of, refuse_pin_of},
    guard::is_hung,
    order::is_in_order,
    snapshot::z_order_snapshot,
};

#[cfg(test)]
mod real_windows;
#[cfg(test)]
mod tests;
