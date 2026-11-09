//! The button size dial: a small window with a rotary detent dial that sets
//! `Settings.button_scale_percent`, opened from the tray item "Button size...".
//!
//! Interaction design after the detent dial of Kokonut UI (270 degree arc, snap points, pull
//! towards the nearest notch while dragging, spring settle on release, arrow, Home/End and wheel
//! steps), rebuilt natively with tiny-skia and GDI text. The scale runs from 60 to 140 percent in
//! steps of 5 with 100 percent at 12 o'clock; every detent clicks (a short kick, a flash of the
//! tick, a pulse of the readout and a quiet sound).
//!
//! - `model`: the scale, detents and the ratchet math. `state`: drag, keys, wheel and the settle
//!   spring (`spring`); `feedback`: kick, flash and pulse of a crossing; `sound`: the click and the
//!   player seam; `egg`, `jingle`: the easter egg (a quick 50 to 150 and back plays a 5 s chiptune).
//! - `size`: the product's button size for a percent.
//! - `render`, `text`: the picture. `window`: the Win32 window; it only posts the chosen
//!   percent to the app window. Theme, drawing helpers, text and window basics are in `popup`.

mod egg;
mod feedback;
mod jingle;
mod model;
mod render;
mod size;
mod sound;
mod spring;
mod state;
mod text;
mod window;

pub(crate) use window::{Request, close, open};

#[cfg(test)]
pub(crate) use window::probe;
