//! Software rendering of the pin button with tiny-skia into premultiplied BGRA.
//!
//! Only the pin glyph is drawn, no disc or ring. Unpinned it is an outline in ink. Pinned, the head
//! (the body of the glyph) is filled with the window accent while the outline and the needle stay ink.
//! The glyph is placed by its rendered bounding box, so it sits in the middle of the button box.

use tiny_skia::{FillRule, LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, Stroke, Transform};

use crate::anim::ButtonAnim;
use crate::color::{ensure_visible, ensure_visible_with, hover_ink, mix, readable_on};

/// Edge of the design box the glyph is drawn in; the stroke width scales from it.
const BOX: f32 = 16.0;
/// The larger side of the glyph's bounding box relative to the button box. The rest leaves room for
/// the hover bump and the spring overshoot.
const GLYPH_FRACTION: f32 = 0.78;
/// Stroke width of the glyph in design units; scales with size / 16 (1 px at 16 px).
const STROKE_WIDTH: f32 = 1.0;
/// Thinnest stroke in physical pixels, so the outline stays visible at low DPI.
pub(super) const MIN_STROKE_PX: f32 = 1.0;
/// Opacity of the black or white fallback outline at rest; it rises to 1 with hover and with the fill.
const REST_INK_ALPHA: f32 = 0.78;
/// Least contrast of the pinned head against the title bar: higher than the 1.5 of the outline, so the fill
/// reads even when the window has no accent of its own (the head then is the bar color moved away from itself).
pub(crate) const MIN_HEAD_CONTRAST: f32 = 2.5;
/// Extra scale of the glyph at full hover.
const HOVER_SCALE_GAIN: f32 = 0.06;
/// Lowest alpha of any pixel. Windows lets mouse input fall through layered pixels with alpha 0, so
/// without this floor only the glyph would be clickable. Black at alpha 1 darkens a title bar by
/// at most one level per channel.
const HIT_ALPHA_FLOOR: u8 = 1;
/// The pin leans by this angle.
const PIN_TILT_DEG: f32 = 45.0;

/// Look of one button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ButtonStyle {
    /// Accent color (RGB): the fill of the pin head when pinned.
    pub(crate) accent: [u8; 3],
    /// Background color of the title bar the button sits on.
    pub(crate) title_bar: [u8; 3],
    /// True when the target is pinned.
    pub(crate) pinned: bool,
    /// Color of the symbols on the window's other caption buttons; the pin outline and needle take it. None: black or white.
    pub(crate) ink: Option<[u8; 3]>,
    /// Play animations; false makes every transition instant.
    pub(crate) animate: bool,
}

impl Default for ButtonStyle {
    fn default() -> Self {
        Self {
            accent: [128, 128, 128],
            title_bar: [255, 255, 255],
            pinned: false,
            ink: None,
            animate: true,
        }
    }
}

/// The colors of a button: ink for outline and needle at rest and under the mouse, and the fill of the head.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Palette {
    pub(super) ink: [u8; 3],
    pub(super) hover_ink: [u8; 3],
    pub(super) head: [u8; 3],
    /// Opacity of the ink at rest: 1 for the sampled ink, `REST_INK_ALPHA` for the black or white fallback.
    rest_alpha: f32,
}

impl Palette {
    /// The head is the accent, moved away from the title bar until it stands out (contrast `MIN_HEAD_CONTRAST`).
    /// The ink is the sampled caption-symbol color, kept as it is unless it hides on the title bar (or, when pinned,
    /// on the head); without a sample it is black or white, whichever reads better on the title bar.
    pub(super) fn of(style: &ButtonStyle) -> Self {
        let bar = style.title_bar;
        let head = ensure_visible_with(style.accent, bar, MIN_HEAD_CONTRAST);
        let Some(sampled) = style.ink else {
            let ink = readable_on(bar);
            return Self {
                ink,
                hover_ink: ink,
                head,
                rest_alpha: REST_INK_ALPHA,
            };
        };
        let visible = ensure_visible(sampled, bar);
        let ink = if style.pinned {
            ensure_visible(visible, head)
        } else {
            visible
        };
        Self {
            ink,
            hover_ink: hover_ink(ink, bar),
            head,
            rest_alpha: 1.0,
        }
    }

    /// Outline and needle paint: the ink steps toward `hover_ink` with the hover; the black or white fallback
    /// gets more opaque with hover and fill instead.
    fn line_paint(&self, anim: &ButtonAnim) -> Paint<'static> {
        let color = mix(self.ink, self.hover_ink, anim.hover());
        let emphasis = anim.hover().max(anim.fill());
        paint(color, self.rest_alpha + (1.0 - self.rest_alpha) * emphasis)
    }
}

fn paint(rgb: [u8; 3], alpha: f32) -> Paint<'static> {
    let mut p = Paint::default();
    let alpha_u8 = (alpha.clamp(0.0, 1.0) * f32::from(u8::MAX)).round() as u8;
    p.set_color_rgba8(rgb[0], rgb[1], rgb[2], alpha_u8);
    p.anti_alias = true;
    p
}

/// Stroke width in physical pixels for a button of `s` pixels at rest.
pub(super) fn stroke_px(s: f32) -> f32 {
    (STROKE_WIDTH * s / BOX).max(MIN_STROKE_PX)
}

/// The pin in glyph space: the head (cap, neck, flange) and the needle.
pub(super) struct Glyph {
    head: Path,
    needle: Path,
}

impl Glyph {
    pub(super) fn new() -> Option<Self> {
        Some(Self {
            head: pin_head()?,
            needle: pin_needle()?,
        })
    }

    /// Center and larger side of the bounding box of the tilted glyph, without stroke.
    fn tilted_bounds(&self) -> Option<((f32, f32), f32)> {
        let tilt = Transform::from_rotate(PIN_TILT_DEG);
        let head = self.head.clone().transform(tilt)?.compute_tight_bounds()?;
        let needle = self
            .needle
            .clone()
            .transform(tilt)?
            .compute_tight_bounds()?;
        let (left, right) = (
            head.left().min(needle.left()),
            head.right().max(needle.right()),
        );
        let (top, bottom) = (
            head.top().min(needle.top()),
            head.bottom().max(needle.bottom()),
        );
        let centre = ((left + right) / 2.0, (top + bottom) / 2.0);
        Some((centre, (right - left).max(bottom - top)))
    }
}

/// Pin head (cap, neck, flange) in the 16x16 design box, pointing up before the tilt.
fn pin_head() -> Option<Path> {
    let mut b = PathBuilder::new();
    b.move_to(-2.0, -7.5);
    b.line_to(2.0, -7.5);
    b.quad_to(2.75, -7.5, 2.75, -6.75);
    b.line_to(2.75, -5.0);
    b.quad_to(2.75, -4.25, 1.75, -4.25);
    b.line_to(1.75, -1.0);
    b.line_to(4.25, 1.0);
    b.quad_to(4.5, 2.5, 3.0, 2.5);
    b.line_to(-3.0, 2.5);
    b.quad_to(-4.5, 2.5, -4.25, 1.0);
    b.line_to(-1.75, -1.0);
    b.line_to(-1.75, -4.25);
    b.quad_to(-2.75, -4.25, -2.75, -5.0);
    b.line_to(-2.75, -6.75);
    b.quad_to(-2.75, -7.5, -2.0, -7.5);
    b.close();
    b.finish()
}

fn pin_needle() -> Option<Path> {
    let mut n = PathBuilder::new();
    n.move_to(0.0, 2.5);
    n.line_to(0.0, 8.5);
    n.finish()
}

/// Renders the button as `size * size * 4` bytes of premultiplied BGRA. Every pixel has an alpha of
/// at least `HIT_ALPHA_FLOOR`, so the whole box takes mouse input.
///
/// Returns an empty vector for `size == 0`.
pub(crate) fn render_button(size: u32, style: &ButtonStyle, anim: &ButtonAnim) -> Vec<u8> {
    let mut out = render_glyph_bgra(size, style, anim);
    for px in out.chunks_exact_mut(4) {
        px[3] = px[3].max(HIT_ALPHA_FLOOR);
    }
    out
}

/// The glyph alone, with transparent pixels around it.
pub(super) fn render_glyph_bgra(size: u32, style: &ButtonStyle, anim: &ButtonAnim) -> Vec<u8> {
    let Some(mut pm) = Pixmap::new(size, size) else {
        return Vec::new();
    };
    draw_glyph(&mut pm, size as f32, style, anim);
    let mut out = pm.take();
    for px in out.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    out
}

/// The pin: head fill (accent, grows with the fill amount), then the outline and the needle in ink.
fn draw_glyph(pm: &mut Pixmap, side: f32, style: &ButtonStyle, anim: &ButtonAnim) {
    let palette = Palette::of(style);
    let Some(glyph) = Glyph::new() else { return };
    let Some(transform) = glyph_transform(side, &glyph, anim) else {
        return;
    };
    let fill = anim.fill();
    if fill > 0.0 {
        let head = paint(palette.head, fill);
        pm.fill_path(&glyph.head, &head, FillRule::Winding, transform, None);
    }
    let line = palette.line_paint(anim);
    let stroke = glyph_stroke(side, transform);
    pm.stroke_path(&glyph.head, &line, &stroke, transform, None);
    pm.stroke_path(&glyph.needle, &line, &stroke, transform, None);
}

/// Glyph space to pixels: the tilted bounding box is centered on the button center and scaled to
/// `GLYPH_FRACTION` of the side, times the press spring and the hover bump around that center.
pub(super) fn glyph_transform(side: f32, glyph: &Glyph, anim: &ButtonAnim) -> Option<Transform> {
    let (centre, extent) = glyph.tilted_bounds()?;
    let live_scale = anim.scale() * (1.0 + HOVER_SCALE_GAIN * anim.hover());
    let scale = side * GLYPH_FRACTION / extent * live_scale;
    let middle = side / 2.0;
    Some(
        Transform::from_translate(middle, middle)
            .pre_scale(scale, scale)
            .pre_translate(-centre.0, -centre.1)
            .pre_rotate(PIN_TILT_DEG),
    )
}

/// Stroke with the pixel width at rest, converted to glyph space through the transform's scale.
fn glyph_stroke(side: f32, transform: Transform) -> Stroke {
    Stroke {
        width: stroke_px(side) / transform.sx.hypot(transform.kx),
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Stroke::default()
    }
}
