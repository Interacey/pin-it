//! Pure color math on sRGB triples (WCAG luminance and contrast, blending, median).

use std::collections::HashMap;

const MIN_PIN_CONTRAST: f32 = 1.5;
/// Share of the remaining distance to black or white that the hover ink moves.
const HOVER_INK_STEP: f32 = 0.45;
/// Added to both luminances in the WCAG contrast ratio.
const CONTRAST_FLARE: f32 = 0.05;
const NUDGE_STEPS: u8 = 20;
const NUDGE_STEP: f32 = 0.05;

/// sRGB transfer function constants (WCAG 2.x relative luminance).
const SRGB_LINEAR_CUTOFF: f32 = 0.03928;
const SRGB_LINEAR_SLOPE: f32 = 12.92;
const SRGB_OFFSET: f32 = 0.055;
const SRGB_SCALE: f32 = 1.055;
const SRGB_GAMMA: f32 = 2.4;
const LUMA_RED: f32 = 0.2126;
const LUMA_GREEN: f32 = 0.7152;
const LUMA_BLUE: f32 = 0.0722;

fn linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= SRGB_LINEAR_CUTOFF {
        c / SRGB_LINEAR_SLOPE
    } else {
        ((c + SRGB_OFFSET) / SRGB_SCALE).powf(SRGB_GAMMA)
    }
}

/// WCAG relative luminance of an sRGB color, 0.0 (black) to 1.0 (white).
pub(crate) fn luminance(rgb: [u8; 3]) -> f32 {
    LUMA_RED * linear(rgb[0]) + LUMA_GREEN * linear(rgb[1]) + LUMA_BLUE * linear(rgb[2])
}

/// WCAG contrast ratio between two colors, 1.0 to 21.0.
pub(crate) fn contrast_ratio(a: [u8; 3], b: [u8; 3]) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + CONTRAST_FLARE) / (la.min(lb) + CONTRAST_FLARE)
}

/// True when white has more contrast than black on this color.
fn is_dark(rgb: [u8; 3]) -> bool {
    contrast_ratio(rgb, [255; 3]) > contrast_ratio(rgb, [0; 3])
}

/// Black or white, whichever has the higher contrast against `bg`.
pub(crate) fn readable_on(bg: [u8; 3]) -> [u8; 3] {
    if is_dark(bg) { [255; 3] } else { [0; 3] }
}

/// Linear blend per channel: `t` 0.0 gives `a`, 1.0 gives `b` (t is clamped).
pub(crate) fn mix(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    std::array::from_fn(|i| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t).round() as u8)
}

/// Returns `accent`, or a lighter (dark bg) or darker (light bg) variant when its contrast to `against_bg` is below 1.5.
pub(crate) fn ensure_visible(accent: [u8; 3], against_bg: [u8; 3]) -> [u8; 3] {
    ensure_visible_with(accent, against_bg, MIN_PIN_CONTRAST)
}

/// Like `ensure_visible` with a chosen minimum contrast ratio. A minimum that no color can reach on this
/// background (above 21) ends at black or white.
pub(crate) fn ensure_visible_with(
    accent: [u8; 3],
    against_bg: [u8; 3],
    min_contrast: f32,
) -> [u8; 3] {
    if contrast_ratio(accent, against_bg) >= min_contrast {
        return accent;
    }
    let target = readable_on(against_bg);
    (1..=NUDGE_STEPS)
        .map(|i| mix(accent, target, i as f32 * NUDGE_STEP))
        .find(|c| contrast_ratio(*c, against_bg) >= min_contrast)
        .unwrap_or(target)
}

/// The ink of a hovered caption symbol: the rest ink moved toward white on a dark bar or toward black on a light bar.
pub(crate) fn hover_ink(rest: [u8; 3], bar: [u8; 3]) -> [u8; 3] {
    mix(rest, readable_on(bar), HOVER_INK_STEP)
}

/// The most frequent color of the densest cluster: the color with the most pixels within `tolerance` per channel,
/// ties to the more frequent exact color, then to the larger value. A background that varies a little from pixel to
/// pixel (a gradient, noise) still wins against a flat patch of another color that has fewer pixels in total.
pub(crate) fn dominant_cluster(
    pixels: impl Iterator<Item = [u8; 3]>,
    tolerance: u8,
) -> Option<[u8; 3]> {
    let mut counts: HashMap<[u8; 3], u32> = HashMap::new();
    for p in pixels {
        *counts.entry(p).or_default() += 1;
    }
    let within =
        |a: &[u8; 3], b: &[u8; 3]| a.iter().zip(b).all(|(&x, &y)| x.abs_diff(y) <= tolerance);
    counts
        .iter()
        .map(|(color, &n)| {
            let cluster: u32 = counts
                .iter()
                .filter(|(c, _)| within(color, c))
                .map(|(_, &m)| m)
                .sum();
            (cluster, n, *color)
        })
        .max()
        .map(|(_, _, color)| color)
}

/// Per-channel median of the samples (upper median for even counts); None when empty.
pub(crate) fn median_color(samples: &[[u8; 3]]) -> Option<[u8; 3]> {
    if samples.is_empty() {
        return None;
    }
    Some(std::array::from_fn(|ch| {
        let mut v: Vec<u8> = samples.iter().map(|s| s[ch]).collect();
        v.sort_unstable();
        v[v.len() / 2]
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn luminance_extremes() {
        assert_eq!(luminance([0; 3]), 0.0);
        assert!((luminance([255; 3]) - 1.0).abs() < 1e-4);
        assert!((luminance([255, 0, 0]) - 0.2126).abs() < 1e-4);
    }

    #[test]
    fn contrast_black_white_is_21() {
        assert!((contrast_ratio([0; 3], [255; 3]) - 21.0).abs() < 1e-3);
        assert!((contrast_ratio([255; 3], [0; 3]) - 21.0).abs() < 1e-3);
        assert!((contrast_ratio([9, 9, 9], [9, 9, 9]) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn dark_and_readable() {
        assert!(is_dark([0, 0, 0]));
        assert!(is_dark([32, 32, 32]));
        assert!(!is_dark([255, 255, 255]));
        assert!(!is_dark([255, 255, 0]));
        assert_eq!(readable_on([20, 20, 20]), [255; 3]);
        assert_eq!(readable_on([240, 240, 240]), [0; 3]);
    }

    #[test]
    fn mix_endpoints_and_midpoint() {
        assert_eq!(mix([0, 0, 0], [255, 100, 50], 0.0), [0, 0, 0]);
        assert_eq!(mix([0, 0, 0], [255, 100, 50], 1.0), [255, 100, 50]);
        assert_eq!(mix([0, 0, 0], [200, 100, 50], 0.5), [100, 50, 25]);
        assert_eq!(mix([10, 10, 10], [20, 20, 20], 7.0), [20, 20, 20]);
    }

    #[test]
    fn ensure_visible_keeps_good_accent() {
        let a = [200, 30, 30];
        assert_eq!(ensure_visible(a, [255, 255, 255]), a);
    }

    #[test]
    fn ensure_visible_nudges_close_colors() {
        let bg = [32, 32, 32];
        let out = ensure_visible([34, 34, 34], bg);
        assert!(contrast_ratio(out, bg) >= 1.5);
        assert!(luminance(out) > luminance(bg));
        let bg = [240, 240, 240];
        let out = ensure_visible([238, 238, 238], bg);
        assert!(contrast_ratio(out, bg) >= 1.5);
        assert!(luminance(out) < luminance(bg));
    }

    #[test]
    fn ensure_visible_with_reaches_the_asked_contrast() {
        for bg in [[32, 32, 32], [243, 243, 243], [0, 120, 215]] {
            for min in [1.5, 2.5, 4.5] {
                let out = ensure_visible_with(bg, bg, min);
                assert!(contrast_ratio(out, bg) >= min, "{bg:?} at {min}: {out:?}");
            }
        }
        let bg = [32, 32, 32];
        assert_eq!(
            ensure_visible_with([200, 200, 200], bg, 2.5),
            [200, 200, 200]
        );
        assert_eq!(
            ensure_visible_with(bg, bg, 30.0),
            [255; 3],
            "unreachable minimum ends at the target"
        );
        assert_eq!(
            ensure_visible(bg, bg),
            ensure_visible_with(bg, bg, MIN_PIN_CONTRAST)
        );
    }

    #[test]
    fn a_varying_background_beats_a_flat_patch_with_fewer_pixels() {
        let mut px = vec![[23, 21, 25]; 100];
        px.extend((0..300).map(|i| [50 + (i % 3) as u8, 43, 68 - (i % 2) as u8]));
        let chosen = dominant_cluster(px.iter().copied(), 12).unwrap();
        assert!(chosen[0] >= 50 && chosen[0] <= 52, "{chosen:?}");
        assert_eq!(dominant_cluster(std::iter::empty(), 12), None);
    }

    #[test]
    fn hover_ink_follows_the_chrome_example() {
        let hovered = hover_ink([0x55; 3], [0x20, 0x21, 0x24]);
        for channel in hovered {
            assert!(channel.abs_diff(0xa0) <= 8, "{hovered:?}");
        }
    }

    #[test]
    fn hover_ink_darkens_on_a_light_bar() {
        let hovered = hover_ink([0x80; 3], [0xf3; 3]);
        assert!(hovered[0] < 0x80 && hovered[0] > 0x30, "{hovered:?}");
    }

    #[test]
    fn hover_ink_stays_in_range_at_the_extremes() {
        assert_eq!(hover_ink([255; 3], [0; 3]), [255; 3]);
        assert_eq!(hover_ink([0; 3], [255; 3]), [0; 3]);
        assert_eq!(hover_ink([0; 3], [0; 3]), [115; 3]);
    }

    #[test]
    fn median_per_channel() {
        assert_eq!(median_color(&[]), None);
        let s = [[10, 200, 0], [12, 0, 0], [11, 100, 255]];
        assert_eq!(median_color(&s), Some([11, 100, 0]));
        // one outlier does not move the result
        let s = [[50, 50, 50], [50, 50, 50], [255, 0, 255]];
        assert_eq!(median_color(&s), Some([50, 50, 50]));
    }
}
