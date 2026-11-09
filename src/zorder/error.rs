//! Failure kinds of z-order changes.

use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_INVALID_WINDOW_HANDLE};

/// Why a z-order change failed.
#[derive(Debug, PartialEq)]
pub(crate) enum ApplyError {
    /// The target belongs to a process with higher integrity (access denied).
    Elevated,
    /// The window no longer exists.
    Gone,
    /// The target's thread is hung or busy; nothing was sent. Retry later.
    Hung,
    /// `SetWindowPos` reported success but the visible window is still not topmost. A soft
    /// failure like `Hung`: the pin stays and the next sweep applies it again.
    Refused,
    /// Any other failure, carrying the HRESULT.
    Failed(i32),
}

impl ApplyError {
    /// A failure that keeps the pin: the sweep applies it again later.
    pub(crate) fn is_soft(&self) -> bool {
        matches!(self, ApplyError::Hung | ApplyError::Refused)
    }

    /// Maps an HRESULT (Win32 errors arrive as 0x8007xxxx) to an error kind.
    fn from_hresult(hr: i32) -> ApplyError {
        let win32 = |e: u32| windows::core::HRESULT::from_win32(e).0;
        if hr == win32(ERROR_ACCESS_DENIED.0) {
            ApplyError::Elevated
        } else if hr == win32(ERROR_INVALID_WINDOW_HANDLE.0) {
            ApplyError::Gone
        } else {
            ApplyError::Failed(hr)
        }
    }
}

impl From<windows::core::Error> for ApplyError {
    fn from(e: windows::core::Error) -> Self {
        ApplyError::from_hresult(e.code().0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_mapping() {
        let hr = |e: u32| windows::core::HRESULT::from_win32(e).0;
        assert_eq!(ApplyError::from_hresult(hr(5)), ApplyError::Elevated);
        assert_eq!(ApplyError::from_hresult(hr(1400)), ApplyError::Gone);
        assert_eq!(ApplyError::from_hresult(hr(87)), ApplyError::Failed(hr(87)));
        assert_eq!(
            ApplyError::from_hresult(0x8000_4005u32 as i32),
            ApplyError::Failed(0x8000_4005u32 as i32)
        );
    }
}
