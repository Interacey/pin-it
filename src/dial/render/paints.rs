//! Paints and shapes the layers are built from: tinted solids, gradients, circles and rings.

use tiny_skia::{
    Color, FillRule, GradientStop, LineCap, LinearGradient, Paint, Path, PathBuilder, Pixmap,
    Point, RadialGradient, SpreadMode, Stroke, Transform,
};

use super::palette::Tint;

pub(super) fn tint_color(t: Tint) -> Color {
    Color::from_rgba8(
        t.rgb[0],
        t.rgb[1],
        t.rgb[2],
        (t.alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}

pub(super) fn solid(t: Tint) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color(tint_color(t));
    paint.anti_alias = true;
    paint
}

fn stops(list: &[(f32, Tint)]) -> Vec<GradientStop> {
    list.iter()
        .map(|&(at, t)| GradientStop::new(at, tint_color(t)))
        .collect()
}

fn shaded(shader: Option<tiny_skia::Shader<'static>>) -> Option<Paint<'static>> {
    Some(Paint {
        shader: shader?,
        anti_alias: true,
        ..Paint::default()
    })
}

/// A gradient along the line `from` to `to`; `list` holds positions from 0 to 1.
pub(super) fn linear(
    from: (f32, f32),
    to: (f32, f32),
    list: &[(f32, Tint)],
) -> Option<Paint<'static>> {
    shaded(LinearGradient::new(
        Point::from_xy(from.0, from.1),
        Point::from_xy(to.0, to.1),
        stops(list),
        SpreadMode::Pad,
        Transform::identity(),
    ))
}

/// A gradient from the point `focus` (position 0) out to the circle around `center` with
/// `radius` (position 1).
pub(super) fn radial(
    center: (f32, f32),
    focus: (f32, f32),
    radius: f32,
    list: &[(f32, Tint)],
) -> Option<Paint<'static>> {
    shaded(RadialGradient::new(
        Point::from_xy(focus.0, focus.1),
        0.0,
        Point::from_xy(center.0, center.1),
        radius,
        stops(list),
        SpreadMode::Pad,
        Transform::identity(),
    ))
}

pub(super) fn circle(center: (f32, f32), radius: f32) -> Option<Path> {
    PathBuilder::from_circle(center.0, center.1, radius)
}

/// The ring between two circles around `center`.
pub(super) fn ring(center: (f32, f32), outer: f32, inner: f32) -> Option<Path> {
    let mut b = PathBuilder::new();
    b.push_circle(center.0, center.1, outer);
    b.push_circle(center.0, center.1, inner);
    b.finish()
}

/// Fills a ring with `paint` (even-odd, so the inner circle stays empty).
pub(super) fn fill_ring(pm: &mut Pixmap, ring: &Path, paint: &Paint) {
    pm.fill_path(ring, paint, FillRule::EvenOdd, Transform::identity(), None);
}

pub(super) fn fill(pm: &mut Pixmap, shape: &Path, paint: &Paint) {
    pm.fill_path(shape, paint, FillRule::Winding, Transform::identity(), None);
}

/// Strokes `path` with `paint`.
pub(super) fn stroke_with(pm: &mut Pixmap, path: &Path, paint: &Paint, style: (f32, LineCap)) {
    let stroke = Stroke {
        width: style.0,
        line_cap: style.1,
        ..Stroke::default()
    };
    pm.stroke_path(path, paint, &stroke, Transform::identity(), None);
}

/// Strokes `path` with a tinted solid.
pub(super) fn stroke_tint(pm: &mut Pixmap, path: &Path, tint: Tint, style: (f32, LineCap)) {
    stroke_with(pm, path, &solid(tint), style);
}
