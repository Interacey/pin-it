//! Mouse handling of the excluded apps window: hover, selection, the click on a trash button and
//! the wheel.

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent};

use super::keys::remove_row;
use super::{drive_animation, focus_input, with_ui};
use crate::exclusions::model::Hit;
use crate::popup::window::point_of;

fn track_mouse_leave(hwnd: HWND) {
    let mut track = TRACKMOUSEEVENT {
        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
        dwFlags: TME_LEAVE,
        hwndTrack: hwnd,
        dwHoverTime: 0,
    };
    // SAFETY: `track` is valid for the call. A failure only means the hover stays until the
    // next move.
    let _ = unsafe { TrackMouseEvent(&mut track) };
}

pub(super) fn on_mouse_move(hwnd: HWND, lparam: LPARAM) {
    let at = point_of(lparam);
    let (changed, track) = with_ui(|ui| {
        let changed = ui.list.set_hover(ui.list.hit(at));
        let track = !std::mem::replace(&mut ui.leave_tracked, true);
        (changed, track)
    })
    .unwrap_or_default();
    if track {
        track_mouse_leave(hwnd);
    }
    if changed {
        drive_animation(hwnd);
    }
}

pub(super) fn on_mouse_leave(hwnd: HWND) {
    let changed = with_ui(|ui| {
        ui.leave_tracked = false;
        ui.list.set_hover(None)
    });
    if changed == Some(true) {
        drive_animation(hwnd);
    }
}

pub(super) fn on_mouse_down(hwnd: HWND, lparam: LPARAM) {
    let at = point_of(lparam);
    with_ui(|ui| {
        ui.pressed = ui.list.hit(at);
        if let Some(Hit::Row(i) | Hit::Trash(i)) = ui.pressed {
            ui.list.select(i);
        }
    });
    focus_input();
    drive_animation(hwnd);
}

/// A click on a trash button counts when the button goes up on the one it went down on.
pub(super) fn on_mouse_up(hwnd: HWND, lparam: LPARAM) {
    let at = point_of(lparam);
    let clicked = with_ui(|ui| {
        let pressed = ui.pressed.take();
        pressed.filter(|&p| ui.list.hit(at) == Some(p))
    })
    .flatten();
    if let Some(Hit::Trash(index)) = clicked {
        remove_row(hwnd, index);
    }
}

pub(super) fn on_wheel(hwnd: HWND, wparam: WPARAM) {
    let delta = i32::from((wparam.0 >> 16) as u16 as i16);
    with_ui(|ui| ui.list.scroll_wheel(delta));
    drive_animation(hwnd);
}
