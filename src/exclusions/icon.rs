//! The trash can as a vector path, drawn with a stroke so it is sharp at any size. The art is a
//! pure outline in the style of the Lucide icons: a lid line with round ends, a small handle,
//! a body with slightly tapered sides and a rounded bottom, and two short lines inside.

use tiny_skia::{LineCap, LineJoin, Mask, Path, PathBuilder, Pixmap, Stroke, Transform};

use super::layout::Area;
use crate::popup::draw::paint;

/// The path is drawn on a 24 by 24 grid.
const GRID: f32 = 24.0;
/// Stroke width on that grid.
const STROKE_WIDTH: f32 = 1.5;

/// The outline on the 24 grid.
pub(super) fn trash_path() -> Option<Path> {
    let mut b = PathBuilder::new();
    // Lid.
    b.move_to(3.3, 6.0);
    b.line_to(20.7, 6.0);
    // Handle, rising from the lid.
    b.move_to(7.5, 6.0);
    b.cubic_to(8.2, 6.0, 8.4, 5.5, 8.7, 4.5);
    b.cubic_to(9.0, 3.5, 9.6, 2.9, 10.6, 2.9);
    b.line_to(13.4, 2.9);
    b.cubic_to(14.4, 2.9, 15.0, 3.5, 15.3, 4.5);
    b.cubic_to(15.6, 5.5, 15.8, 6.0, 16.5, 6.0);
    // Body.
    b.move_to(5.25, 8.35);
    b.line_to(6.5, 17.5);
    b.cubic_to(6.7, 19.3, 7.3, 20.2, 9.4, 20.2);
    b.line_to(14.6, 20.2);
    b.cubic_to(16.7, 20.2, 17.3, 19.3, 17.5, 17.5);
    b.line_to(18.75, 8.35);
    // The two lines inside.
    b.move_to(9.5, 10.9);
    b.line_to(10.0, 16.2);
    b.move_to(14.5, 10.9);
    b.line_to(14.0, 16.2);
    b.finish()
}

/// Where to draw the icon and in which color.
#[derive(Debug, Clone, Copy)]
pub(super) struct TrashIcon {
    /// The square the 24 grid is stretched to.
    pub(super) area: Area,
    pub(super) rgb: [u8; 3],
}

/// Strokes the trash can into `pm`, limited to `clip` when given.
pub(super) fn draw_trash(pm: &mut Pixmap, icon: &TrashIcon, clip: Option<&Mask>) {
    let Some(path) = trash_path() else {
        return;
    };
    let scale = icon.area.w / GRID;
    let style = Stroke {
        width: STROKE_WIDTH,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Stroke::default()
    };
    let placement = Transform::from_row(scale, 0.0, 0.0, scale, icon.area.x, icon.area.y);
    pm.stroke_path(&path, &paint(icon.rgb), &style, placement, clip);
}

#[cfg(test)]
mod tests;
