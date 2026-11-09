//! Hung-window guard: nothing is sent to a target whose thread is hung or busy.

use windows::Win32::Foundation::{
    ERROR_TIMEOUT, GetLastError, LPARAM, SetLastError, WIN32_ERROR, WPARAM,
};
use windows::Win32::UI::WindowsAndMessaging::{
    IsHungAppWindow, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_NULL,
};

use super::ApplyError;
use crate::Hwnd;
use crate::win32::to_hwnd;

/// How long the responsiveness probe waits before the target counts as busy.
const PROBE_TIMEOUT_MS: u32 = 150;

/// True when Windows reports the window's thread as not responding (about 5 s without message processing).
pub(crate) fn is_hung(hwnd: Hwnd) -> bool {
    // SAFETY: plain query on a handle that may be stale.
    unsafe { IsHungAppWindow(to_hwnd(hwnd)) }.as_bool()
}

/// True when the target thread did not answer a `WM_NULL` within `PROBE_TIMEOUT_MS`.
pub(super) fn is_busy(hwnd: Hwnd) -> bool {
    // SAFETY: WM_NULL has no side effects; the result out pointer is not needed.
    let ok = unsafe {
        SetLastError(WIN32_ERROR(0));
        SendMessageTimeoutW(
            to_hwnd(hwnd),
            WM_NULL,
            WPARAM(0),
            LPARAM(0),
            SMTO_ABORTIFHUNG,
            PROBE_TIMEOUT_MS,
            None,
        )
    };
    // SAFETY: reads the calling thread's last error.
    let err = unsafe { GetLastError() };
    ok.0 == 0 && (err == ERROR_TIMEOUT || err.0 == 0)
}

/// `Hung` when the target is hung or busy, so callers skip the cross-process call that would block.
pub(super) fn guard_responsive(hwnd: Hwnd) -> Result<(), ApplyError> {
    if is_hung(hwnd) || is_busy(hwnd) {
        Err(ApplyError::Hung)
    } else {
        Ok(())
    }
}
