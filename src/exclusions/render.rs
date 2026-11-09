//! Drawing of the excluded apps window with tiny-skia: frame, row backgrounds, trash icons,
//! the scroll indicator and the input field. The text is drawn on top by the window with GDI
//! at the positions `Layout` gives. No Win32 here.

use tiny_skia::{FillRule, LineCap, Mask, Path, PathBuilder, Pixmap, Rect, Transform};

use super::colors::{TRASH_REST, trash_hover};
use super::icon::{TrashIcon, draw_trash};
use super::layout::{Area, Layout};
use super::model::{Hit, ListState};
use crate::popup::Theme;
use crate::popup::draw::{
    Glow, GlowShape, draw_frame, draw_glow, fill_path, paint, rounded_rect, stroke, stroke_path,
};
use crate::popup::theme::BRAND_ACCENT;

/// Border width of the input field at rest and with an error, and of the focus ring, in dp.
const INPUT_BORDER_DP: f32 = 1.0;
const FOCUS_RING_DP: f32 = 1.5;
/// The glow around the focused input: its radius in dp and its strength on the dark and the
/// light theme (the light one is softer).
const FOCUS_GLOW_DP: f32 = 6.0;
const FOCUS_GLOW_DARK: f32 = 2.0;
const FOCUS_GLOW_LIGHT: f32 = 1.1;
/// Share of the brand color in a row at the start of its flash.
const FLASH_STRENGTH: f32 = 0.6;

/// What the field under the list looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InputLook {
    Idle,
    /// The EDIT control has the keyboard focus: a ring in the brand color with a glow.
    Focused,
    /// The last entry was refused: an outline in the error color.
    Invalid,
}

/// What the picture shows.
pub(super) struct Scene<'a> {
    pub(super) theme: Theme,
    pub(super) list: &'a ListState,
    pub(super) input: InputLook,
}

fn mix(from: [u8; 3], to: [u8; 3], share: f32) -> [u8; 3] {
    let blend = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * share).round() as u8;
    [
        blend(from[0], to[0]),
        blend(from[1], to[1]),
        blend(from[2], to[2]),
    ]
}

/// A mask that lets only `area` through, so rows cut off at the edge of the list stay inside.
fn clip_to(pm: &Pixmap, area: &Area) -> Option<Mask> {
    let mut mask = Mask::new(pm.width(), pm.height())?;
    let rect = Rect::from_xywh(area.x, area.y, area.w, area.h)?;
    mask.fill_path(
        &PathBuilder::from_rect(rect),
        FillRule::Winding,
        false,
        Transform::identity(),
    );
    Some(mask)
}

/// The tone behind a row: the selection and the hover are one step off the background, and a
/// flash leans toward the brand color.
fn row_background(scene: &Scene, index: usize) -> Option<[u8; 3]> {
    let theme = &scene.theme;
    let hovered = matches!(
        scene.list.hover(),
        Some(Hit::Row(i) | Hit::Trash(i)) if i == index
    );
    let base = if scene.list.selected() == Some(index) {
        Some(theme.track)
    } else if hovered {
        Some(theme.strip)
    } else {
        None
    };
    let flash = scene.list.flash_level(index);
    if flash > 0.0 {
        let under = base.unwrap_or(theme.background);
        return Some(mix(under, BRAND_ACCENT, FLASH_STRENGTH * flash));
    }
    base
}

/// The parts one frame is drawn from.
struct Painter<'a> {
    layout: &'a Layout,
    scene: &'a Scene<'a>,
    /// Limits row drawing to the list.
    clip: Option<Mask>,
}

impl Painter<'_> {
    fn fill_clipped(&self, pm: &mut Pixmap, shape: &Path, rgb: [u8; 3]) {
        pm.fill_path(
            shape,
            &paint(rgb),
            FillRule::Winding,
            Transform::identity(),
            self.clip.as_ref(),
        );
    }

    fn draw_row(&self, pm: &mut Pixmap, index: usize) {
        let (layout, scene) = (self.layout, self.scene);
        let row = scene.list.row_area(index);
        if let Some(rgb) = row_background(scene, index)
            && let Some(shape) = rounded_rect(row.x, row.y, row.w, row.h, layout.row_radius())
        {
            self.fill_clipped(pm, &shape, rgb);
        }
        if scene.list.selected() == Some(index) {
            let mark = layout.select_mark(&row);
            if let Some(shape) = rounded_rect(mark.x, mark.y, mark.w, mark.h, mark.w / 2.0) {
                self.fill_clipped(pm, &shape, BRAND_ACCENT);
            }
        }
        let rgb = if scene.list.hover() == Some(Hit::Trash(index)) {
            trash_hover(&scene.theme)
        } else {
            TRASH_REST
        };
        let icon = TrashIcon {
            area: layout.trash_icon(&row),
            rgb,
        };
        draw_trash(pm, &icon, self.clip.as_ref());
    }
}

fn draw_scrollbar(pm: &mut Pixmap, scene: &Scene) {
    let Some(thumb) = scene.list.scrollbar_thumb() else {
        return;
    };
    if let Some(shape) = rounded_rect(thumb.x, thumb.y, thumb.w, thumb.h, thumb.w / 2.0) {
        fill_path(pm, &shape, scene.theme.tick_idle);
    }
}

fn draw_input(pm: &mut Pixmap, layout: &Layout, scene: &Scene) {
    let area = layout.input_box();
    let theme = &scene.theme;
    let (rgb, width_dp) = match scene.input {
        InputLook::Idle => (theme.hairline, INPUT_BORDER_DP),
        InputLook::Focused => (BRAND_ACCENT, FOCUS_RING_DP),
        InputLook::Invalid => (theme.error, INPUT_BORDER_DP),
    };
    let width = layout.px(width_dp).max(1.0);
    let half = width / 2.0;
    // Inset by half the width so the whole stroke lies inside the box.
    let ring = rounded_rect(
        area.x + half,
        area.y + half,
        area.w - width,
        area.h - width,
        layout.input_radius() - half,
    );
    if scene.input == InputLook::Focused
        && let Some(ring) = &ring
    {
        draw_focus_glow(pm, layout, scene, (ring, width));
    }
    if let Some(shape) = rounded_rect(area.x, area.y, area.w, area.h, layout.input_radius()) {
        fill_path(pm, &shape, theme.surface);
    }
    if let Some(ring) = &ring {
        stroke_path(pm, ring, rgb, &stroke(width, LineCap::Butt));
    }
}

/// The glow of the focus ring, drawn before the field so that it only shows around it.
fn draw_focus_glow(pm: &mut Pixmap, layout: &Layout, scene: &Scene, ring: (&Path, f32)) {
    let strength = if scene.theme.dark {
        FOCUS_GLOW_DARK
    } else {
        FOCUS_GLOW_LIGHT
    };
    let glow = Glow::brand(&scene.theme, layout.px(FOCUS_GLOW_DP), strength);
    let shape = GlowShape::Stroke {
        path: ring.0,
        width: ring.1,
        cap: LineCap::Butt,
    };
    draw_glow(pm, &shape, &glow);
}

/// Draws the whole window without text. `None` only for a zero-sized layout.
pub(super) fn render_window(layout: &Layout, scene: &Scene) -> Option<Pixmap> {
    let (w, h) = layout.size_px();
    let mut pm = Pixmap::new(w, h)?;
    draw_frame(&mut pm, &scene.theme);
    let painter = Painter {
        layout,
        scene,
        clip: clip_to(&pm, &layout.list()),
    };
    for index in scene.list.visible() {
        painter.draw_row(&mut pm, index);
    }
    draw_scrollbar(&mut pm, scene);
    draw_input(&mut pm, layout, scene);
    Some(pm)
}

#[cfg(test)]
mod tests;
