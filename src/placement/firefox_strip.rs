//! A synthetic caption strip modelled on the one measured in Firefox (96 dpi): tabs with an icon, a title, a
//! close glyph and a faint separator, the new-tab button, free drag space, the list-all-tabs chevron (with an
//! optional hover highlight) and three caption buttons that show only a symbol.

use std::sync::Arc;

use super::{BACKGROUND_TOLERANCE, TitleBarScan};
use crate::color::{TitleSample, dominant_cluster};

const CAPTION_PX: i32 = 32;
const SAMPLED_ROWS: usize = 7;
const BUTTON_PX: usize = 46;
const BUTTON_COUNT: usize = 3;
const SYMBOL_PX: usize = 10;
const TAB_START: usize = 8;
const TAB_WIDTH: usize = 140;
const ICON: (usize, usize) = (8, 24);
const TITLE_START: usize = 30;
/// Columns from the right edge of a tab to the end of its title, the close glyph and the separator column.
const TITLE_FROM_END: usize = 50;
const CLOSE_FROM_END: (usize, usize) = (24, 10);
const SEPARATOR_FROM_END: usize = 1;
/// Free columns the strip keeps between the new-tab button and the chevron when nothing says otherwise.
const MIN_FREE_BEFORE_CHEVRON: usize = 40;
const PLUS_PX: usize = 14;
/// The rows of the sampled strip that show text.
const TEXT_ROWS: std::ops::Range<usize> = 2..5;
const TEXT_INK_PX: usize = 2;
const TEXT_GAP_PX: usize = 3;
const CHEVRON_PX: usize = 18;
/// Free columns between the chevron and the first caption button.
const CHEVRON_TO_BUTTONS_PX: usize = 4;
const HOVER_PAD_PX: usize = 8;
/// Largest deviation of a gradient background from its middle colour; below the placement's tolerance.
const GRADIENT_SPREAD: u8 = 10;
/// How much fainter than the symbols the tab separator is (still above the placement's tolerance).
const SEPARATOR_STEP: u8 = 15;
const HOVER_STEP: u8 = 25;

/// Firefox's shaded theme (measured): a flat purple left part, the active tab filled with a darker flat color,
/// and right of it a strip that shades from purple towards red.
const COLORWAY_LEFT: [u8; 3] = [50, 43, 68];
const COLORWAY_TAB: [u8; 3] = [23, 21, 25];
const COLORWAY_SHADE_FROM: [u8; 3] = [59, 42, 55];
const COLORWAY_SHADE_TO: [u8; 3] = [72, 35, 32];
/// The active tab spans this share of the width, in percent.
const COLORWAY_TAB_PERCENT: (usize, usize) = (47, 64);

#[derive(Clone, Copy)]
pub(crate) struct FirefoxStrip {
    pub(crate) width: usize,
    /// Window-relative column where the caption buttons end.
    pub(crate) group_right: usize,
    pub(crate) dark: bool,
    pub(crate) tabs: usize,
    pub(crate) hover_chevron: bool,
    pub(crate) gradient: bool,
    /// The shaded theme with a filled active tab; `dark` and `gradient` do not apply.
    pub(crate) colorway: bool,
    /// Free columns between the last tab or new-tab button and the chevron's left edge; `None` for whatever the
    /// tabs leave.
    pub(crate) free_before_chevron: Option<usize>,
    /// Window height, as the scan reports it next to the width.
    pub(crate) height: i32,
}

impl FirefoxStrip {
    pub(crate) fn new(width: usize, dark: bool, tabs: usize) -> Self {
        Self {
            width,
            group_right: width - 8,
            dark,
            tabs,
            hover_chevron: false,
            gradient: false,
            colorway: false,
            free_before_chevron: None,
            height: CAPTION_PX,
        }
    }

    fn background(&self) -> [u8; 3] {
        if self.dark {
            [11, 10, 12]
        } else {
            [240, 240, 244]
        }
    }

    fn ink(&self) -> [u8; 3] {
        if self.dark {
            [121, 120, 124]
        } else {
            [91, 91, 102]
        }
    }

    fn step(&self, color: [u8; 3], by: u8) -> [u8; 3] {
        color.map(|c| if self.dark { c + by } else { c - by })
    }

    /// First column of the three caption buttons.
    pub(crate) fn span_left(&self) -> usize {
        self.group_right - BUTTON_COUNT * BUTTON_PX
    }

    /// Left edge of the chevron's glyph.
    pub(crate) fn chevron_left(&self) -> usize {
        self.span_left() - CHEVRON_TO_BUTTONS_PX - CHEVRON_PX
    }

    /// First column of whatever sits next to the caption buttons: the hover highlight, else the glyph.
    pub(crate) fn content_left(&self) -> usize {
        if self.hover_chevron {
            self.chevron_left() - HOVER_PAD_PX
        } else {
            self.chevron_left()
        }
    }

    /// Column after the new-tab button.
    fn plus_end(&self) -> usize {
        match self.free_before_chevron {
            Some(free) => self.content_left() - free,
            None => (TAB_START + self.tabs * TAB_WIDTH + PLUS_PX)
                .min(self.content_left() - MIN_FREE_BEFORE_CHEVRON),
        }
    }

    /// Paints the tabs side by side, stretched to fill the space left of the new-tab button.
    fn paint_tabs(&self, row: &mut [[u8; 3]], text: bool) {
        let area = self.plus_end() - PLUS_PX - TAB_START;
        let count = self.tabs.min(area / TAB_WIDTH).max(1);
        let width = area / count;
        let ink = self.ink();
        let separator = self.step(self.background(), SEPARATOR_STEP);
        for tab in 0..count {
            let (start, end) = (TAB_START + tab * width, TAB_START + (tab + 1) * width);
            fill(row, start + ICON.0, start + ICON.1, ink);
            if text {
                fill_sparse(row, start + TITLE_START, end - TITLE_FROM_END, ink);
            }
            fill(row, end - CLOSE_FROM_END.0, end - CLOSE_FROM_END.1, ink);
            fill(row, end - SEPARATOR_FROM_END, end, separator);
        }
    }

    fn paint(&self, row: &mut [[u8; 3]], text: bool) {
        let ink = self.ink();
        self.paint_tabs(row, text);
        fill(row, self.plus_end() - PLUS_PX, self.plus_end(), ink);
        if self.hover_chevron {
            let highlight = self.step(self.background(), HOVER_STEP);
            fill(row, self.content_left(), self.span_left(), highlight);
        }
        fill(
            row,
            self.chevron_left(),
            self.chevron_left() + CHEVRON_PX,
            ink,
        );
        for button in 0..BUTTON_COUNT {
            let middle = self.group_right - BUTTON_PX / 2 - button * BUTTON_PX;
            fill(row, middle - SYMBOL_PX / 2, middle + SYMBOL_PX / 2, ink);
        }
    }

    fn base_row(&self) -> Vec<[u8; 3]> {
        let middle = self.background();
        (0..self.width)
            .map(|x| {
                if self.colorway {
                    self.colorway_pixel(x)
                } else if self.gradient {
                    let spread = usize::from(GRADIENT_SPREAD);
                    let shift = (x * 2 * spread / self.width.max(1)) as i32 - spread as i32;
                    middle.map(|c| (i32::from(c) + shift).clamp(0, 255) as u8)
                } else {
                    middle
                }
            })
            .collect()
    }

    /// The shaded theme at column `x`.
    fn colorway_pixel(&self, x: usize) -> [u8; 3] {
        let (tab_start, tab_end) = (
            self.width * COLORWAY_TAB_PERCENT.0 / 100,
            self.width * COLORWAY_TAB_PERCENT.1 / 100,
        );
        if x < tab_start {
            COLORWAY_LEFT
        } else if x < tab_end {
            COLORWAY_TAB
        } else {
            let (done, span) = (x - tab_end, (self.width - tab_end).max(1));
            std::array::from_fn(|c| {
                let (from, to) = (
                    i32::from(COLORWAY_SHADE_FROM[c]),
                    i32::from(COLORWAY_SHADE_TO[c]),
                );
                (from + (to - from) * done as i32 / span as i32) as u8
            })
        }
    }

    /// The sampled rows: the tab titles (text) show only in the middle rows, like real text in a caption.
    fn painted_rows(&self) -> Vec<Vec<[u8; 3]>> {
        let painted = |text: bool| {
            let mut row = self.base_row();
            self.paint(&mut row, text);
            row
        };
        let (plain, texted) = (painted(false), painted(true));
        (0..SAMPLED_ROWS)
            .map(|r| {
                if TEXT_ROWS.contains(&r) {
                    texted.clone()
                } else {
                    plain.clone()
                }
            })
            .collect()
    }

    pub(crate) fn scan(&self) -> TitleBarScan {
        let rows = self.painted_rows();
        let background = if self.colorway {
            // The product takes the densest color cluster; here a strip has no single flat color.
            dominant_cluster(rows.iter().flatten().copied(), BACKGROUND_TOLERANCE)
                .unwrap_or(COLORWAY_LEFT)
        } else {
            self.background()
        };
        TitleBarScan {
            window_size: (self.width as i32, self.height),
            caption_px: CAPTION_PX,
            background,
            caption_ink: Some(self.ink()),
            accent: None,
            rows,
        }
    }

    /// The same window before it painted its caption: background only.
    pub(crate) fn unpainted_scan(&self) -> TitleBarScan {
        TitleBarScan {
            rows: vec![self.base_row(); SAMPLED_ROWS],
            ..self.scan()
        }
    }

    /// A capture that caught the strip in another color than the one that dominates it afterwards, so the right
    /// half reads as content (measured at the start of Firefox).
    pub(crate) fn half_painted_scan(&self) -> TitleBarScan {
        let mut scan = self.scan();
        let other = self.step(self.background(), HOVER_STEP * 2);
        for row in &mut scan.rows {
            fill(row, self.width / 4, self.width, other);
        }
        scan
    }

    pub(crate) fn sample(scan: TitleBarScan) -> TitleSample {
        TitleSample {
            color: Some(scan.background),
            scan: Some(Arc::new(scan)),
        }
    }
}

fn fill(row: &mut [[u8; 3]], from: usize, to: usize, color: [u8; 3]) {
    row[from..to].fill(color);
}

/// Text: `TEXT_INK_PX` inked columns, then `TEXT_GAP_PX` free ones, so that a title holds no free run a pin fits in
/// and covers less of the strip than a solid block.
fn fill_sparse(row: &mut [[u8; 3]], from: usize, to: usize, color: [u8; 3]) {
    for (i, pixel) in row[from..to].iter_mut().enumerate() {
        if i % (TEXT_INK_PX + TEXT_GAP_PX) < TEXT_INK_PX {
            *pixel = color;
        }
    }
}
