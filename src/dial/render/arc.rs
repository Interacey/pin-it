//! The scale ring around the knob: a recessed track over the 270 degree arc and, from 100 percent
//! to the knob, a glowing inlay in the brand color.

use tiny_skia::{LineCap, Path, PathBuilder, Pixmap};

use super::Ctx;
use super::paints::stroke_tint;
use super::palette::Tint;
use crate::dial::model::{MAX_ANGLE, MIN_ANGLE};
use crate::popup::draw::{Glow, GlowShape, draw_glow};
use crate::popup::theme::BRAND_ACCENT;

pub(in crate::dial) const TRACK_RADIUS_DP: f32 = 72.0;
const TRACK_WIDTH_DP: f32 = 6.0;
const INLAY_WIDTH_DP: f32 = 3.0;
const INLAY_GLOW_RADIUS_DP: f32 = 6.0;
/// The arc is drawn as a polyline with a point every this many degrees.
const SEGMENT_DEG: f32 = 1.5;
/// An inlay shorter than this many degrees is not drawn.
const MIN_INLAY_DEG: f32 = 0.05;
const EDGE_OFFSET_PX: f32 = 1.0;

/// Arc from `from` to `to` degrees on the track, as a polyline.
fn arc_path(ctx: &Ctx, from: f32, to: f32) -> Option<Path> {
    let mut b = PathBuilder::new();
    let (x, y) = ctx.layout.polar(from, TRACK_RADIUS_DP);
    b.move_to(x, y);
    let segments = ((to - from).abs() / SEGMENT_DEG).ceil().max(1.0) as u32;
    for i in 1..=segments {
        let angle = from + (to - from) * i as f32 / segments as f32;
        let (x, y) = ctx.layout.polar(angle, TRACK_RADIUS_DP);
        b.line_to(x, y);
    }
    b.finish()
}

/// The empty track: a groove with its light edge below.
pub(super) fn draw_track(pm: &mut Pixmap, ctx: &Ctx) {
    let Some(arc) = arc_path(ctx, MIN_ANGLE, MAX_ANGLE) else {
        return;
    };
    let width = ctx.layout.px(TRACK_WIDTH_DP);
    if let Some(below) = arc.clone().transform(tiny_skia::Transform::from_translate(
        0.0,
        ctx.layout.px(EDGE_OFFSET_PX),
    )) {
        stroke_tint(
            pm,
            &below,
            ctx.plastic.engraved_edge,
            (width, LineCap::Round),
        );
    }
    stroke_tint(pm, &arc, ctx.plastic.groove, (width, LineCap::Round));
}

/// The inlay from 12 o'clock to `angle`, with its glow underneath.
pub(super) fn draw_inlay(pm: &mut Pixmap, ctx: &Ctx, angle: f32) {
    let angle = angle.clamp(MIN_ANGLE, MAX_ANGLE);
    if angle.abs() < MIN_INLAY_DEG {
        return;
    }
    let Some(arc) = arc_path(ctx, 0.0, angle) else {
        return;
    };
    let width = ctx.layout.px(INLAY_WIDTH_DP);
    let glow = Glow::brand(
        &ctx.theme,
        ctx.layout.px(INLAY_GLOW_RADIUS_DP),
        ctx.plastic.glow,
    );
    let shape = GlowShape::Stroke {
        path: &arc,
        width,
        cap: LineCap::Round,
    };
    draw_glow(pm, &shape, &glow);
    let brand = Tint {
        rgb: BRAND_ACCENT,
        alpha: 1.0,
    };
    stroke_tint(pm, &arc, brand, (width, LineCap::Round));
}
