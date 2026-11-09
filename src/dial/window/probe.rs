//! Read access to the open dial for the tests in `app::tests`.

use std::cell::Cell;

use super::ui::{WINDOW, with_ui};
use crate::Hwnd;
use crate::dial::sound::counting;

/// The dial window; 0 when none is open.
pub(crate) fn window() -> Hwnd {
    WINDOW.with(Cell::get)
}

/// The percent the dial has chosen so far.
pub(crate) fn percent() -> Option<u32> {
    with_ui(|ui| ui.dial.percent())
}

/// Something (the knob or an effect) is still moving.
pub(crate) fn animating() -> Option<bool> {
    with_ui(|ui| ui.dial.is_animating())
}

/// The tick that is flashing right now.
pub(crate) fn flashing() -> Option<u32> {
    with_ui(|ui| ui.dial.flash().map(|f| f.percent)).flatten()
}

/// Client point at `radius_dp` from the dial center in direction `angle` degrees.
pub(crate) fn point_at(angle: f32, radius_dp: f32) -> Option<(i32, i32)> {
    with_ui(|ui| {
        let (x, y) = ui.layout.polar(angle, radius_dp);
        (x.round() as i32, y.round() as i32)
    })
}

/// Clicks the window asked its player for on this thread; the test player makes no sound.
pub(crate) fn clicks() -> usize {
    counting::requests()
}

pub(crate) fn reset_clicks() {
    counting::reset();
}

/// Jingle requests of the open window's player on this thread.
pub(crate) fn egg_requests() -> usize {
    counting::egg_requests()
}

/// Stop requests of the window's player on this thread.
pub(crate) fn egg_stops() -> usize {
    counting::stops()
}
