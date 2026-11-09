//! The indicator groove of the knob: a short radial cut toward the rim with a dark edge and a
//! brand colored glowing core. It turns with the knob and glows brighter during a flash.

use tiny_skia::{LineCap, Path, Pixmap};

use super::Ctx;
use super::paints::stroke_tint;
use super::palette::Tint;
use crate::popup::draw::{Glow, GlowShape, draw_glow, line};
use crate::popup::theme::BRAND_ACCENT;

pub(in crate::dial) const INNER_DP: f32 = 24.0;
pub(in crate::dial) const OUTER_DP: f32 = 40.0;
const GROOVE_WIDTH_DP: f32 = 4.4;
const CORE_WIDTH_DP: f32 = 2.4;
const GLOW_RADIUS_DP: f32 = 5.0;
const EDGE_OFFSET_PX: f32 = 1.0;

fn indicator_line(ctx: &Ctx, angle: f32) -> Option<Path> {
    line(
        ctx.layout.polar(angle, INNER_DP),
        ctx.layout.polar(angle, OUTER_DP),
    )
}

/// Draws the indicator at `angle`; `flash` (0 to 1) is how much the glow is lifted.
pub(super) fn draw_indicator(pm: &mut Pixmap, ctx: &Ctx, angle: f32, flash: f32) {
    let Some(path) = indicator_line(ctx, angle) else {
        return;
    };
    let layout = &ctx.layout;
    let plastic = &ctx.plastic;
    let groove = (layout.px(GROOVE_WIDTH_DP), LineCap::Round);
    let strength = plastic.glow + (plastic.flash_glow - plastic.glow) * flash.clamp(0.0, 1.0);
    let glow = Glow::brand(&ctx.theme, layout.px(GLOW_RADIUS_DP), strength);
    draw_glow(
        pm,
        &GlowShape::Stroke {
            path: &path,
            width: groove.0,
            cap: groove.1,
        },
        &glow,
    );
    if let Some(below) = path.clone().transform(tiny_skia::Transform::from_translate(
        0.0,
        layout.px(EDGE_OFFSET_PX),
    )) {
        stroke_tint(pm, &below, plastic.engraved_edge, groove);
    }
    stroke_tint(pm, &path, plastic.engraved, groove);
    let core = Tint {
        rgb: BRAND_ACCENT,
        alpha: 1.0,
    };
    stroke_tint(pm, &path, core, (layout.px(CORE_WIDTH_DP), LineCap::Round));
}
