//! tiny-skia helpers shared by the popup windows: colors, strokes, the window frame and the
//! conversion to the pixel layout of a DIB. No Win32 here.

use tiny_skia::{
    Color, FillRule, LineCap, Paint, Path, PathBuilder, Pixmap, Rect, Stroke, Transform,
};

use super::Theme;

mod glow;

pub(crate) use glow::{Glow, GlowShape, draw_glow};

/// Width of the window border in pixels.
const BORDER_PX: f32 = 1.0;

pub(crate) fn color(rgb: [u8; 3]) -> Color {
    Color::from_rgba8(rgb[0], rgb[1], rgb[2], u8::MAX)
}

pub(crate) fn paint(rgb: [u8; 3]) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(color(rgb));
    p.anti_alias = true;
    p
}

pub(crate) fn stroke(width: f32, cap: LineCap) -> Stroke {
    Stroke {
        width,
        line_cap: cap,
        ..Stroke::default()
    }
}

pub(crate) fn stroke_path(pm: &mut Pixmap, path: &Path, rgb: [u8; 3], style: &Stroke) {
    pm.stroke_path(path, &paint(rgb), style, Transform::identity(), None);
}

pub(crate) fn fill_path(pm: &mut Pixmap, path: &Path, rgb: [u8; 3]) {
    pm.fill_path(
        path,
        &paint(rgb),
        FillRule::Winding,
        Transform::identity(),
        None,
    );
}

pub(crate) fn line(from: (f32, f32), to: (f32, f32)) -> Option<Path> {
    let mut b = PathBuilder::new();
    b.move_to(from.0, from.1);
    b.line_to(to.0, to.1);
    b.finish()
}

pub(crate) fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let mut b = PathBuilder::new();
    b.move_to(x + r, y);
    b.line_to(x + w - r, y);
    b.quad_to(x + w, y, x + w, y + r);
    b.line_to(x + w, y + h - r);
    b.quad_to(x + w, y + h, x + w - r, y + h);
    b.line_to(x + r, y + h);
    b.quad_to(x, y + h, x, y + h - r);
    b.line_to(x, y + r);
    b.quad_to(x, y, x + r, y);
    b.close();
    b.finish()
}

/// Fills the window with the background and draws the one pixel border.
pub(crate) fn draw_frame(pm: &mut Pixmap, theme: &Theme) {
    pm.fill(color(theme.background));
    let (w, h) = (pm.width() as f32, pm.height() as f32);
    let half = BORDER_PX / 2.0;
    if let Some(rect) = Rect::from_xywh(half, half, w - BORDER_PX, h - BORDER_PX) {
        let outline = PathBuilder::from_rect(rect);
        stroke_path(
            pm,
            &outline,
            theme.hairline,
            &stroke(BORDER_PX, LineCap::Butt),
        );
    }
}

/// The pixmap as premultiplied BGRA, the layout of a 32-bit DIB.
pub(crate) fn to_bgra(pm: Pixmap) -> Vec<u8> {
    let mut data = pm.take();
    for px in data.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bgra_conversion_swaps_red_and_blue() {
        let mut pm = Pixmap::new(2, 2).expect("pixmap");
        pm.fill(color([10, 20, 30]));
        let first = pm.pixel(0, 0).expect("pixel");
        let bgra = to_bgra(pm);
        assert_eq!(&bgra[..3], &[first.blue(), first.green(), first.red()]);
        assert_eq!(&bgra[..3], &[30, 20, 10]);
    }

    #[test]
    fn the_frame_is_background_inside_and_hairline_on_the_edge() {
        let mut pm = Pixmap::new(20, 20).expect("pixmap");
        draw_frame(&mut pm, &Theme::DARK);
        let rgb = |x, y| {
            let p = pm.pixel(x, y).expect("pixel");
            [p.red(), p.green(), p.blue()]
        };
        assert_eq!(rgb(10, 10), Theme::DARK.background);
        assert_eq!(rgb(0, 10), Theme::DARK.hairline);
        assert_eq!(rgb(10, 19), Theme::DARK.hairline);
    }
}
