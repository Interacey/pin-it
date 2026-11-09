//! A soft glow (or shadow) around a shape: the shape is drawn into an offscreen alpha mask, the
//! mask is blurred with three box blurs (close to a gaussian) and the result is tinted and
//! composited into the pixmap. The caller draws the crisp shape on top afterwards.

use tiny_skia::{FillRule, LineCap, Paint, Path, Pixmap, Rect, Stroke, Transform};

use super::Theme;

/// Box blur passes; three give a result close to a gaussian.
const BLUR_PASSES: usize = 3;
/// The gaussian sigma that matches a glow of radius `r` is `r` times this.
const SIGMA_PER_RADIUS: f32 = 0.5;
/// Share of the radius the three passes together reach, plus a pixel of slack at each end.
const REACH_PER_RADIUS: f32 = 1.5;
const SLACK_PX: i32 = 2;

/// What gets a glow.
#[derive(Debug, Clone, Copy)]
pub(crate) enum GlowShape<'a> {
    Fill(&'a Path),
    Stroke {
        path: &'a Path,
        width: f32,
        cap: LineCap,
    },
}

/// How the tint meets the pixels below.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Blend {
    /// Adds light: right for a glow on a dark surface, where it can only brighten.
    Plus,
    /// Normal alpha blending: a tinted halo on a light surface, a shadow with black.
    Over,
}

/// The look of one glow.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Glow {
    pub(crate) color: [u8; 3],
    /// Blur radius in pixels; 0 draws nothing.
    pub(crate) radius_px: f32,
    /// Peak opacity of the tint, 0 to 1.
    pub(crate) strength: f32,
    pub(crate) blend: Blend,
    /// Where the glow sits relative to the shape (a shadow is offset), in pixels.
    pub(crate) offset: (f32, f32),
}

impl Glow {
    /// The brand glow of `theme`: additive on the dark theme, a tinted halo on the light one.
    pub(crate) fn brand(theme: &Theme, radius_px: f32, strength: f32) -> Glow {
        Glow {
            color: theme.glow_color(),
            radius_px,
            strength,
            blend: if theme.dark { Blend::Plus } else { Blend::Over },
            offset: (0.0, 0.0),
        }
    }

    /// A black drop shadow of `alpha` at `offset`.
    pub(crate) fn shadow(radius_px: f32, alpha: f32, offset: (f32, f32)) -> Glow {
        Glow {
            color: [0, 0, 0],
            radius_px,
            strength: alpha,
            blend: Blend::Over,
            offset,
        }
    }
}

/// A blurred alpha mask placed at (`x0`, `y0`) of the pixmap; values 0 to 1.
struct Mask {
    x0: i32,
    y0: i32,
    width: usize,
    height: usize,
    alpha: Vec<f32>,
}

/// Draws `glow` around `shape` into `pm`. Pixels outside the pixmap are dropped; the result
/// stays premultiplied.
pub(crate) fn draw_glow(pm: &mut Pixmap, shape: &GlowShape, glow: &Glow) {
    if glow.radius_px <= 0.0 || glow.strength <= 0.0 {
        return;
    }
    let Some(mask) = blurred_mask(pm, shape, glow) else {
        return;
    };
    composite(pm, &mask, glow);
}

fn shape_bounds(shape: &GlowShape) -> Option<Rect> {
    match shape {
        GlowShape::Fill(path) => Some(path.bounds()),
        GlowShape::Stroke { path, width, .. } => {
            let b = path.bounds();
            let pad = width / 2.0;
            Rect::from_ltrb(
                b.left() - pad,
                b.top() - pad,
                b.right() + pad,
                b.bottom() + pad,
            )
        }
    }
}

/// The pixel region the glow can reach, limited to the pixmap plus the reach of the blur.
fn region(pm: &Pixmap, shape: &GlowShape, glow: &Glow) -> Option<(i32, i32, usize, usize)> {
    let bounds = shape_bounds(shape)?;
    let reach = (glow.radius_px * REACH_PER_RADIUS).ceil() as i32 + SLACK_PX;
    let left = (bounds.left() + glow.offset.0).floor() as i32 - reach;
    let top = (bounds.top() + glow.offset.1).floor() as i32 - reach;
    let right = (bounds.right() + glow.offset.0).ceil() as i32 + reach;
    let bottom = (bounds.bottom() + glow.offset.1).ceil() as i32 + reach;
    let (x0, y0) = (left.max(-reach), top.max(-reach));
    let x1 = right.min(pm.width() as i32 + reach);
    let y1 = bottom.min(pm.height() as i32 + reach);
    (x1 > x0 && y1 > y0).then(|| (x0, y0, (x1 - x0) as usize, (y1 - y0) as usize))
}

fn blurred_mask(pm: &Pixmap, shape: &GlowShape, glow: &Glow) -> Option<Mask> {
    let (x0, y0, width, height) = region(pm, shape, glow)?;
    let mut canvas = Pixmap::new(width as u32, height as u32)?;
    let mut paint = Paint::default();
    paint.set_color_rgba8(255, 255, 255, 255);
    paint.anti_alias = true;
    let at = Transform::from_translate(glow.offset.0 - x0 as f32, glow.offset.1 - y0 as f32);
    match shape {
        GlowShape::Fill(path) => canvas.fill_path(path, &paint, FillRule::Winding, at, None),
        GlowShape::Stroke { path, width, cap } => {
            let style = Stroke {
                width: *width,
                line_cap: *cap,
                ..Stroke::default()
            };
            canvas.stroke_path(path, &paint, &style, at, None);
        }
    }
    let mut alpha: Vec<f32> = canvas
        .pixels()
        .iter()
        .map(|p| f32::from(p.alpha()) / 255.0)
        .collect();
    box_blur(&mut alpha, (width, height), glow.radius_px);
    Some(Mask {
        x0,
        y0,
        width,
        height,
        alpha,
    })
}

/// Width of one box (odd) of the three passes for a glow of `radius_px`.
fn box_width(radius_px: f32) -> usize {
    let sigma = radius_px * SIGMA_PER_RADIUS;
    let ideal = (12.0 * sigma * sigma / BLUR_PASSES as f32 + 1.0).sqrt();
    let width = ideal.round().max(1.0) as usize;
    width | 1
}

fn box_blur(data: &mut [f32], size: (usize, usize), radius_px: f32) {
    let half = (box_width(radius_px) - 1) / 2;
    if half == 0 {
        return;
    }
    let (width, height) = size;
    let mut column = vec![0.0; height];
    let mut scratch = vec![0.0; width.max(height)];
    for _ in 0..BLUR_PASSES {
        for row in data.chunks_exact_mut(width) {
            blur_line(row, half, &mut scratch);
        }
        for col in 0..width {
            for (i, slot) in column.iter_mut().enumerate() {
                *slot = data[i * width + col];
            }
            blur_line(&mut column, half, &mut scratch);
            for (i, v) in column.iter().enumerate() {
                data[i * width + col] = *v;
            }
        }
    }
}

/// Moving average over `2 * half + 1` samples with zeros outside, in place.
fn blur_line(values: &mut [f32], half: usize, scratch: &mut [f32]) {
    let n = values.len();
    let norm = 1.0 / (2 * half + 1) as f32;
    let mut sum: f32 = values.iter().take(half).sum();
    for i in 0..n {
        if i + half < n {
            sum += values[i + half];
        }
        scratch[i] = sum * norm;
        if i >= half {
            sum -= values[i - half];
        }
    }
    values.copy_from_slice(&scratch[..n]);
}

fn composite(pm: &mut Pixmap, mask: &Mask, glow: &Glow) {
    let (pm_w, pm_h) = (pm.width() as i32, pm.height() as i32);
    let stride = pm.width() as usize * 4;
    let data = pm.data_mut();
    let color = glow.color.map(f32::from);
    for row in 0..mask.height {
        let y = mask.y0 + row as i32;
        if !(0..pm_h).contains(&y) {
            continue;
        }
        for col in 0..mask.width {
            let x = mask.x0 + col as i32;
            let a = (mask.alpha[row * mask.width + col] * glow.strength).clamp(0.0, 1.0);
            if a <= 0.0 || !(0..pm_w).contains(&x) {
                continue;
            }
            let at = y as usize * stride + x as usize * 4;
            blend_pixel(&mut data[at..at + 4], color, a, glow.blend);
        }
    }
}

/// One premultiplied RGBA pixel under a tint of opacity `a`.
fn blend_pixel(px: &mut [u8], color: [f32; 3], a: f32, blend: Blend) {
    let below_a = f32::from(px[3]);
    let alpha = match blend {
        Blend::Plus => below_a + 255.0 * a,
        Blend::Over => 255.0 * a + below_a * (1.0 - a),
    }
    .min(255.0);
    for (channel, tint) in px[..3].iter_mut().zip(color) {
        let below = f32::from(*channel);
        let value = match blend {
            Blend::Plus => below + tint * a,
            Blend::Over => tint * a + below * (1.0 - a),
        };
        *channel = value.min(alpha).round() as u8;
    }
    px[3] = alpha.round() as u8;
}

#[cfg(test)]
mod tests;
