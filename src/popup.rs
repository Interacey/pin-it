//! What the small popup windows share: the size dial and the excluded apps list.
//!
//! - `theme`: dark or light neutrals following the Windows app theme.
//! - `draw`: tiny-skia helpers, the window frame and the conversion to a DIB.
//! - `text`: GDI fonts and text output onto the DIB's memory DC.
//! - `placement`: the monitor under the cursor and the bottom-right position.
//! - `window`: class registration, creation with rounded corners, raise, and deactivation.

pub(crate) mod draw;
pub(crate) mod placement;
pub(crate) mod text;
pub(crate) mod theme;
pub(crate) mod window;

pub(crate) use theme::Theme;
