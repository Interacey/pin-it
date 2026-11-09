//! The ticks of the scale, engraved into the surface: a dark cut with a light edge one pixel
//! below it. 100 percent is the largest tick in the brand color, 75 and 125 are big, the other
//! multiples of 5 are small. The tick under the pointer flashes.

use tiny_skia::{LineCap, Path, Pixmap};

use super::Ctx;
use super::paints::stroke_tint;
use super::palette::Tint;
use crate::dial::feedback::Flash;
use crate::dial::model::{DETENTS, TickClass, angle_of_percent, tick_class};
use crate::popup::draw::{Glow, GlowShape, draw_glow, line};
use crate::popup::theme::{BRAND_ACCENT, mix_rgb};

/// Radius of the outer end of every tick.
const TICK_OUTER_DP: f32 = 100.0;
const GLOW_RADIUS_DP: f32 = 4.0;
const FLASH_GLOW_RADIUS_DP: f32 = 7.0;
/// Share (percent) of white a flashing tick is lifted by on the dark theme.
const FLASH_WHITE_PERCENT: u32 = 35;
/// A flashing tick is drawn this much wider than it is engraved.
const FLASH_WIDEN: f32 = 2.4;
/// The engraved edge sits this many pixels below the cut.
const EDGE_OFFSET_PX: f32 = 1.0;

/// Length and width of one tick class, in dp.
#[derive(Debug, Clone, Copy)]
pub(in crate::dial) struct TickSize {
    pub(in crate::dial) length_dp: f32,
    pub(in crate::dial) width_dp: f32,
}

pub(in crate::dial) const MAJOR: TickSize = TickSize {
    length_dp: 18.0,
    width_dp: 3.0,
};
pub(in crate::dial) const BIG: TickSize = TickSize {
    length_dp: 12.0,
    width_dp: 2.0,
};
pub(in crate::dial) const SMALL: TickSize = TickSize {
    length_dp: 7.0,
    width_dp: 1.0,
};

pub(in crate::dial) fn size_of(class: TickClass) -> TickSize {
    match class {
        TickClass::Major => MAJOR,
        TickClass::Big => BIG,
        TickClass::Small => SMALL,
    }
}

/// The tick of `percent` as a line from its inner to its outer end, and its width in pixels.
pub(super) fn tick_line(ctx: &Ctx, percent: u32) -> Option<(Path, f32)> {
    let angle = angle_of_percent(percent as f32);
    let size = size_of(tick_class(percent));
    let inner = ctx.layout.polar(angle, TICK_OUTER_DP - size.length_dp);
    let outer = ctx.layout.polar(angle, TICK_OUTER_DP);
    Some((line(inner, outer)?, ctx.layout.px(size.width_dp).max(1.0)))
}

fn shifted(path: &Path, dy: f32) -> Option<Path> {
    path.clone()
        .transform(tiny_skia::Transform::from_translate(0.0, dy))
}

fn glow_line(pm: &mut Pixmap, ctx: &Ctx, percent: u32, radius_dp: f32, strength: f32) {
    let Some((path, width)) = tick_line(ctx, percent) else {
        return;
    };
    let glow = Glow::brand(&ctx.theme, ctx.layout.px(radius_dp), strength);
    let shape = GlowShape::Stroke {
        path: &path,
        width,
        cap: LineCap::Butt,
    };
    draw_glow(pm, &shape, &glow);
}

/// All ticks, engraved; the major one in the brand color with a faint glow.
pub(super) fn draw_ticks(pm: &mut Pixmap, ctx: &Ctx) {
    let plastic = &ctx.plastic;
    let edge = ctx.layout.px(EDGE_OFFSET_PX);
    for percent in DETENTS {
        if let Some((path, width)) = tick_line(ctx, percent)
            && let Some(below) = shifted(&path, edge)
        {
            stroke_tint(pm, &below, plastic.engraved_edge, (width, LineCap::Butt));
        }
    }
    for percent in DETENTS {
        let major = tick_class(percent) == TickClass::Major;
        if major {
            glow_line(pm, ctx, percent, GLOW_RADIUS_DP, plastic.glow);
        }
        let cut = if major {
            Tint {
                rgb: BRAND_ACCENT,
                alpha: 1.0,
            }
        } else {
            plastic.engraved
        };
        if let Some((path, width)) = tick_line(ctx, percent) {
            stroke_tint(pm, &path, cut, (width, LineCap::Butt));
        }
    }
}

/// The tick under the pointer, brighter and with a pulse of glow; `flash.level` fades it out.
pub(super) fn draw_flash(pm: &mut Pixmap, ctx: &Ctx, flash: Flash) {
    let level = flash.level.clamp(0.0, 1.0);
    if level <= 0.0 {
        return;
    }
    glow_line(
        pm,
        ctx,
        flash.percent,
        FLASH_GLOW_RADIUS_DP,
        ctx.plastic.flash_glow * level,
    );
    let tone = if ctx.theme.dark {
        mix_rgb(ctx.theme.glow_color(), [255, 255, 255], FLASH_WHITE_PERCENT)
    } else {
        ctx.theme.glow_color()
    };
    let bright = Tint {
        rgb: tone,
        alpha: level,
    };
    if let Some((path, width)) = tick_line(ctx, flash.percent) {
        stroke_tint(pm, &path, bright, (width * FLASH_WIDEN, LineCap::Butt));
    }
}
