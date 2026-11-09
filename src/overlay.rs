//! Layered overlay window per target; software-rendered button via UpdateLayeredWindow.
//!
//! [`render_button`] draws the pin glyph with tiny-skia into premultiplied
//! BGRA. [`PinButton`] owns one click-able, non-activating popup window that
//! shows that bitmap and reports clicks to a notify window.
//!
//! - `render`: the drawing, no Win32.
//! - `dib`, `state`: the bitmap behind the window and the per-window state with its animation timer.
//! - `input`, `wndproc`: mouse handling and the window procedure that feeds it.
//! - `button`, `stacking`, `pulse`: `PinButton`, its z-order upkeep and the 1 ms pulse that makes it quick.

mod button;
mod fade;
mod input;
mod pulse;
mod render;
mod stacking;
mod state;
mod wndproc;

#[cfg(test)]
mod input_tests;
#[cfg(test)]
mod render_tests;
#[cfg(test)]
mod stacking_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod window_tests;

pub(crate) use button::PinButton;
pub(crate) use render::ButtonStyle;
#[cfg(test)]
pub(crate) use render::MIN_HEAD_CONTRAST;
pub(crate) use render::render_button;
