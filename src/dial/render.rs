//! Drawing of the dial window with tiny-skia. The layers that never change (frame, title bar,
//! track, ticks, the knob's shadow) are drawn once into a base picture per theme and scale; each
//! frame copies it and adds only what moves: the glyph, the inlay, the flash and the knob. The
//! text (readout and scale labels) is drawn on top by the window with GDI, at the positions
//! `Layout` gives. No Win32 here.

mod arc;
mod indicator;
mod knob;
mod layout;
mod paints;
mod palette;
mod ticks;
mod title_bar;

use tiny_skia::Pixmap;

pub(super) use layout::Layout;

use super::feedback::Flash;
use crate::popup::Theme;
use crate::popup::draw::draw_frame;
use palette::Plastic;

/// What one drawing pass needs besides the pixmap.
#[derive(Debug, Clone, Copy)]
struct Ctx {
    layout: Layout,
    theme: Theme,
    plastic: Plastic,
}

/// What the picture shows.
#[derive(Debug, Clone, Copy)]
pub(super) struct Scene {
    /// Knob angle in degrees, with its kick; the inlay runs from 0 to here.
    pub(super) angle: f32,
    /// The tick that flashes after a detent crossing.
    pub(super) flash: Option<Flash>,
    /// Edge of the pin glyph preview in physical pixels.
    pub(super) glyph_px: u32,
}

/// The static layers for one theme and scale, and the drawing of a frame on top of them.
pub(super) struct Renderer {
    ctx: Ctx,
    base: Pixmap,
}

impl Renderer {
    /// Draws the static layers. `None` only for a zero-sized layout.
    pub(super) fn new(layout: Layout, theme: Theme) -> Option<Renderer> {
        let (w, h) = layout.size_px();
        let mut base = Pixmap::new(w, h)?;
        let ctx = Ctx {
            layout,
            theme,
            plastic: Plastic::of(&theme),
        };
        draw_frame(&mut base, &theme);
        title_bar::draw_strip(&mut base, &ctx);
        arc::draw_track(&mut base, &ctx);
        ticks::draw_ticks(&mut base, &ctx);
        knob::draw_shadow(&mut base, &ctx);
        Some(Renderer { ctx, base })
    }

    /// One frame: the static layers plus everything that moves.
    pub(super) fn frame(&self, scene: &Scene) -> Pixmap {
        let mut pm = self.base.clone();
        let ctx = &self.ctx;
        title_bar::draw_glyph(&mut pm, ctx, scene.glyph_px);
        arc::draw_inlay(&mut pm, ctx, scene.angle);
        let level = scene.flash.map_or(0.0, |f| f.level);
        if let Some(flash) = scene.flash {
            ticks::draw_flash(&mut pm, ctx, flash);
        }
        knob::draw_knob(&mut pm, ctx, scene.angle);
        indicator::draw_indicator(&mut pm, ctx, scene.angle, level);
        pm
    }
}

/// Draws the whole window without text in one go, for tests that need no cache. `None` only for
/// a zero-sized layout.
#[cfg(test)]
pub(super) fn render_dial(layout: Layout, theme: Theme, scene: &Scene) -> Option<Pixmap> {
    Some(Renderer::new(layout, theme)?.frame(scene))
}

#[cfg(test)]
mod tests;
