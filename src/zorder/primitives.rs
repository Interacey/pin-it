//! Pin and unpin primitives: one `SetWindowPos` without activation. Pinning is guarded against
//! hung targets; unpinning is queued and never waits for the target.

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetWindowLongPtrW, HWND_NOTOPMOST, HWND_TOPMOST, IsIconic, IsWindowVisible,
    SET_WINDOW_POS_FLAGS, SWP_ASYNCWINDOWPOS, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_SHOWWINDOW, SetWindowPos, WS_EX_TOPMOST,
};

use super::ApplyError;
use super::guard::guard_responsive;
use crate::Hwnd;
use crate::win32::to_hwnd;

pub(super) const NO_FOCUS_FLAGS: SET_WINDOW_POS_FLAGS =
    SET_WINDOW_POS_FLAGS(SWP_NOMOVE.0 | SWP_NOSIZE.0 | SWP_NOACTIVATE.0);
const NO_FOCUS_SHOW_FLAGS: SET_WINDOW_POS_FLAGS =
    SET_WINDOW_POS_FLAGS(NO_FOCUS_FLAGS.0 | SWP_SHOWWINDOW.0);
/// Removing the topmost flag does not depend on the order of other windows, so the request can be
/// queued in the target's thread: it returns at once and is applied when the target resumes.
const NO_FOCUS_QUEUED_FLAGS: SET_WINDOW_POS_FLAGS =
    SET_WINDOW_POS_FLAGS(NO_FOCUS_FLAGS.0 | SWP_ASYNCWINDOWPOS.0);

/// Makes the window topmost, placing it above all other topmost windows, without activating it.
/// Returns `Hung` without touching the window when its thread is hung or busy, and `Refused` when
/// a visible window stays non-topmost although the system reported success.
pub(crate) fn pin(hwnd: Hwnd) -> Result<(), ApplyError> {
    #[cfg(test)]
    if super::faults::pin_refused(hwnd) {
        return Err(ApplyError::Refused);
    }
    set_z(hwnd, HWND_TOPMOST)
}

/// Removes the topmost flag from the window without activating it. The request is queued for a
/// target in another thread, so it succeeds for a hung window too and takes effect when the
/// target answers again; `is_topmost` can still be true right after the call.
pub(crate) fn unpin(hwnd: Hwnd) -> Result<(), ApplyError> {
    #[cfg(test)]
    if super::faults::unpin_fails(hwnd) {
        return Err(ApplyError::Failed(windows::Win32::Foundation::E_FAIL.0));
    }
    set_pos(hwnd, HWND_NOTOPMOST, NO_FOCUS_QUEUED_FLAGS)
}

/// Returns true when the window has the WS_EX_TOPMOST style.
pub(crate) fn is_topmost(hwnd: Hwnd) -> bool {
    // SAFETY: plain query on a handle that may be stale.
    let ex = unsafe { GetWindowLongPtrW(to_hwnd(hwnd), GWL_EXSTYLE) } as u32;
    ex & WS_EX_TOPMOST.0 != 0
}

fn set_z(hwnd: Hwnd, after: HWND) -> Result<(), ApplyError> {
    guard_responsive(hwnd)?;
    set_pos(hwnd, after, NO_FOCUS_FLAGS)?;
    if after == HWND_TOPMOST && !is_topmost(hwnd) {
        retry_topmost(hwnd)?;
    }
    Ok(())
}

/// Measured: a window that a background process showed without activation can end up at the bottom
/// of the z-order, and the first HWND_TOPMOST for it returns success but changes nothing. Done only
/// for a visible, restored window; a hidden or minimized one cannot be judged and counts as done.
fn retry_topmost(hwnd: Hwnd) -> Result<(), ApplyError> {
    let w = to_hwnd(hwnd);
    // SAFETY: plain queries on a handle that may be stale.
    let visible_and_restored = unsafe { IsWindowVisible(w).as_bool() && !IsIconic(w).as_bool() };
    if !visible_and_restored {
        return Ok(());
    }
    retry_topmost_steps(
        |after, flags| set_pos(hwnd, after, flags),
        || is_topmost(hwnd),
    )
}

/// The retries after a first HWND_TOPMOST that did not set the flag, each checked before the next:
/// 1. HWND_NOTOPMOST then HWND_TOPMOST, which resets the window's place in the band;
/// 2. HWND_TOPMOST with SWP_SHOWWINDOW (no activation, no effect on a visible window).
///
/// `Refused` only when the flag is still missing after all of them.
fn retry_topmost_steps(
    mut apply: impl FnMut(HWND, SET_WINDOW_POS_FLAGS) -> Result<(), ApplyError>,
    is_topmost: impl Fn() -> bool,
) -> Result<(), ApplyError> {
    // A failed drop of the flag is not fatal: the topmost call after it decides.
    let _ = apply(HWND_NOTOPMOST, NO_FOCUS_FLAGS);
    apply(HWND_TOPMOST, NO_FOCUS_FLAGS)?;
    if is_topmost() {
        return Ok(());
    }
    apply(HWND_TOPMOST, NO_FOCUS_SHOW_FLAGS)?;
    if is_topmost() {
        Ok(())
    } else {
        Err(ApplyError::Refused)
    }
}

pub(super) fn set_pos(
    hwnd: Hwnd,
    after: HWND,
    flags: SET_WINDOW_POS_FLAGS,
) -> Result<(), ApplyError> {
    // SAFETY: plain call; failure comes back as an error.
    unsafe { SetWindowPos(to_hwnd(hwnd), Some(after), 0, 0, 0, 0, flags) }?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_handle_maps_to_gone() {
        assert_eq!(pin(0x7FFF_FFF0), Err(ApplyError::Gone));
        assert_eq!(unpin(0x7FFF_FFF0), Err(ApplyError::Gone));
    }

    type Call = (HWND, SET_WINDOW_POS_FLAGS);

    /// Runs the retry with a fake `SetWindowPos`; the flag turns on after `succeeds_at` calls (0: never)
    /// and `failing` calls (counted from 1) return an error. Returns the result and the calls made.
    fn run(succeeds_at: usize, failing: &[usize]) -> (Result<(), ApplyError>, Vec<Call>) {
        let calls = std::cell::RefCell::new(Vec::new());
        let result = retry_topmost_steps(
            |after, flags| {
                calls.borrow_mut().push((after, flags));
                let n = calls.borrow().len();
                if failing.contains(&n) {
                    Err(ApplyError::Failed(1))
                } else {
                    Ok(())
                }
            },
            || succeeds_at != 0 && calls.borrow().len() >= succeeds_at,
        );
        (result, calls.into_inner())
    }

    #[test]
    fn retry_drops_the_flag_then_sets_it() {
        let (result, calls) = run(2, &[]);
        assert_eq!(result, Ok(()));
        assert_eq!(
            calls,
            vec![
                (HWND_NOTOPMOST, NO_FOCUS_FLAGS),
                (HWND_TOPMOST, NO_FOCUS_FLAGS)
            ]
        );
    }

    #[test]
    fn retry_ends_with_show_and_then_refuses() {
        let (result, calls) = run(0, &[]);
        assert_eq!(result, Err(ApplyError::Refused));
        assert_eq!(
            calls,
            vec![
                (HWND_NOTOPMOST, NO_FOCUS_FLAGS),
                (HWND_TOPMOST, NO_FOCUS_FLAGS),
                (HWND_TOPMOST, NO_FOCUS_SHOW_FLAGS)
            ]
        );
    }

    #[test]
    fn retry_uses_show_when_the_plain_retry_is_ignored() {
        let (result, calls) = run(3, &[]);
        assert_eq!(result, Ok(()));
        assert_eq!(calls.last(), Some(&(HWND_TOPMOST, NO_FOCUS_SHOW_FLAGS)));
    }

    #[test]
    fn retry_continues_after_a_failed_first_step() {
        let (result, calls) = run(2, &[1]);
        assert_eq!(result, Ok(()));
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[1], (HWND_TOPMOST, NO_FOCUS_FLAGS));
    }
}
