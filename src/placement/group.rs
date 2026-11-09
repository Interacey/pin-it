//! The caption-button group of a window with a custom title bar, read from the scan instead of assumed.
//!
//! Chromium, Electron and WinUI windows draw their own caption buttons, so DWM reports no bounds and the
//! default slot assumes three buttons. The real group can be narrower (one close button) or sit next to
//! more icons; its left edge is found by walking left from the right edge over occupied columns.

/// A free gap up to this wide, outside the buttons' own span, still belongs to the group.
pub(super) const MAX_GAP_DP: i32 = 12;

/// Where to look, in scan columns.
pub(super) struct GroupSearch {
    /// Column just right of the last caption button (the window's visible right edge).
    pub(super) right: usize,
    /// Width of one caption button.
    pub(super) button: usize,
    /// How many buttons the default slot assumes.
    pub(super) buttons: usize,
    /// Free columns that may separate two parts of the group outside the buttons' span.
    pub(super) max_gap: usize,
}

/// First column of the caption-button group. `free[x]` is true when column `x` shows only title-bar background.
///
/// A button may hold only a small symbol on the background, so inside the span of the assumed buttons a gap up
/// to a whole button width is bridged. When the group ends inside that span, its edge snaps to the button grid
/// (the symbol sits in the middle of its button). Left of the span only `max_gap` is bridged. An empty strip
/// gives the assumed span.
pub(super) fn group_left(free: &[bool], search: &GroupSearch) -> usize {
    let right = search.right.min(free.len());
    let span_left = right.saturating_sub(search.buttons.saturating_mul(search.button));
    let mut left = None;
    let mut last = right;
    for x in (0..right).rev().filter(|&x| !free[x]) {
        let tolerance = if x >= span_left || left.is_none() {
            search.button
        } else {
            search.max_gap
        };
        if last - x - 1 > tolerance {
            break;
        }
        left = Some(x);
        last = x;
    }
    match left {
        None => span_left,
        Some(x) if x >= span_left => snap_to_buttons(right, x, search.button),
        Some(x) => x,
    }
}

/// The left edge of the button that holds column `x`, counted from `right` in whole buttons.
fn snap_to_buttons(right: usize, x: usize, button: usize) -> usize {
    let buttons = (right - x).div_ceil(button.max(1));
    right.saturating_sub(buttons * button)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUTTON: usize = 46;

    fn search(right: usize) -> GroupSearch {
        GroupSearch {
            right,
            button: BUTTON,
            buttons: 3,
            max_gap: 12,
        }
    }

    /// A strip of `width` free columns with the given `(from, to)` ranges occupied.
    fn strip(width: usize, occupied: &[(usize, usize)]) -> Vec<bool> {
        (0..width)
            .map(|x| !occupied.iter().any(|&(a, b)| (a..b).contains(&x)))
            .collect()
    }

    #[test]
    fn three_buttons_with_small_symbols_give_the_three_button_span() {
        let width = 600;
        let glyphs = [(556, 566), (510, 520), (464, 474)];
        let free = strip(width, &glyphs);
        assert_eq!(group_left(&free, &search(width)), width - 3 * BUTTON);
    }

    #[test]
    fn one_close_button_gives_a_group_of_one_button() {
        let width = 446;
        let free = strip(width, &[(400, 410), (20, 120)]);
        assert_eq!(group_left(&free, &search(width)), width - BUTTON);
    }

    #[test]
    fn two_buttons_give_a_group_of_two_buttons() {
        let width = 600;
        let free = strip(width, &[(556, 566), (510, 520)]);
        assert_eq!(group_left(&free, &search(width)), width - 2 * BUTTON);
    }

    #[test]
    fn buttons_of_another_width_snap_to_the_button_grid_never_into_a_button() {
        let width = 600;
        // Three 40 px buttons: symbols at 20, 60 and 100 from the right edge.
        let free = strip(width, &[(570, 580), (530, 540), (490, 500)]);
        let left = group_left(&free, &search(width));
        assert!(left <= 480, "never inside a button: {left}");
        assert!(
            left >= 480 - BUTTON,
            "at most one button too far left: {left}"
        );
    }

    #[test]
    fn a_hover_highlighted_button_is_a_filled_run() {
        let width = 600;
        let free = strip(width, &[(554, 600), (510, 520), (464, 474)]);
        assert_eq!(group_left(&free, &search(width)), width - 3 * BUTTON);
    }

    #[test]
    fn content_touching_the_group_extends_it_with_the_content() {
        let width = 600;
        // Layout icons directly left of three buttons, each within 12 columns of the next.
        let free = strip(
            width,
            &[(556, 566), (510, 520), (464, 474), (440, 458), (410, 428)],
        );
        assert_eq!(group_left(&free, &search(width)), 410);
    }

    #[test]
    fn content_beyond_a_wide_gap_is_not_part_of_the_group() {
        let width = 600;
        let free = strip(width, &[(556, 566), (510, 520), (464, 474), (300, 380)]);
        assert_eq!(group_left(&free, &search(width)), width - 3 * BUTTON);
    }

    #[test]
    fn a_strip_without_symbols_gives_the_assumed_span() {
        let free = strip(600, &[]);
        assert_eq!(group_left(&free, &search(600)), 600 - 3 * BUTTON);
    }

    #[test]
    fn extreme_sizes_and_empty_input_do_not_panic() {
        let wide = GroupSearch {
            right: 100,
            button: usize::MAX,
            buttons: usize::MAX,
            max_gap: usize::MAX,
        };
        assert!(group_left(&strip(100, &[(80, 90)]), &wide) <= 100);
        assert_eq!(group_left(&[], &search(0)), 0);
        assert_eq!(group_left(&[], &search(50)), 0);
        assert_eq!(group_left(&strip(10, &[(0, 10)]), &search(1_000_000)), 0);
        let zero = GroupSearch {
            button: 0,
            buttons: 0,
            ..search(30)
        };
        let _ = group_left(&strip(30, &[(5, 9)]), &zero);
    }

    #[test]
    fn a_window_narrower_than_the_assumed_group_keeps_the_symbol_it_sees() {
        let free = strip(100, &[(80, 90)]);
        assert_eq!(group_left(&free, &search(100)), 100 - BUTTON);
    }
}
