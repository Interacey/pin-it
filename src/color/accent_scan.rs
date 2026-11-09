//! The accent of a window: the largest chromatic region of the title-bar strip that is big enough to be a
//! button or a banner, not an icon.

use super::math::median_color;

/// Pixels with less chroma (max channel minus min channel) are grey and never count.
const MIN_CHROMA: u8 = 40;
/// Pixels this close to the bar or to the symbol ink are not accent.
const NEAR_TOLERANCE: u8 = 12;
/// Colors that differ only in the low bits (anti-aliasing, dithering) share a bucket.
const BUCKET_SHIFT: u8 = 3;
/// Smallest area, in pixels at 96 dpi, a color needs to count as an accent; it grows with the dpi squared.
const MIN_AREA_PX_AT_96_DPI: u64 = 40;
/// A region counts only when its bounding box is at least this wide and this high, in dp. A tab icon (16 dp)
/// stays below it; an "Update" button (about 60 x 22 dp) reaches it.
const MIN_EXTENT_DP: u64 = 18;
const BASE_DPI: u64 = 96;

/// What the accent search needs besides the pixels.
pub(super) struct AccentContext {
    pub(super) bar: [u8; 3],
    pub(super) ink: Option<[u8; 3]>,
    pub(super) dpi: u32,
}

/// A connected region of one color bucket.
struct Region {
    key: [u8; 3],
    pixels: Vec<[u8; 3]>,
    min: (usize, usize),
    max: (usize, usize),
}

impl Region {
    fn new(key: [u8; 3], at: (usize, usize)) -> Self {
        Self {
            key,
            pixels: Vec::new(),
            min: at,
            max: at,
        }
    }

    fn add(&mut self, at: (usize, usize), px: [u8; 3]) {
        self.pixels.push(px);
        self.min = (self.min.0.min(at.0), self.min.1.min(at.1));
        self.max = (self.max.0.max(at.0), self.max.1.max(at.1));
    }

    fn qualifies(&self, dpi: u32) -> bool {
        let extent = min_extent_px(dpi);
        let (width, height) = (self.max.0 - self.min.0 + 1, self.max.1 - self.min.1 + 1);
        width as u64 >= extent
            && height as u64 >= extent
            && self.pixels.len() as u64 >= min_area(dpi)
    }

    /// Larger regions first; ties go to the more saturated color, then to the larger bucket key.
    fn rank(&self) -> (usize, u8, [u8; 3]) {
        (self.pixels.len(), chroma(self.pixels[0]), self.key)
    }
}

/// Median color of the largest chromatic region of `rows` whose bounding box is at least 18 dp wide and high,
/// or `None` when there is none. Regions are 4-connected pixels of one color bucket.
pub(super) fn dominant_accent(rows: &[Vec<[u8; 3]>], ctx: &AccentContext) -> Option<[u8; 3]> {
    let mut seen: Vec<Vec<bool>> = rows.iter().map(|row| vec![false; row.len()]).collect();
    let mut best: Option<Region> = None;
    for (y, row) in rows.iter().enumerate() {
        for (x, &px) in row.iter().enumerate() {
            if seen[y][x] || !counts_as_accent(px, ctx) {
                continue;
            }
            let region = grow_region(rows, &mut seen, (x, y), ctx);
            if region.qualifies(ctx.dpi) && best.as_ref().is_none_or(|b| region.rank() > b.rank()) {
                best = Some(region);
            }
        }
    }
    median_color(&best?.pixels)
}

/// Flood fill (4 neighbours) from `start` over the pixels that share its bucket.
fn grow_region(
    rows: &[Vec<[u8; 3]>],
    seen: &mut [Vec<bool>],
    start: (usize, usize),
    ctx: &AccentContext,
) -> Region {
    let key = bucket(rows[start.1][start.0]);
    let mut region = Region::new(key, start);
    let mut open = vec![start];
    seen[start.1][start.0] = true;
    while let Some((x, y)) = open.pop() {
        region.add((x, y), rows[y][x]);
        let neighbours = [
            (x.wrapping_sub(1), y),
            (x + 1, y),
            (x, y.wrapping_sub(1)),
            (x, y + 1),
        ];
        for (nx, ny) in neighbours {
            let Some(&px) = rows.get(ny).and_then(|row| row.get(nx)) else {
                continue;
            };
            if !seen[ny][nx] && bucket(px) == key && counts_as_accent(px, ctx) {
                seen[ny][nx] = true;
                open.push((nx, ny));
            }
        }
    }
    region
}

fn bucket(px: [u8; 3]) -> [u8; 3] {
    px.map(|c| c >> BUCKET_SHIFT)
}

fn counts_as_accent(px: [u8; 3], ctx: &AccentContext) -> bool {
    chroma(px) >= MIN_CHROMA && !near(px, ctx.bar) && !ctx.ink.is_some_and(|ink| near(px, ink))
}

fn min_area(dpi: u32) -> u64 {
    let dpi = u64::from(dpi.max(1));
    (MIN_AREA_PX_AT_96_DPI * dpi * dpi).div_ceil(BASE_DPI * BASE_DPI)
}

fn min_extent_px(dpi: u32) -> u64 {
    (MIN_EXTENT_DP * u64::from(dpi.max(1))).div_ceil(BASE_DPI)
}

fn chroma(px: [u8; 3]) -> u8 {
    px.iter().max().copied().unwrap_or(0) - px.iter().min().copied().unwrap_or(0)
}

fn near(a: [u8; 3], b: [u8; 3]) -> bool {
    a.iter()
        .zip(b)
        .all(|(&x, y)| x.abs_diff(y) <= NEAR_TOLERANCE)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DARK: [u8; 3] = [24, 25, 26];
    const BLUE: [u8; 3] = [0, 120, 215];
    const GREEN: [u8; 3] = [16, 160, 80];
    const RED: [u8; 3] = [220, 30, 30];
    /// Caption rows of the scan at 96 dpi: 70% of a 32 dp caption.
    const ROWS: usize = 22;

    /// A bar of `rows` rows with filled rectangles `(x from, x to, y from, y to, color)`.
    fn bar(
        width: usize,
        rows: usize,
        blobs: &[(usize, usize, usize, usize, [u8; 3])],
    ) -> Vec<Vec<[u8; 3]>> {
        (0..rows)
            .map(|y| {
                (0..width)
                    .map(|x| {
                        blobs
                            .iter()
                            .find(|&&(x0, x1, y0, y1, _)| {
                                (x0..x1).contains(&x) && (y0..y1).contains(&y)
                            })
                            .map_or(DARK, |&(_, _, _, _, c)| c)
                    })
                    .collect()
            })
            .collect()
    }

    fn at(dpi: u32) -> AccentContext {
        AccentContext {
            bar: DARK,
            ink: Some([230, 230, 230]),
            dpi,
        }
    }

    fn ctx() -> AccentContext {
        at(96)
    }

    #[test]
    fn a_blue_button_on_a_dark_bar_is_the_accent() {
        let rows = bar(300, ROWS, &[(100, 160, 0, ROWS, BLUE)]);
        assert_eq!(dominant_accent(&rows, &ctx()), Some(BLUE));
    }

    #[test]
    fn a_button_of_60_by_22_dp_is_found() {
        let rows = bar(300, 22, &[(100, 160, 0, 22, BLUE)]);
        assert_eq!(dominant_accent(&rows, &ctx()), Some(BLUE));
    }

    #[test]
    fn a_favicon_sized_blob_is_ignored() {
        let rows = bar(300, ROWS, &[(100, 116, 3, 19, RED)]);
        assert_eq!(dominant_accent(&rows, &ctx()), None);
        let rows = bar(300, ROWS, &[(100, 117, 3, 20, RED)]);
        assert_eq!(
            dominant_accent(&rows, &ctx()),
            None,
            "17 x 17 is still an icon"
        );
    }

    #[test]
    fn a_region_of_exactly_18_by_18_counts() {
        let rows = bar(300, ROWS, &[(100, 118, 2, 20, RED)]);
        assert_eq!(dominant_accent(&rows, &ctx()), Some(RED));
    }

    #[test]
    fn a_wide_but_low_or_tall_but_narrow_region_is_ignored() {
        let low = bar(300, ROWS, &[(50, 250, 5, 15, BLUE)]);
        assert_eq!(dominant_accent(&low, &ctx()), None);
        let narrow = bar(300, ROWS, &[(50, 60, 0, ROWS, BLUE)]);
        assert_eq!(dominant_accent(&narrow, &ctx()), None);
    }

    #[test]
    fn a_thin_colored_line_is_ignored() {
        for thickness in [1, 2] {
            let rows = bar(300, ROWS, &[(20, 280, 10, 10 + thickness, BLUE)]);
            assert_eq!(dominant_accent(&rows, &ctx()), None, "{thickness} px line");
        }
        let vertical = bar(300, ROWS, &[(100, 102, 0, ROWS, BLUE)]);
        assert_eq!(dominant_accent(&vertical, &ctx()), None);
    }

    #[test]
    fn a_favicon_next_to_a_button_leaves_the_button_the_accent() {
        let rows = bar(
            400,
            ROWS,
            &[(20, 36, 3, 19, RED), (200, 260, 0, ROWS, BLUE)],
        );
        assert_eq!(dominant_accent(&rows, &ctx()), Some(BLUE));
    }

    #[test]
    fn of_two_qualifying_blobs_the_larger_wins() {
        let rows = bar(
            400,
            ROWS,
            &[(20, 60, 0, ROWS, GREEN), (100, 180, 0, ROWS, BLUE)],
        );
        assert_eq!(dominant_accent(&rows, &ctx()), Some(BLUE));
        let rows = bar(
            400,
            ROWS,
            &[(20, 100, 0, ROWS, GREEN), (200, 240, 0, ROWS, BLUE)],
        );
        assert_eq!(dominant_accent(&rows, &ctx()), Some(GREEN));
    }

    #[test]
    fn two_small_blobs_of_one_color_do_not_add_up() {
        let rows = bar(400, ROWS, &[(20, 36, 3, 19, RED), (100, 116, 3, 19, RED)]);
        assert_eq!(dominant_accent(&rows, &ctx()), None);
    }

    #[test]
    fn text_inside_a_button_does_not_split_it() {
        let mut rows = bar(300, ROWS, &[(100, 160, 0, ROWS, BLUE)]);
        for row in &mut rows[8..14] {
            for px in &mut row[110..150] {
                *px = [255, 255, 255];
            }
        }
        assert_eq!(dominant_accent(&rows, &ctx()), Some(BLUE));
    }

    #[test]
    fn the_extent_limit_scales_with_the_dpi() {
        // 18 dp is 18, 27 and 36 px at 96, 144 and 192 dpi.
        for (dpi, extent) in [(96u32, 18usize), (144, 27), (192, 36)] {
            let rows_px = extent + 6;
            let big = bar(400, rows_px, &[(50, 50 + extent, 3, 3 + extent, BLUE)]);
            assert_eq!(
                dominant_accent(&big, &at(dpi)),
                Some(BLUE),
                "{dpi} dpi, {extent} px"
            );
            let small = bar(400, rows_px, &[(50, 50 + extent - 1, 3, 2 + extent, BLUE)]);
            assert_eq!(
                dominant_accent(&small, &at(dpi)),
                None,
                "{dpi} dpi, {} px",
                extent - 1
            );
        }
        // The same 24 px blob is an icon at 144 dpi (16 dp) and a button at 96 dpi (24 dp).
        let blob = bar(300, 30, &[(50, 74, 3, 27, BLUE)]);
        assert_eq!(dominant_accent(&blob, &at(96)), Some(BLUE));
        assert_eq!(dominant_accent(&blob, &at(144)), None);
    }

    #[test]
    fn a_neutral_bar_has_no_accent() {
        let rows = bar(300, ROWS, &[(100, 160, 0, ROWS, [120, 120, 124])]);
        assert_eq!(dominant_accent(&rows, &ctx()), None);
    }

    #[test]
    fn the_bar_color_itself_is_never_the_accent() {
        let themed = [200, 60, 90];
        let rows: Vec<Vec<[u8; 3]>> = vec![vec![themed; 300]; ROWS];
        let ctx = AccentContext {
            bar: themed,
            ..ctx()
        };
        assert_eq!(dominant_accent(&rows, &ctx), None);
    }

    #[test]
    fn a_few_noise_pixels_are_ignored() {
        let rows = bar(300, ROWS, &[(100, 103, 0, 3, BLUE)]);
        assert_eq!(dominant_accent(&rows, &ctx()), None);
    }

    #[test]
    fn caption_ink_pixels_do_not_count() {
        let ink = [200, 150, 120];
        let rows = bar(300, ROWS, &[(100, 160, 0, ROWS, ink)]);
        let ctx = AccentContext {
            ink: Some(ink),
            ..ctx()
        };
        assert_eq!(dominant_accent(&rows, &ctx), None);
    }

    #[test]
    fn ragged_or_empty_rows_do_not_panic() {
        assert_eq!(dominant_accent(&[], &ctx()), None);
        assert_eq!(
            dominant_accent(&[vec![], vec![BLUE; 3], vec![]], &ctx()),
            None
        );
        // A dpi of 0 counts as 1: tiny limits, but no division by zero.
        assert!(dominant_accent(&[vec![BLUE; 40]], &at(0)).is_some());
        assert_eq!(dominant_accent(&[vec![BLUE; 4]], &at(u32::MAX)), None);
    }
}
