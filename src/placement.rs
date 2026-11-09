//! Where the pin button goes: spacing constants on a 4 dp grid and the choice of a free spot in the title bar.
//! Everything here is pure; the pixel scan comes from `color`, the default slot from `windows_filter`.

use std::time::Duration;

use self::group::{GroupSearch, MAX_GAP_DP, group_left};
pub(crate) use self::maximized::TopBorder;
use crate::color::dominant_cluster;
use crate::win32::BASE_DPI;
use crate::windows_filter::{CAPTION_BUTTON_COUNT, FALLBACK_BUTTON_WIDTH_DP, PinSlot};

// ----- constants -----
// Spacing is in logical pixels ("dp"); physical pixels = dp * dpi / 96, rounded.

/// All sizes and gaps are multiples of this.
const GRID_DP: i32 = 4;
/// Gap between the pin and the caption-button group.
pub(crate) const GAP_TO_CAPTION_BUTTONS_DP: i32 = 4;
/// Free space kept between the pin and neighbouring title-bar content.
pub(crate) const CONTENT_MARGIN_DP: i32 = 4;
/// Free space kept above and below the pin inside the caption row. Smaller than the horizontal
/// margin so that a title bar of 30 to 32 dp still leaves room for sizes above the automatic one.
const CAPTION_VERTICAL_MARGIN_DP: i32 = 2;
/// Automatic button size range, chosen from the caption height.
const AUTO_MIN_SIZE_DP: i32 = 16;
const AUTO_MAX_SIZE_DP: i32 = 32;
/// Button size range after the user's scale percent is applied.
const USER_MIN_SIZE_DP: i32 = 12;
const USER_MAX_SIZE_DP: i32 = 40;
/// The automatic size is 7/10 of the caption height, rounded to the grid.
const SIZE_NUMERATOR: i32 = 7;
const SIZE_DENOMINATOR: i32 = 10;
const PERCENT_DENOMINATOR: i64 = 100;

/// Per-channel difference up to which a pixel still counts as title-bar background.
pub(crate) const BACKGROUND_TOLERANCE: u8 = 12;
/// Per-channel difference up to which the tracked strip color follows a pixel (a gradient changes by one or two per column).
const TRACK_FOLLOW_STEP: u8 = 3;
/// How far from the strip color the shaded right end of a strip may be, per channel.
const TRACK_SEED_RANGE: u8 = 48;
/// The strip color is read from the last 1/4 of each row.
const SEED_FRACTION_DENOMINATOR: usize = 4;
/// Columns at each end that may hold the window frame or its shadow instead of title-bar content.
const FRAME_MARGIN_DP: i32 = 8;
/// How long a custom-title-bar window waits for a scan of the painted window. The default slot
/// of such a window is an estimate (in Firefox it lies on the list-all-tabs button), so it is never shown;
/// when the wait is over without a usable scan the button stays hidden.
const SCAN_WAIT: Duration = Duration::from_secs(3);

// ----- sizes -----

/// Physical pixels for `dp` logical pixels.
pub(crate) fn scale_dp(dp: i32, dpi: u32) -> i32 {
    let dpi = i64::from(dpi.max(1));
    ((i64::from(dp) * dpi + i64::from(BASE_DPI) / 2) / i64::from(BASE_DPI)) as i32
}

/// Logical pixels for `px` physical pixels.
fn to_dp(px: i32, dpi: u32) -> i32 {
    let dpi = i64::from(dpi.max(1));
    ((i64::from(px) * i64::from(BASE_DPI) + dpi / 2) / dpi) as i32
}

fn round_to_grid(dp: i32) -> i32 {
    (dp + GRID_DP / 2).div_euclid(GRID_DP) * GRID_DP
}

/// Automatic button size in dp for a caption row of `caption_dp`: 0.7 x caption height on the 4 dp grid, 16..32.
/// Caption 24 -> 16, 28 -> 20, 30 -> 20, 32 -> 24, 40 -> 28, 48 and more -> 32.
fn default_size_dp(caption_dp: i32) -> i32 {
    let target = (caption_dp * SIZE_NUMERATOR + SIZE_DENOMINATOR / 2) / SIZE_DENOMINATOR;
    round_to_grid(target).clamp(AUTO_MIN_SIZE_DP, AUTO_MAX_SIZE_DP)
}

/// Button size in dp after the user's scale `percent`, on the grid, 12..40.
fn scaled_size_dp(base_dp: i32, percent: u32) -> i32 {
    let scaled =
        (i64::from(base_dp) * i64::from(percent) + PERCENT_DENOMINATOR / 2) / PERCENT_DENOMINATOR;
    round_to_grid(scaled as i32).clamp(USER_MIN_SIZE_DP, USER_MAX_SIZE_DP)
}

/// Pin button size in physical pixels for a caption row of `caption_px`.
pub(crate) fn default_size_px(caption_px: i32, dpi: u32) -> i32 {
    scale_dp(default_size_dp(to_dp(caption_px, dpi)), dpi)
}

fn round_down_to_grid(dp: i32) -> i32 {
    dp.div_euclid(GRID_DP) * GRID_DP
}

/// Slot with side `size`, keeping the right edge and the vertical center.
fn resized(slot: PinSlot, size: i32) -> PinSlot {
    PinSlot {
        x: slot.x + slot.size - size,
        y: slot.y + (slot.size - size) / 2,
        size,
    }
}

/// Slot with its size changed to the user's scale `percent` (grid snapped), keeping the right edge and the vertical center.
pub(crate) fn apply_user_scale(slot: PinSlot, percent: u32, dpi: u32) -> PinSlot {
    let size = scale_dp(scaled_size_dp(to_dp(slot.size, dpi), percent), dpi).max(1);
    resized(slot, size)
}

/// Sizes to try for a slot of `size_px`: the size itself, then every smaller grid step down to the user minimum.
fn candidate_sizes(size_px: i32, dpi: u32) -> impl Iterator<Item = i32> {
    let smaller = (USER_MIN_SIZE_DP..to_dp(size_px, dpi))
        .rev()
        .filter(|dp| dp % GRID_DP == 0)
        .map(move |dp| scale_dp(dp, dpi));
    std::iter::once(size_px).chain(smaller)
}

// ----- placement input -----

/// Everything `decide` needs to know about one window, besides its scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PlacementInput {
    /// Default slot after the user's scale; the right edge and the vertical center stay when the size shrinks.
    pub(crate) slot: PinSlot,
    /// True when the slot is an estimate (custom title bar): such a window waits for its scan.
    pub(crate) estimated: bool,
    pub(crate) dpi: u32,
    /// Window origin and size in screen pixels; scan columns are relative to the origin.
    pub(crate) window_origin: (i32, i32),
    pub(crate) window_size: (i32, i32),
    /// Caption height to assume until a scan reports the real one.
    pub(crate) caption_hint_px: i32,
    /// Window-relative column where the caption buttons visibly end; 0 when unknown.
    pub(crate) group_right: i32,
    /// Time since the button was created; an estimated slot waits for its scan this long.
    pub(crate) waited: Duration,
}

impl PlacementInput {
    /// Free space kept around the button, in pixels.
    fn margin_px(&self) -> i32 {
        scale_dp(CONTENT_MARGIN_DP, self.dpi)
    }

    /// The caption height the scan measured, or the hint while there is no scan.
    fn caption_px(&self, scan: ScanState) -> i32 {
        match scan {
            ScanState::Ready(s) if s.caption_px > 0 => s.caption_px,
            _ => self.caption_hint_px,
        }
    }

    /// The slot reduced on the grid so that it leaves the margin above and below it in a caption of
    /// `caption_px`; `None` when not even the user minimum fits.
    fn fitted_to_caption(&self, caption_px: i32) -> Option<PinSlot> {
        let size = fit_size_to_caption(self.slot.size, caption_px, self.dpi)?;
        Some(resized(self.slot, size))
    }
}

/// `size_px` limited so that the button keeps `CAPTION_VERTICAL_MARGIN_DP` above and below it in a
/// caption row of `caption_px`: a size that fits is kept, a larger one is rounded down on the grid.
/// `None` when not even the user minimum fits.
pub(crate) fn fit_size_to_caption(size_px: i32, caption_px: i32, dpi: u32) -> Option<i32> {
    let room_dp = to_dp(
        caption_px - 2 * scale_dp(CAPTION_VERTICAL_MARGIN_DP, dpi),
        dpi,
    );
    if to_dp(size_px, dpi) <= room_dp {
        return Some(size_px);
    }
    let size_dp = round_down_to_grid(room_dp);
    (size_dp >= USER_MIN_SIZE_DP).then(|| scale_dp(size_dp, dpi))
}

/// Caption height of a window whose unscaled default button is `default_size_px`, inverted from the size rule.
/// Never smaller than what the default button needs, so the hint does not shrink a button that fits.
pub(crate) fn caption_hint_px(default_size_px: i32, dpi: u32) -> i32 {
    let size_dp = to_dp(default_size_px, dpi);
    let from_rule = (size_dp * SIZE_DENOMINATOR + SIZE_NUMERATOR / 2) / SIZE_NUMERATOR;
    scale_dp(from_rule.max(size_dp + 2 * CAPTION_VERTICAL_MARGIN_DP), dpi)
}

// ----- scan and chooser -----

/// Sampled rows of the caption area of one window; columns are window-relative physical pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TitleBarScan {
    /// Window size at scan time, to notice resizes.
    pub(crate) window_size: (i32, i32),
    /// Height of the caption row in pixels, to keep the button inside it.
    pub(crate) caption_px: i32,
    /// Dominant color of the sampled pixels.
    pub(crate) background: [u8; 3],
    /// Color of the symbols of the window's caption buttons (read from the minimize and maximize buttons); None when none was found.
    pub(crate) caption_ink: Option<[u8; 3]>,
    /// Most prominent chromatic color of the strip (an "Update" button, a colored tab); None for a neutral strip.
    pub(crate) accent: Option<[u8; 3]>,
    /// Sampled rows, each as wide as the window.
    pub(crate) rows: Vec<Vec<[u8; 3]>>,
}

impl TitleBarScan {
    fn width(&self) -> usize {
        self.rows.iter().map(Vec::len).min().unwrap_or(0)
    }
}

fn near(a: [u8; 3], b: [u8; 3], tolerance: u8) -> bool {
    a.iter().zip(b).all(|(&x, y)| x.abs_diff(y) <= tolerance)
}

/// `true` per column when every sampled row of that column is background: the strip color of the whole scan, or
/// the local color of a strip that shades from one end to the other (`tracked_free_columns`).
fn free_columns(scan: &TitleBarScan, tolerance: u8) -> Vec<bool> {
    let tracked = tracked_free_columns(scan, tolerance);
    (0..scan.width())
        .map(|x| {
            tracked[x]
                || scan
                    .rows
                    .iter()
                    .all(|row| near(row[x], scan.background, tolerance))
        })
        .collect()
}

/// Columns whose pixels are near the strip color as it runs along each row, read from the right end (the
/// caption buttons sit on the strip, so the right end shows its color) to the left. The color follows a smooth
/// change (Firefox shades its strip from one color to another) and is held across anything that changes it
/// abruptly (a tab, a symbol, a highlight), so a shaded strip is background all along and a tab fill is not.
fn tracked_free_columns(scan: &TitleBarScan, tolerance: u8) -> Vec<bool> {
    let width = scan.width();
    let mut tracks = seed_tracks(scan, tolerance);
    let mut free = vec![false; width];
    for x in (0..width).rev() {
        let on_track = scan
            .rows
            .iter()
            .zip(&tracks)
            .all(|(row, &track)| near(row[x], track, tolerance));
        if on_track {
            free[x] = true;
            for (row, track) in scan.rows.iter().zip(&mut tracks) {
                if near(row[x], *track, TRACK_FOLLOW_STEP) {
                    *track = row[x];
                }
            }
        }
    }
    free
}

/// The strip color of each row at the right end: the dominant color of the last quarter of the row, when it is
/// a shade of the strip color (within `TRACK_SEED_RANGE`); else the strip color itself, so that a row that is
/// content at its right end is never taken for background.
fn seed_tracks(scan: &TitleBarScan, tolerance: u8) -> Vec<[u8; 3]> {
    let from = scan.width() - scan.width() / SEED_FRACTION_DENOMINATOR;
    scan.rows
        .iter()
        .map(|row| {
            dominant_cluster(row[from..scan.width()].iter().copied(), tolerance)
                .filter(|&seed| near(seed, scan.background, TRACK_SEED_RANGE))
                .unwrap_or(scan.background)
        })
        .collect()
}

/// Half-open ranges of consecutive `true` values.
fn free_runs(free: &[bool]) -> Vec<(usize, usize)> {
    let mut runs = Vec::new();
    let mut start = None;
    for (x, &is_free) in free.iter().enumerate() {
        match (is_free, start) {
            (true, None) => start = Some(x),
            (false, Some(s)) => {
                runs.push((s, x));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        runs.push((s, free.len()));
    }
    runs
}

/// Free columns of a scan made at another window width, moved so that the right edges line up. The
/// caption buttons and what sits next to them stay where they are, while the tab strip squeezes from the
/// left, so the right end is the part of the old scan that is still true. Missing columns count as free.
fn aligned_to_right(mut free: Vec<bool>, width: usize) -> Vec<bool> {
    if free.len() >= width {
        free.drain(..free.len() - width);
        free
    } else {
        let mut grown = vec![true; width - free.len()];
        grown.append(&mut free);
        grown
    }
}

/// What a pin needs from a run of free columns, in columns.
struct Fit {
    /// First column that belongs to the caption buttons; a run ending here needs no margin on that side.
    limit: usize,
    margin: usize,
    size: usize,
}

impl Fit {
    /// End column of the pin placed at the right end of `run`, if the run is wide enough.
    fn end_in_run(&self, (start, end): (usize, usize)) -> Option<usize> {
        let usable_start = start + self.margin;
        let usable_end = if end == self.limit {
            end
        } else {
            end.saturating_sub(self.margin)
        };
        (usable_end >= usable_start + self.size).then_some(usable_end)
    }
}

/// The free slot closest to the caption buttons, or `None` when no free run of title-bar background is wide enough.
/// A button that does not fit at `input.slot.size` is tried again one grid step smaller, down to the user minimum.
/// Only the horizontal position changes; the vertical center and right edge of the slot are kept, so it stays in the caption row.
/// Columns at and right of the slot's right edge belong to the caption buttons and are never used.
/// A run needs `size + 2 * margin` pixels, except that a run ending at the slot needs no margin on that side (the gap to the buttons is there).
/// For an estimated slot the caption-button group is read from the columns (`group_left`) and the pin ends
/// `GAP_TO_CAPTION_BUTTONS_DP` left of it; the estimate is only the place to start looking.
fn choose_in(free: &[bool], input: &PlacementInput) -> Option<PinSlot> {
    let limit = pin_limit(free, input);
    let runs = free_runs(&free[..limit]);
    let margin = input.margin_px().max(0) as usize;
    candidate_sizes(input.slot.size, input.dpi).find_map(|size| {
        let fit = Fit {
            limit,
            margin,
            size: size.max(1) as usize,
        };
        let end = runs.iter().rev().find_map(|&run| fit.end_in_run(run))?;
        Some(PinSlot {
            x: input.window_origin.0 + (end - fit.size) as i32,
            ..resized(input.slot, size)
        })
    })
}

/// The column where the pin's right edge may be at most: the left edge of the caption-button group minus the gap.
/// The group edge is read from the columns (`group_left`) starting at the visible right edge of the window.
/// A native slot comes from DWM, whose group bounds can be wider than the drawn buttons (8 px at 96 dpi on
/// Windows 11), so the columns only replace it when they agree within one button; otherwise the DWM edge stands.
fn pin_limit(free: &[bool], input: &PlacementInput) -> usize {
    let estimate = estimated_right_edge(free, input);
    let gap = scale_dp(GAP_TO_CAPTION_BUTTONS_DP, input.dpi).max(0) as usize;
    let button = button_width_px(input);
    let slot_edge = estimate + gap;
    let Some(visible_right) = visible_right(input, slot_edge) else {
        return estimate;
    };
    let search = GroupSearch {
        right: visible_right.min(free.len()),
        button,
        buttons: assumed_buttons(input, visible_right.saturating_sub(slot_edge), button),
        max_gap: scale_dp(MAX_GAP_DP, input.dpi).max(0) as usize,
    };
    let group = group_left(free, &search);
    if input.estimated || group.abs_diff(slot_edge) <= button {
        group.saturating_sub(gap).min(free.len())
    } else {
        estimate
    }
}

/// The right edge of the slot in scan columns.
fn estimated_right_edge(free: &[bool], input: &PlacementInput) -> usize {
    let right_edge = input.slot.x + input.slot.size - input.window_origin.0;
    right_edge.clamp(0, free.len() as i32) as usize
}

fn button_width_px(input: &PlacementInput) -> usize {
    scale_dp(FALLBACK_BUTTON_WIDTH_DP, input.dpi).max(1) as usize
}

/// The column where the caption buttons visibly end: the measured one, else for an estimated slot the end of
/// three standard buttons behind it; `None` for a native slot without a measurement.
fn visible_right(input: &PlacementInput, slot_edge: usize) -> Option<usize> {
    if input.group_right > 0 {
        Some(input.group_right as usize)
    } else if input.estimated {
        Some(slot_edge + CAPTION_BUTTON_COUNT as usize * button_width_px(input))
    } else {
        None
    }
}

/// Whether the columns look like a painted custom title bar: background in the span of the three standard
/// buttons at the right edge, and some content between the window frames. A window that is not painted yet shows
/// nothing but its frame; one captured half way shows content in place of the background; in both cases its free
/// runs say nothing about the real layout.
fn looks_painted(free: &[bool], input: &PlacementInput) -> bool {
    let gap = scale_dp(GAP_TO_CAPTION_BUTTONS_DP, input.dpi).max(0) as usize;
    let slot_edge = estimated_right_edge(free, input) + gap;
    let Some(right) = visible_right(input, slot_edge).map(|r| r.min(free.len())) else {
        return true;
    };
    let span_left = right.saturating_sub(CAPTION_BUTTON_COUNT as usize * button_width_px(input));
    let frame = scale_dp(FRAME_MARGIN_DP, input.dpi).max(0) as usize;
    let inner = &free[frame.min(right)..right.saturating_sub(frame).max(frame.min(right))];
    free[span_left..right].contains(&true) && inner.contains(&false)
}

/// How many caption buttons the group is assumed to have: three for an estimated slot, else what the width
/// between the slot's group edge and the visible right edge holds.
fn assumed_buttons(input: &PlacementInput, group_width: usize, button: usize) -> usize {
    if input.estimated {
        return CAPTION_BUTTON_COUNT as usize;
    }
    ((group_width + button / 2) / button).clamp(1, CAPTION_BUTTON_COUNT as usize)
}

// ----- decision -----

/// What the cache knows about a window's scan.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ScanState<'a> {
    /// Requested, nothing back yet.
    Pending,
    /// The capture failed; there is nothing to scan.
    Unavailable,
    Ready(&'a TitleBarScan),
}

/// What to do with the button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Placement {
    /// Show the button in this slot.
    Show(PinSlot),
    /// No room for it (title bar full or caption too low): hide the button. A pinned window stays pinned.
    Hide,
    /// Keep the button hidden until a usable scan arrives (custom title bar).
    Wait,
    /// A custom title bar whose scan did not arrive within `SCAN_WAIT`: the button stays hidden.
    NoScan,
}

/// Decides the button placement from the window and the scan state.
/// The size is first limited to the caption height (see `PlacementInput::fitted_to_caption`).
/// A window with a native title bar uses the default slot until a scan says otherwise. A window with an
/// estimated slot (custom title bar) is never shown at that estimate: it waits up to `SCAN_WAIT` for a scan
/// of the painted window, then places the button from the scan, or stays hidden without one.
/// A scan of another window size is stale: the old scan, lined up at the right edge, places the button until
/// the new scan arrives, and when even that finds no room the default slot next to the caption buttons is
/// used. Only a scan of the current size can hide the button.
pub(crate) fn decide(input: &PlacementInput, scan: ScanState) -> Placement {
    let Some(slot) = input.fitted_to_caption(input.caption_px(scan)) else {
        return Placement::Hide;
    };
    let fitted = PlacementInput { slot, ..*input };
    match scan {
        ScanState::Ready(s) if s.window_size != input.window_size => {
            let width = input.window_size.0.max(0) as usize;
            let free = aligned_to_right(free_columns(s, BACKGROUND_TOLERANCE), width);
            Placement::Show(choose_in(&free, &fitted).unwrap_or(slot))
        }
        ScanState::Ready(s) => place_from_scan(s, &fitted),
        ScanState::Pending | ScanState::Unavailable if input.estimated => no_scan_yet(input),
        ScanState::Pending | ScanState::Unavailable => Placement::Show(slot),
    }
}

/// An estimated window has no scan: wait for it, and give up after `SCAN_WAIT`.
fn no_scan_yet(input: &PlacementInput) -> Placement {
    if input.waited < SCAN_WAIT {
        Placement::Wait
    } else {
        Placement::NoScan
    }
}

/// The slot from a scan of the current size. An estimated window whose scan does not look painted waits for a
/// better one until `SCAN_WAIT` is over; after that the scan is taken as it is.
fn place_from_scan(scan: &TitleBarScan, input: &PlacementInput) -> Placement {
    let free = free_columns(scan, BACKGROUND_TOLERANCE);
    if input.estimated && !looks_painted(&free, input) && input.waited < SCAN_WAIT {
        return Placement::Wait;
    }
    choose_in(&free, input).map_or(Placement::Hide, Placement::Show)
}

#[cfg(test)]
pub(crate) mod firefox_strip;
#[cfg(test)]
mod firefox_tests;
mod group;
pub(crate) mod maximized;
#[cfg(test)]
pub(crate) mod real_windows;
#[cfg(test)]
mod tests;
