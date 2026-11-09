//! The ink of the caption symbols: the color the minimize and maximize glyphs are drawn in, read from their button area.

use std::collections::HashMap;

use super::math::median_color;

/// A pixel counts as glyph when some channel differs this much from the button background.
const GLYPH_DISTANCE: u8 = 24;
/// Fewer glyph pixels than this means there is no glyph in the area.
const MIN_GLYPH_PIXELS: usize = 4;
/// More glyph pixels than this share of the area means the area is no plain background with a symbol.
const MAX_GLYPH_PERCENT: usize = 40;
/// The glyph color comes from the strongest 1/4 of the glyph pixels, so anti-aliased edges do not dilute it.
const CORE_DIVISOR: usize = 4;
/// A thin line has few full-strength pixels; the core never gets smaller than this.
const MIN_CORE_PIXELS: usize = 3;
/// Background colors that differ only in the low bits (dithering, Mica noise) count as one.
const BACKGROUND_BUCKET_SHIFT: u8 = 3;

/// Color of the symbol drawn on a plain background, from the readable pixels of the button area (any order).
/// None when no symbol stands out from the background.
pub(super) fn glyph_ink(pixels: &[[u8; 3]]) -> Option<[u8; 3]> {
    let background = area_background(pixels)?;
    let mut glyph: Vec<(u8, [u8; 3])> = pixels
        .iter()
        .map(|&p| (distance(p, background), p))
        .filter(|&(d, _)| d > GLYPH_DISTANCE)
        .collect();
    if glyph.len() < MIN_GLYPH_PIXELS || glyph.len() * 100 > pixels.len() * MAX_GLYPH_PERCENT {
        return None;
    }
    glyph.sort_unstable_by_key(|&(d, _)| std::cmp::Reverse(d));
    let core = (glyph.len() / CORE_DIVISOR).max(MIN_CORE_PIXELS);
    let colors: Vec<[u8; 3]> = glyph.iter().take(core).map(|&(_, p)| p).collect();
    median_color(&colors)
}

/// Largest per-channel difference.
fn distance(a: [u8; 3], b: [u8; 3]) -> u8 {
    a.iter()
        .zip(b)
        .map(|(&x, y)| x.abs_diff(y))
        .max()
        .unwrap_or(0)
}

/// Median color of the most frequent bucket of similar colors.
fn area_background(pixels: &[[u8; 3]]) -> Option<[u8; 3]> {
    let bucket = |p: &[u8; 3]| p.map(|c| c >> BACKGROUND_BUCKET_SHIFT);
    let mut counts: HashMap<[u8; 3], usize> = HashMap::new();
    for p in pixels {
        *counts.entry(bucket(p)).or_default() += 1;
    }
    let (best, _) = counts.into_iter().max_by_key(|&(key, n)| (n, key))?;
    let members: Vec<[u8; 3]> = pixels
        .iter()
        .filter(|p| bucket(p) == best)
        .copied()
        .collect();
    median_color(&members)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DARK_BAR: [u8; 3] = [0x20, 0x21, 0x24];
    const LIGHT_BAR: [u8; 3] = [0xde, 0xe1, 0xe6];
    const GLYPH: [u8; 3] = [0x55; 3];
    const WIDTH: usize = 46;
    const HEIGHT: usize = 32;

    /// A button area of `bg` with a 10 px wide, `thickness` px high line in `ink`, plus one anti-aliased row above it.
    fn area(bg: [u8; 3], ink: [u8; 3], thickness: usize) -> Vec<[u8; 3]> {
        let mut grid = vec![bg; WIDTH * HEIGHT];
        let top = HEIGHT / 2;
        for y in top..top + thickness {
            for x in 18..28 {
                grid[y * WIDTH + x] = ink;
            }
        }
        let edge = std::array::from_fn(|i| ((bg[i] as u16 + ink[i] as u16) / 2) as u8);
        for x in 18..28 {
            grid[(top - 1) * WIDTH + x] = edge;
        }
        grid
    }

    fn near(a: Option<[u8; 3]>, want: [u8; 3], tolerance: u8) -> bool {
        a.is_some_and(|a| distance(a, want) <= tolerance)
    }

    #[test]
    fn grey_glyph_on_a_dark_bar() {
        assert!(near(glyph_ink(&area(DARK_BAR, GLYPH, 1)), GLYPH, 2));
    }

    #[test]
    fn dark_glyph_on_a_light_bar() {
        let ink = [0x40, 0x40, 0x42];
        assert!(near(glyph_ink(&area(LIGHT_BAR, ink, 1)), ink, 2));
    }

    #[test]
    fn highlighted_button_background_is_the_background() {
        let hovered = [0x3a, 0x3a, 0x3d];
        let ink = [0xa0; 3];
        assert!(near(glyph_ink(&area(hovered, ink, 1)), ink, 2));
    }

    #[test]
    fn a_two_pixel_glyph_is_found_too() {
        assert!(near(glyph_ink(&area(DARK_BAR, GLYPH, 2)), GLYPH, 2));
    }

    #[test]
    fn a_smeared_thin_line_does_not_dilute_the_full_strength_symbol_beside_it() {
        let mut grid = area(DARK_BAR, [0x8f; 3], 2);
        for i in 0..10 {
            for (x, y) in [(30 + i, 8), (30 + i, 17), (30, 8 + i), (39, 8 + i)] {
                grid[y * WIDTH + x] = [0xf0; 3];
            }
        }
        assert!(near(glyph_ink(&grid), [0xf0; 3], 2));
    }

    #[test]
    fn no_glyph_gives_none() {
        assert_eq!(glyph_ink(&vec![DARK_BAR; WIDTH * HEIGHT]), None);
        assert_eq!(glyph_ink(&[]), None);
    }

    #[test]
    fn a_few_stray_pixels_are_no_glyph() {
        let mut grid = vec![DARK_BAR; WIDTH * HEIGHT];
        grid[5] = [255; 3];
        grid[900] = [255; 3];
        assert_eq!(glyph_ink(&grid), None);
    }

    #[test]
    fn background_noise_is_ignored() {
        let mut grid = area(DARK_BAR, GLYPH, 1);
        for (i, p) in grid.iter_mut().enumerate() {
            if *p == DARK_BAR {
                let wobble = (i % 5) as u8;
                *p = DARK_BAR.map(|c| c + wobble);
            }
        }
        assert!(near(glyph_ink(&grid), GLYPH, 3));
    }

    #[test]
    fn a_mostly_filled_area_is_no_glyph() {
        let mut grid = vec![DARK_BAR; WIDTH * HEIGHT];
        for p in grid.iter_mut().take(WIDTH * HEIGHT / 2) {
            *p = [0xf0; 3];
        }
        assert_eq!(glyph_ink(&grid), None);
    }
}
