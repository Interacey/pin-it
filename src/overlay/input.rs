//! Mouse handling of the overlay: hover, press, release and the click it produces.

use windows::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent};

use super::state::State;
use crate::Hwnd;

/// A completed click: `message` goes to `notify` with `target` as `wParam`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Click {
    pub(super) notify: Hwnd,
    pub(super) message: u32,
    pub(super) target: Hwnd,
}

impl State {
    pub(super) fn set_hover(&mut self, hover: bool) {
        if self.hover != hover {
            self.hover = hover;
            self.anim.on_hover(hover);
            self.begin_transition();
        }
    }

    pub(super) fn mouse_move(&mut self) {
        if !self.hover {
            self.request_leave_message();
        }
        self.set_hover(true);
    }

    fn request_leave_message(&self) {
        let mut tme = TRACKMOUSEEVENT {
            cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
            dwFlags: TME_LEAVE,
            hwndTrack: self.hwnd,
            dwHoverTime: 0,
        };
        // SAFETY: valid struct for our own window.
        // Without leave tracking the hover tint would stay on; there is no fallback to run.
        let _ = unsafe { TrackMouseEvent(&mut tme) };
    }

    /// The caller calls `SetCapture` afterwards, once no `&mut State` is live.
    pub(super) fn button_down(&mut self) {
        self.pressed = true;
        self.anim.on_press();
        self.begin_transition();
    }

    /// Returns the click to post if the release counts as one.
    /// The caller calls `ReleaseCapture` afterwards, once no `&mut State` is live.
    pub(super) fn button_up(&mut self, x: i32, y: i32) -> Option<Click> {
        let was_pressed = std::mem::take(&mut self.pressed);
        self.anim.on_release();
        self.begin_transition();
        let side = self.size as i32;
        let inside = (0..side).contains(&x) && (0..side).contains(&y);
        (was_pressed && inside).then_some(Click {
            notify: self.notify,
            message: self.notify_msg,
            target: self.target,
        })
    }

    /// Mouse capture was taken away (for example by hiding): treat it as a release without click.
    pub(super) fn capture_lost(&mut self) {
        if self.pressed {
            self.pressed = false;
            self.anim.on_release();
            self.begin_transition();
        }
    }
}
