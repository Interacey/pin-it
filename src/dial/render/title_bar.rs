//! The imitated title bar at the top of the dial window, with the pin glyph at the size the user
//! would get.

use tiny_skia::{FillRule, LineCap, PathBuilder, Pixmap, PixmapPaint, PixmapRef, Rect, Transform};

use super::Ctx;
use crate::anim::ButtonAnim;
use crate::overlay::{ButtonStyle, render_button};
use crate::placement::GAP_TO_CAPTION_BUTTONS_DP;
use crate::popup::draw::{paint, rounded_rect, stroke, stroke_path};
use crate::popup::theme::BRAND_ACCENT;

const STRIP_RADIUS_DP: f32 = 8.0;
/// Edge of the minimize, maximize and close marks.
const CAPTION_MARK_DP: f32 = 10.0;
/// The bar that stands for the window title.
const TITLE_BAR_LEFT_DP: f32 = 16.0;
const TITLE_BAR_WIDTH_DP: f32 = 64.0;
const TITLE_BAR_HEIGHT_DP: f32 = 4.0;

/// The strip, the title bar and the caption marks; they never change.
pub(super) fn draw_strip(pm: &mut Pixmap, ctx: &Ctx) {
    let (layout, theme) = (&ctx.layout, &ctx.theme);
    let (x, y, w, h) = layout.strip();
    if let Some(strip) = rounded_rect(x, y, w, h, layout.px(STRIP_RADIUS_DP)) {
        pm.fill_path(
            &strip,
            &paint(theme.strip),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
    let mid_y = y + h / 2.0;
    let bar_h = layout.px(TITLE_BAR_HEIGHT_DP);
    if let Some(bar) = rounded_rect(
        x + layout.px(TITLE_BAR_LEFT_DP),
        mid_y - bar_h / 2.0,
        layout.px(TITLE_BAR_WIDTH_DP),
        bar_h,
        bar_h / 2.0,
    ) {
        pm.fill_path(
            &bar,
            &paint(theme.track),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
    draw_caption_marks(pm, ctx);
}

fn draw_caption_marks(pm: &mut Pixmap, ctx: &Ctx) {
    let layout = &ctx.layout;
    let (_, y, _, h) = layout.strip();
    let mid_y = y + h / 2.0;
    let half = layout.px(CAPTION_MARK_DP) / 2.0;
    let style = stroke(layout.px(1.0).max(1.0), LineCap::Butt);
    let left = layout.caption_group_left();
    let cell = layout.caption_cell();
    let center = |i: usize| left + cell * (i as f32 + 0.5);
    let mut marks = PathBuilder::new();
    marks.move_to(center(0) - half, mid_y);
    marks.line_to(center(0) + half, mid_y);
    if let Some(square) = Rect::from_xywh(center(1) - half, mid_y - half, 2.0 * half, 2.0 * half) {
        marks.push_rect(square);
    }
    marks.move_to(center(2) - half, mid_y - half);
    marks.line_to(center(2) + half, mid_y + half);
    marks.move_to(center(2) + half, mid_y - half);
    marks.line_to(center(2) - half, mid_y + half);
    if let Some(path) = marks.finish() {
        stroke_path(pm, &path, ctx.theme.muted, &style);
    }
}

/// The pin glyph, `size` pixels wide, left of the caption buttons.
pub(super) fn draw_glyph(pm: &mut Pixmap, ctx: &Ctx, size: u32) {
    let mut bgra = render_button(
        size,
        &ButtonStyle {
            accent: BRAND_ACCENT,
            title_bar: ctx.theme.strip,
            pinned: false,
            ink: None,
            animate: false,
        },
        &ButtonAnim::new(true),
    );
    for px in bgra.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    let Some(glyph) = PixmapRef::from_bytes(&bgra, size, size) else {
        return;
    };
    let layout = &ctx.layout;
    let (_, y, _, h) = layout.strip();
    let gap = layout.px(GAP_TO_CAPTION_BUTTONS_DP as f32);
    let x = layout.caption_group_left() - gap - size as f32;
    let top = y + (h - size as f32) / 2.0;
    pm.draw_pixmap(
        x.round() as i32,
        top.round() as i32,
        glyph,
        &PixmapPaint::default(),
        Transform::identity(),
        None,
    );
}
