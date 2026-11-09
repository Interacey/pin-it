//! `PinButton`: the handle that owns one overlay window and its state.

use windows::Win32::Foundation::HWND;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, GWLP_USERDATA, SW_HIDE, SW_SHOWNA, SWP_NOACTIVATE,
    SWP_NOZORDER, SetWindowLongPtrW, SetWindowPos, ShowWindow, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows::core::w;

use super::ButtonStyle;
use super::state::State;
use super::wndproc::register_class;
use crate::Hwnd;

/// One overlay window showing the pin button for one target window.
///
/// Must be created, used and dropped on the same thread, which has to pump messages.
pub(crate) struct PinButton {
    pub(super) state: *mut State,
}

impl PinButton {
    /// Creates a hidden, unowned overlay for `target`; keep it stacked with [`PinButton::restack`].
    ///
    /// An owner on another thread would attach both threads' input queues, so none is set.
    ///
    /// A click posts `notify_msg` to `notify_hwnd` with the target handle as `wParam`.
    pub(crate) fn new(
        target: Hwnd,
        notify_hwnd: Hwnd,
        notify_msg: u32,
    ) -> windows::core::Result<Self> {
        register_class();
        let hwnd = create_overlay_window()?;
        let state = Box::into_raw(Box::new(State::new(hwnd, target, notify_hwnd, notify_msg)));
        // SAFETY: the pointer stays valid until Drop.
        unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize) };
        Ok(Self { state })
    }

    pub(super) fn st(&mut self) -> &mut State {
        // SAFETY: `state` is a live Box allocation freed only in Drop.
        unsafe { &mut *self.state }
    }

    /// Returns the overlay window handle.
    #[cfg(test)]
    pub(crate) fn hwnd(&self) -> Hwnd {
        // SAFETY: see `st`.
        unsafe { (*self.state).hwnd.0 as isize }
    }

    /// Moves the button to screen position `(x, y)` with a square side of `size` pixels.
    pub(crate) fn set_slot(&mut self, x: i32, y: i32, size: u32) {
        let size = size.max(1);
        let hwnd = self.st().hwnd;
        if hwnd.0.is_null() {
            return;
        }
        // SAFETY: repositions our own window without activating it or changing z-order.
        // No `&mut State` is live here: the call sends WM_WINDOWPOSCHANGED to our wndproc.
        // A failed move leaves the old position; the next set_slot repeats it.
        let _ = unsafe {
            SetWindowPos(
                hwnd,
                None,
                x,
                y,
                size as i32,
                size as i32,
                SWP_NOACTIVATE | SWP_NOZORDER,
            )
        };
        let st = self.st();
        if st.size != size {
            st.size = size;
            st.redraw();
        }
    }

    /// Applies a new look and re-renders; pinned changes animate unless `animate` is false.
    pub(crate) fn set_style(&mut self, style: ButtonStyle) {
        self.st().apply_style(style);
    }

    /// Fades the overlay out and hides it when the fade is over (the target is being minimized); with
    /// `animate` off it is hidden at once.
    pub(crate) fn fade_out_then_hide(&mut self, animate: bool) {
        if animate {
            self.st().fade_out_then_hide();
        } else {
            self.hide();
        }
    }

    /// Makes the overlay invisible at once and brings it back after the window animation of the target
    /// (restore or maximize). Without `animate` nothing changes.
    pub(crate) fn hold_back(&mut self, animate: bool) {
        if animate {
            self.st().hold_back();
        }
    }

    /// Shows the overlay without activating it.
    pub(crate) fn show(&mut self) {
        self.st().reveal();
        let hwnd = self.st().hwnd;
        if !hwnd.0.is_null() {
            // SW_SHOWNA on purpose: it was the variant that stopped the system from refusing
            // HWND_TOPMOST for the overlay; keep it.
            // SAFETY: our own window; no `&mut State` is live (the call re-enters the wndproc).
            let _ = unsafe { ShowWindow(hwnd, SW_SHOWNA) };
        }
    }

    /// Hides the overlay.
    pub(crate) fn hide(&mut self) {
        let hwnd = self.st().hwnd;
        if !hwnd.0.is_null() {
            // SAFETY: as in `show`; hiding can release capture and re-enter via WM_CAPTURECHANGED.
            let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
        }
        let st = self.st();
        st.pressed = false;
        st.set_hover(false);
    }

    /// Advances the animation by `dt` seconds and redraws; returns true while still animating.
    #[cfg(test)]
    pub(crate) fn tick(&mut self, dt: f32) -> bool {
        self.st().tick(dt)
    }

    /// The constant alpha the overlay is presented with.
    #[cfg(test)]
    pub(crate) fn alpha(&self) -> u8 {
        // SAFETY: see `st`.
        unsafe { (*self.state).fade.alpha() }
    }

    /// Returns true while the animation timer is running.
    #[cfg(test)]
    pub(crate) fn is_animating(&self) -> bool {
        // SAFETY: see `st`.
        unsafe { (*self.state).timer_on }
    }
}

fn create_overlay_window() -> windows::core::Result<HWND> {
    // SAFETY: plain popup creation without owner or parent.
    unsafe {
        let hinst = GetModuleHandleW(None)?;
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            w!("PinItOverlay"),
            w!(""),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(hinst.into()),
            None,
        )
    }
}

impl Drop for PinButton {
    fn drop(&mut self) {
        // SAFETY: after DestroyWindow the WM_NCDESTROY handler has cleared USERDATA,
        // so freeing the state cannot race the window procedure. If the destroy fails
        // (wrong thread) the state is leaked rather than left dangling.
        unsafe {
            let hwnd = (*self.state).hwnd;
            if !hwnd.0.is_null() && DestroyWindow(hwnd).is_err() {
                return;
            }
            drop(Box::from_raw(self.state));
        }
    }
}
