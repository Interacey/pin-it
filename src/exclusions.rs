//! The excluded apps window, opened from the tray item "Excluded apps...": the list of excluded
//! programs with a trash button per row, and under it a one-line input that adds a program.
//!
//! - `model`: the check and normalization of an entered name, and the list state (scroll,
//!   selection, hover, flash, hit testing). `layout`: positions in dp and pixels.
//! - `render`, `icon`, `colors`, `text`: the picture; the trash can is a stroked vector path.
//! - `window`, `input`: the Win32 window and its EDIT control. `actions`: what the window asks
//!   the app to do; the window never touches the `App` and only posts a message to it.
//!
//! Theme, drawing helpers, text and window basics are in `popup`, shared with the size dial.

mod actions;
mod colors;
mod icon;
mod input;
mod layout;
mod model;
mod render;
mod text;
mod window;

pub(crate) use actions::{Action, drain_actions};
pub(crate) use model::normalize_entry;
pub(crate) use window::{close, open, refresh};

#[cfg(test)]
pub(crate) use window::probe;
