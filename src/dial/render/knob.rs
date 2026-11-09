//! The knob as a plastic part: a bevelled rim lit from the top left with knurled ridges that turn
//! with the knob, a recessed face with an inner shadow, and a highlight that stays where the
//! light is. The indicator groove is in `indicator`.

use tiny_skia::{LineCap, PathBuilder, Pixmap, Transform};

use super::Ctx;
use super::paints::{circle, fill, fill_ring, linear, radial, ring, stroke_tint, stroke_with};
use super::palette::Tint;
use crate::popup::draw::{Glow, GlowShape, draw_glow};

pub(in crate::dial) const KNOB_RADIUS_DP: f32 = 56.0;
pub(in crate::dial) const FACE_RADIUS_DP: f32 = 45.0;
const RIDGE_COUNT: usize = 28;
const RIDGE_INNER_DP: f32 = 47.5;
const RIDGE_OUTER_DP: f32 = 54.5;
const RIDGE_WIDTH_DP: f32 = 1.1;
/// The light flank of a ridge sits this many degrees after its dark flank.
const RIDGE_FLANK_DEG: f32 = 3.0;
/// Width of the shadow the rim casts into the recessed face.
const RECESS_SHADE_DP: f32 = 4.0;
/// How far the lit side of the face sits toward the top left, in face radii.
const FACE_FOCUS: (f32, f32) = (-0.30, -0.35);
const FACE_SPREAD: f32 = 1.35;
/// The soft highlight on the face: center in knob radii, half sizes in dp and tilt in degrees.
const SPECULAR_CENTER: (f32, f32) = (-0.32, -0.40);
const SPECULAR_HALF_DP: (f32, f32) = (17.0, 8.0);
const SPECULAR_TILT_DEG: f32 = -38.0;
/// The glint along the rim, from the left to the top.
const GLINT_FROM_DEG: f32 = -88.0;
const GLINT_TO_DEG: f32 = -8.0;
const GLINT_RADIUS_DP: f32 = 54.0;
const GLINT_WIDTH_DP: f32 = 1.4;
const SHADOW_RADIUS_DP: f32 = 6.0;
const SHADOW_ALPHA: f32 = 0.35;
const SHADOW_OFFSET_DP: f32 = 3.0;
/// Direction toward the light as a unit vector (top left).
const TOWARD_LIGHT: f32 = -std::f32::consts::FRAC_1_SQRT_2;

fn opaque(rgb: [u8; 3]) -> Tint {
    Tint { rgb, alpha: 1.0 }
}

fn clear(t: Tint) -> Tint {
    Tint { alpha: 0.0, ..t }
}

/// The drop shadow under the knob: it does not turn, so it belongs to the static layer.
pub(super) fn draw_shadow(pm: &mut Pixmap, ctx: &Ctx) {
    let layout = &ctx.layout;
    let Some(disc) = circle(layout.center(), layout.px(KNOB_RADIUS_DP)) else {
        return;
    };
    let offset = layout.px(SHADOW_OFFSET_DP);
    let glow = Glow::shadow(layout.px(SHADOW_RADIUS_DP), SHADOW_ALPHA, (offset, offset));
    draw_glow(pm, &GlowShape::Fill(&disc), &glow);
}

/// The knob turned to `angle` degrees.
pub(super) fn draw_knob(pm: &mut Pixmap, ctx: &Ctx, angle: f32) {
    draw_rim(pm, ctx);
    draw_ridges(pm, ctx, angle);
    draw_face(pm, ctx);
    draw_recess(pm, ctx);
    draw_highlights(pm, ctx);
}

/// The ends of the gradient line that runs from the light side to the shaded side.
fn light_axis(ctx: &Ctx, radius_dp: f32) -> ((f32, f32), (f32, f32)) {
    let (cx, cy) = ctx.layout.center();
    let r = ctx.layout.px(radius_dp);
    (
        (cx + TOWARD_LIGHT * r, cy + TOWARD_LIGHT * r),
        (cx - TOWARD_LIGHT * r, cy - TOWARD_LIGHT * r),
    )
}

fn draw_rim(pm: &mut Pixmap, ctx: &Ctx) {
    let layout = &ctx.layout;
    let (plastic, center) = (&ctx.plastic, layout.center());
    let (Some(rim), Some(outline)) = (
        ring(center, layout.px(KNOB_RADIUS_DP), layout.px(FACE_RADIUS_DP)),
        circle(center, layout.px(KNOB_RADIUS_DP) - 0.5),
    ) else {
        return;
    };
    let (lit_side, shaded_side) = light_axis(ctx, KNOB_RADIUS_DP);
    let base = opaque(plastic.rim);
    let paints = [
        super::paints::solid(base),
        // One gradient along the light axis: lit at the top left, bare in the middle, shaded at
        // the bottom right.
        linear(
            lit_side,
            shaded_side,
            &[
                (0.0, plastic.rim_lit),
                (0.45, clear(plastic.rim_lit)),
                (0.55, clear(plastic.rim_shade)),
                (1.0, plastic.rim_shade),
            ],
        )
        .unwrap_or_else(|| super::paints::solid(clear(base))),
    ];
    for paint in &paints {
        fill_ring(pm, &rim, paint);
    }
    let seam = (layout.px(1.0).max(1.0), LineCap::Butt);
    stroke_tint(pm, &outline, plastic.seam, seam);
}

/// The knurling: 28 ridges, each a dark and a light flank. The flanks fade with the light
/// direction (they are lit at the top left and dark at the bottom right) while the ridges turn.
fn draw_ridges(pm: &mut Pixmap, ctx: &Ctx, angle: f32) {
    let layout = &ctx.layout;
    let (mut dark, mut light) = (PathBuilder::new(), PathBuilder::new());
    for i in 0..RIDGE_COUNT {
        let a = angle + 360.0 * i as f32 / RIDGE_COUNT as f32;
        for (builder, at) in [(&mut dark, a), (&mut light, a + RIDGE_FLANK_DEG)] {
            let (x, y) = layout.polar(at, RIDGE_INNER_DP);
            builder.move_to(x, y);
            let (x, y) = layout.polar(at, RIDGE_OUTER_DP);
            builder.line_to(x, y);
        }
    }
    let (lit_side, shaded_side) = light_axis(ctx, KNOB_RADIUS_DP);
    let plastic = &ctx.plastic;
    let width = (layout.px(RIDGE_WIDTH_DP).max(1.0), LineCap::Butt);
    if let (Some(path), Some(paint)) = (
        dark.finish(),
        linear(
            lit_side,
            shaded_side,
            &[(0.0, clear(plastic.ridge_dark)), (1.0, plastic.ridge_dark)],
        ),
    ) {
        stroke_with(pm, &path, &paint, width);
    }
    if let (Some(path), Some(paint)) = (
        light.finish(),
        linear(
            lit_side,
            shaded_side,
            &[
                (0.0, plastic.ridge_light),
                (1.0, clear(plastic.ridge_light)),
            ],
        ),
    ) {
        stroke_with(pm, &path, &paint, width);
    }
}

fn draw_face(pm: &mut Pixmap, ctx: &Ctx) {
    let layout = &ctx.layout;
    let (plastic, (cx, cy)) = (&ctx.plastic, layout.center());
    let radius = layout.px(FACE_RADIUS_DP);
    let focus = (cx + FACE_FOCUS.0 * radius, cy + FACE_FOCUS.1 * radius);
    if let (Some(disc), Some(paint)) = (
        circle((cx, cy), radius),
        radial(
            (cx, cy),
            focus,
            radius * FACE_SPREAD,
            &[
                (0.0, opaque(plastic.face_lit)),
                (1.0, opaque(plastic.face_edge)),
            ],
        ),
    ) {
        fill(pm, &disc, &paint);
    }
}

/// The edge of the recess: a shadow along the upper left inside, a bright lip at the lower right
/// and a dark seam between rim and face.
fn draw_recess(pm: &mut Pixmap, ctx: &Ctx) {
    let layout = &ctx.layout;
    let (plastic, center) = (&ctx.plastic, layout.center());
    let face = layout.px(FACE_RADIUS_DP);
    let (lit_side, shaded_side) = light_axis(ctx, FACE_RADIUS_DP);
    if let (Some(shade), Some(paint)) = (
        ring(center, face, face - layout.px(RECESS_SHADE_DP)),
        linear(
            lit_side,
            shaded_side,
            &[
                (0.0, plastic.recess_shade),
                (0.6, clear(plastic.recess_shade)),
            ],
        ),
    ) {
        fill_ring(pm, &shade, &paint);
    }
    let line = (layout.px(1.0).max(1.0), LineCap::Butt);
    if let (Some(lip), Some(paint)) = (
        circle(center, face + 0.5),
        linear(
            lit_side,
            shaded_side,
            &[(0.4, clear(plastic.recess_lip)), (1.0, plastic.recess_lip)],
        ),
    ) {
        stroke_with(pm, &lip, &paint, line);
    }
    if let Some(seam) = circle(center, face - 0.5) {
        stroke_tint(pm, &seam, plastic.seam, line);
    }
}

/// The highlights that do not turn with the knob: a soft ellipse on the face and a glint on the
/// rim, both at the top left where the light is.
fn draw_highlights(pm: &mut Pixmap, ctx: &Ctx) {
    let layout = &ctx.layout;
    let specular = ctx.plastic.specular;
    let (cx, cy) = layout.center();
    let knob = layout.px(KNOB_RADIUS_DP);
    let at =
        Transform::from_translate(cx + SPECULAR_CENTER.0 * knob, cy + SPECULAR_CENTER.1 * knob)
            .pre_rotate(SPECULAR_TILT_DEG)
            .pre_scale(layout.px(SPECULAR_HALF_DP.0), layout.px(SPECULAR_HALF_DP.1));
    if let (Some(blob), Some(paint)) = (
        circle((0.0, 0.0), 1.0),
        radial(
            (0.0, 0.0),
            (0.0, 0.0),
            1.0,
            &[(0.0, specular), (1.0, clear(specular))],
        ),
    ) {
        pm.fill_path(&blob, &paint, tiny_skia::FillRule::Winding, at, None);
    }
    draw_glint(pm, ctx);
}

fn draw_glint(pm: &mut Pixmap, ctx: &Ctx) {
    let layout = &ctx.layout;
    let specular = ctx.plastic.specular;
    let steps = 24;
    let mut b = PathBuilder::new();
    for i in 0..=steps {
        let a = GLINT_FROM_DEG + (GLINT_TO_DEG - GLINT_FROM_DEG) * i as f32 / steps as f32;
        let (x, y) = layout.polar(a, GLINT_RADIUS_DP);
        if i == 0 {
            b.move_to(x, y);
        } else {
            b.line_to(x, y);
        }
    }
    let from = layout.polar(GLINT_FROM_DEG, GLINT_RADIUS_DP);
    let to = layout.polar(GLINT_TO_DEG, GLINT_RADIUS_DP);
    if let (Some(path), Some(paint)) = (
        b.finish(),
        linear(
            from,
            to,
            &[
                (0.0, clear(specular)),
                (0.5, specular),
                (1.0, clear(specular)),
            ],
        ),
    ) {
        stroke_with(
            pm,
            &path,
            &paint,
            (layout.px(GLINT_WIDTH_DP), LineCap::Round),
        );
    }
}
