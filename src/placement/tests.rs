//! Unit tests of the sizing rules, the slot chooser and the placement decision.

use super::*;

const BG: [u8; 3] = [32, 32, 32];
const INK: [u8; 3] = [230, 230, 230];
const CAPTION: i32 = 32;

fn slot(x: i32, size: i32) -> PinSlot {
    PinSlot { x, y: 4, size }
}

/// A scan `width` wide with every row background, then `paint` marks columns as content.
fn scan_with(width: usize, content: &[(usize, usize)]) -> TitleBarScan {
    let mut row = vec![BG; width];
    for &(from, to) in content {
        row[from..to].fill(INK);
    }
    TitleBarScan {
        window_size: (width as i32, CAPTION),
        caption_px: CAPTION,
        background: BG,
        caption_ink: None,
        accent: None,
        rows: vec![row; 7],
    }
}

/// A native-caption window of 800 x 32 px at 96 dpi with `slot` as its (scaled) button slot.
fn input(slot: PinSlot, origin: i32, dpi: u32) -> PlacementInput {
    PlacementInput {
        slot,
        estimated: false,
        dpi,
        window_origin: (origin, 0),
        window_size: (800, CAPTION),
        caption_hint_px: CAPTION,
        group_right: 0,
        waited: Duration::ZERO,
    }
}

/// The chosen slot of `scan` for `input`.
fn choose(scan: &TitleBarScan, input: &PlacementInput) -> Option<PinSlot> {
    choose_in(&free_columns(scan, BACKGROUND_TOLERANCE), input)
}

fn pick(scan: &TitleBarScan, default: PinSlot, origin: i32) -> Option<PinSlot> {
    choose(scan, &input(default, origin, 96))
}

#[test]
fn size_rule_on_the_grid() {
    let table = [
        (24, 16),
        (28, 20),
        (30, 20),
        (32, 24),
        (36, 24),
        (40, 28),
        (48, 32),
        (80, 32),
        (10, 16),
    ];
    for (caption, size) in table {
        assert_eq!(default_size_dp(caption), size, "caption {caption}");
        assert_eq!(size % GRID_DP, 0);
    }
}

#[test]
fn size_px_follows_dpi() {
    // 30 dp caption: 96 dpi 30 px, 125 dpi 38 px, 150 dpi 45 px, 200 dpi 60 px
    assert_eq!(default_size_px(30, 96), 20);
    assert_eq!(default_size_px(38, 120), 25);
    assert_eq!(default_size_px(45, 144), 30);
    assert_eq!(default_size_px(60, 192), 40);
}

#[test]
fn user_scale_snaps_to_grid_and_clamps() {
    assert_eq!(scaled_size_dp(20, 100), 20);
    assert_eq!(scaled_size_dp(20, 125), 24);
    assert_eq!(scaled_size_dp(20, 150), 32);
    assert_eq!(scaled_size_dp(20, 60), 12);
    assert_eq!(scaled_size_dp(16, 60), 12);
    assert_eq!(scaled_size_dp(32, 200), 40);
    assert_eq!(scaled_size_dp(24, 80), 20);
}

#[test]
fn user_scale_keeps_right_edge_and_center() {
    let s = PinSlot {
        x: 100,
        y: 10,
        size: 24,
    };
    assert_eq!(apply_user_scale(s, 100, 96), s);
    let big = apply_user_scale(s, 150, 96);
    assert_eq!(big.size, 36);
    assert_eq!(big.x + big.size, 124);
    assert_eq!(big.y + big.size / 2, 22);
    let at_150 = apply_user_scale(slot(500, 30), 100, 144);
    assert_eq!(at_150.size, 30);
}

#[test]
fn free_bar_keeps_the_default_slot() {
    let scan = scan_with(800, &[]);
    assert_eq!(pick(&scan, slot(700, 20), 0), Some(slot(700, 20)));
}

#[test]
fn occupied_right_side_shifts_left() {
    // content from 660 to 710; pin must end 4 px before 660
    let scan = scan_with(800, &[(660, 710)]);
    assert_eq!(pick(&scan, slot(700, 20), 0), Some(slot(660 - 4 - 20, 20)));
}

#[test]
fn content_left_of_the_default_slot_leaves_it_alone() {
    let scan = scan_with(800, &[(100, 300)]);
    assert_eq!(pick(&scan, slot(700, 20), 0), Some(slot(700, 20)));
}

#[test]
fn occupied_everywhere_gives_none() {
    let scan = scan_with(800, &[(0, 800)]);
    assert_eq!(pick(&scan, slot(700, 20), 0), None);
}

#[test]
fn tiny_gaps_are_too_small() {
    // free runs of 19 px: even the smallest button (12 + 8 = 20) does not fit
    let content: Vec<(usize, usize)> = (0..20).map(|i| (i * 40 + 19, i * 40 + 40)).collect();
    let scan = scan_with(800, &content);
    assert_eq!(pick(&scan, slot(700, 20), 0), None);
    let content: Vec<(usize, usize)> = (0..20).map(|i| (i * 40 + 20, i * 40 + 40)).collect();
    let scan = scan_with(800, &content);
    assert_eq!(pick(&scan, slot(700, 20), 0).map(|s| s.size), Some(12));
}

#[test]
fn a_button_that_does_not_fit_shrinks_on_the_grid_instead_of_vanishing() {
    // free runs of 27 px: 20 needs 28, 16 needs 24
    let content: Vec<(usize, usize)> = (0..20).map(|i| (i * 40 + 27, i * 40 + 40)).collect();
    let scan = scan_with(800, &content);
    let got = pick(&scan, slot(700, 20), 0).unwrap();
    assert_eq!(got.size, 16);
    assert_eq!(got.y, 6, "stays centred in the caption row");
    assert!(got.x + got.size <= 700 + 20);
}

#[test]
fn shrink_steps_follow_the_grid_at_other_dpi() {
    // 150 percent: 30 px is 20 dp; steps are 16 dp = 24 px and 12 dp = 18 px
    let sizes: Vec<i32> = candidate_sizes(30, 144).collect();
    assert_eq!(sizes, vec![30, 24, 18]);
    let sizes: Vec<i32> = candidate_sizes(48, 96).collect();
    assert_eq!(sizes, vec![48, 44, 40, 36, 32, 28, 24, 20, 16, 12]);
    let sizes: Vec<i32> = candidate_sizes(12, 96).collect();
    assert_eq!(sizes, vec![12]);
}

#[test]
fn noise_within_tolerance_is_background() {
    let mut scan = scan_with(800, &[]);
    for (i, px) in scan.rows[3].iter_mut().enumerate() {
        let d = (i % 3) as u8 * 6; // 0, 6, 12
        *px = [BG[0] + d, BG[1] - d, BG[2] + d];
    }
    assert_eq!(pick(&scan, slot(700, 20), 0), Some(slot(700, 20)));
}

#[test]
fn tolerance_exceeded_is_content() {
    let mut scan = scan_with(800, &[]);
    // one row of one column differs by 13 in a single channel
    scan.rows[2][710] = [BG[0] + 13, BG[1], BG[2]];
    assert_eq!(pick(&scan, slot(700, 20), 0), Some(slot(710 - 4 - 20, 20)));
}

#[test]
fn one_occupied_row_blocks_the_column() {
    let mut scan = scan_with(800, &[]);
    scan.rows[6][500..520].fill(INK);
    assert_eq!(pick(&scan, slot(515, 20), 0), Some(slot(500 - 4 - 20, 20)));
}

#[test]
fn run_touching_the_left_edge_keeps_the_margin() {
    // everything from 28 on is content; the free run 0..28 fits 20 + two margins exactly
    let scan = scan_with(800, &[(28, 800)]);
    assert_eq!(pick(&scan, slot(700, 20), 0), Some(slot(4, 20)));
    let narrow = scan_with(800, &[(27, 800)]);
    assert_eq!(pick(&narrow, slot(700, 20), 0).map(|s| s.size), Some(16));
    let too_narrow = scan_with(800, &[(19, 800)]);
    assert_eq!(pick(&too_narrow, slot(700, 20), 0), None);
}

#[test]
fn pin_never_reaches_the_caption_buttons() {
    let scan = scan_with(800, &[]);
    for default_x in [400, 600, 779] {
        let chosen = pick(&scan, slot(default_x, 20), 0).unwrap();
        assert!(chosen.x + chosen.size <= default_x + 20);
    }
}

#[test]
fn dpi_variants() {
    for (dpi, size, margin) in [(96, 20, 4), (120, 25, 5), (144, 30, 6), (192, 40, 8)] {
        let width = 1600usize;
        let default = slot(1400, size);
        let blocker = (1300usize, 1420usize);
        let scan = scan_with(width, &[blocker]);
        let got = choose(&scan, &input(default, 0, dpi)).unwrap();
        assert_eq!(got.x, 1300 - margin - size, "dpi {dpi}");
        assert_eq!(got.size, size);
    }
}

#[test]
fn maximized_window_offset_maps_columns_to_screen() {
    // maximized: window rect starts at -8, so column 0 is screen x -8
    let scan = scan_with(800, &[(660, 710)]);
    let got = pick(&scan, slot(692, 20), -8).unwrap();
    assert_eq!(got.x, -8 + 660 - 4 - 20);
    assert_eq!(got.y, 4);
}

#[test]
fn default_beyond_the_scan_is_clamped() {
    let scan = scan_with(100, &[]);
    let got = pick(&scan, slot(500, 20), 0).unwrap();
    assert_eq!(got.x, 80);
}

#[test]
fn empty_scan_gives_none() {
    let scan = TitleBarScan {
        window_size: (0, 0),
        caption_px: 0,
        background: BG,
        caption_ink: None,
        accent: None,
        rows: vec![],
    };
    assert_eq!(pick(&scan, slot(10, 20), 0), None);
}

fn estimated(slot: PinSlot) -> PlacementInput {
    PlacementInput {
        estimated: true,
        ..input(slot, 0, 96)
    }
}

#[test]
fn an_estimated_window_never_shows_the_default_slot_without_a_scan() {
    let d = slot(700, 20);
    let waiting = PlacementInput {
        waited: SCAN_WAIT - Duration::from_millis(1),
        ..estimated(d)
    };
    let over = PlacementInput {
        waited: SCAN_WAIT,
        ..estimated(d)
    };
    for state in [ScanState::Pending, ScanState::Unavailable] {
        assert_eq!(decide(&waiting, state), Placement::Wait);
        assert_eq!(decide(&over, state), Placement::NoScan);
        assert_eq!(decide(&input(d, 0, 96), state), Placement::Show(d));
    }
}

#[test]
fn decide_uses_the_scan_and_hides_when_full() {
    let d = slot(700, 20);
    let busy = scan_with(800, &[(660, 710)]);
    assert_eq!(
        decide(&estimated(d), ScanState::Ready(&busy)),
        Placement::Show(slot(638, 20))
    );
    let full = scan_with(800, &[(0, 800)]);
    assert_eq!(
        decide(&input(d, 0, 96), ScanState::Ready(&full)),
        Placement::Hide
    );
}

#[test]
fn a_pinned_windows_button_shrinks_before_it_is_hidden() {
    // Free runs of 27 px: the 20 px button would be hidden, the 16 px one fits.
    let content: Vec<(usize, usize)> = (0..20).map(|i| (i * 40 + 27, i * 40 + 40)).collect();
    let scan = scan_with(800, &content);
    let placed = decide(&input(slot(700, 20), 0, 96), ScanState::Ready(&scan));
    assert!(
        matches!(placed, Placement::Show(s) if s.size == 16),
        "{placed:?}"
    );
}

/// A window that was 900 px wide when it was scanned and is 800 px wide now (or the other way round).
fn resized_scan() -> TitleBarScan {
    TitleBarScan {
        window_size: (900, CAPTION),
        ..scan_with(900, &[(0, 900)])
    }
}

#[test]
fn a_stale_scan_keeps_an_estimated_button_visible_at_the_new_default_slot() {
    // The old behavior hid the button until the new scan arrived; a resize drag (top border, corner)
    // made it vanish. The estimated slot is anchored to the caption buttons and moves with them.
    let scan = resized_scan();
    let default = slot(700, 20);
    assert_eq!(
        decide(&estimated(default), ScanState::Ready(&scan)),
        Placement::Show(default)
    );
}

#[test]
fn a_stale_scan_moves_a_native_caption_button_to_the_new_default_slot() {
    // after a maximize the slot is the new default one, whatever the old scan said about the old width
    let scan = resized_scan();
    let default = slot(1700, 20);
    assert_eq!(
        decide(&input(default, 0, 96), ScanState::Ready(&scan)),
        Placement::Show(default)
    );
}

#[test]
fn a_scan_of_the_current_size_is_not_stale() {
    let scan = scan_with(800, &[(0, 800)]);
    assert_eq!(
        decide(&input(slot(700, 20), 0, 96), ScanState::Ready(&scan)),
        Placement::Hide
    );
}

fn in_caption(slot: PinSlot, caption_px: i32) -> PlacementInput {
    PlacementInput {
        caption_hint_px: caption_px,
        group_right: 0,
        waited: Duration::ZERO,
        ..input(slot, 0, 96)
    }
}

#[test]
fn the_button_never_exceeds_the_caption_minus_both_margins() {
    // 150 percent of a 24 px default is 36 px; a 32 px caption leaves 28
    let big = PinSlot {
        x: 700,
        y: 0,
        size: 36,
    };
    let Placement::Show(s) = decide(&in_caption(big, 32), ScanState::Pending) else {
        panic!("expected a slot");
    };
    assert_eq!(s.size, 28);
    assert_eq!(s.x + s.size, 736, "right edge stays");
    assert_eq!(s.y + s.size / 2, 18, "vertical center stays");
    // 200 percent of 20 is 40 px in a 30 px caption: 26 px of room, 24 on the grid
    let huge = PinSlot {
        x: 700,
        y: 0,
        size: 40,
    };
    let Placement::Show(s) = decide(&in_caption(huge, 30), ScanState::Pending) else {
        panic!("expected a slot");
    };
    assert_eq!(s.size, 24);
}

#[test]
fn the_scan_caption_height_wins_over_the_hint() {
    let big = slot(700, 36);
    let scan = TitleBarScan {
        caption_px: 40,
        ..scan_with(800, &[])
    };
    let Placement::Show(s) = decide(&in_caption(big, 32), ScanState::Ready(&scan)) else {
        panic!("expected a slot");
    };
    assert_eq!(s.size, 36, "40 px caption: 36 px of room");
}

#[test]
fn a_button_that_fits_the_caption_is_left_alone() {
    let d = slot(700, 20);
    assert_eq!(
        decide(&in_caption(d, 30), ScanState::Pending),
        Placement::Show(d)
    );
    let at_limit = slot(700, 24);
    assert_eq!(
        decide(&in_caption(at_limit, 32), ScanState::Pending),
        Placement::Show(at_limit)
    );
}

#[test]
fn a_caption_that_cannot_hold_twelve_dp_hides_the_button() {
    // 12 dp plus two 2 dp margins needs 16 px
    assert!(matches!(
        decide(&in_caption(slot(700, 20), 16), ScanState::Pending),
        Placement::Show(s) if s.size == 12
    ));
    assert_eq!(
        decide(&in_caption(slot(700, 20), 15), ScanState::Pending),
        Placement::Hide
    );
}

#[test]
fn the_caption_clamp_works_in_dp_at_high_dpi() {
    // 150 percent: caption 45 px (30 dp), vertical margin 3 px; a 45 px (30 dp) button leaves 39 px = 26 dp, 24 dp (36 px) on the grid
    let input = PlacementInput {
        dpi: 144,
        caption_hint_px: 45,
        group_right: 0,
        waited: Duration::ZERO,
        ..input(slot(1400, 45), 0, 144)
    };
    let Placement::Show(s) = decide(&input, ScanState::Pending) else {
        panic!("expected a slot");
    };
    assert_eq!(s.size, 36);
}

#[test]
fn the_dial_reference_caption_allows_sizes_above_the_automatic_one() {
    // 32 dp caption leaves 28 dp: 24 dp stays, 32 dp is limited to 28 dp, below 12 dp is Hide.
    assert_eq!(fit_size_to_caption(24, 32, 96), Some(24));
    assert_eq!(fit_size_to_caption(32, 32, 96), Some(28));
    assert_eq!(fit_size_to_caption(40, 30, 96), Some(24));
    assert_eq!(fit_size_to_caption(20, 15, 96), None);
}

#[test]
fn the_hint_never_shrinks_a_default_button() {
    for dpi in [96, 120, 144, 192] {
        for caption_dp in 20..=80 {
            let caption = scale_dp(caption_dp, dpi);
            let size = default_size_px(caption, dpi);
            let default = PinSlot { x: 700, y: 0, size };
            let input = PlacementInput {
                caption_hint_px: caption_hint_px(size, dpi),
                group_right: 0,
                waited: Duration::ZERO,
                ..input(default, 0, dpi)
            };
            assert_eq!(
                decide(&input, ScanState::Pending),
                Placement::Show(default),
                "dpi {dpi} caption {caption_dp} dp"
            );
        }
    }
}

// ----- caption-button group of custom title bars -----

/// An estimated window `width` px wide at 96 dpi whose default slot sits left of three 46 px buttons.
fn custom_bar(width: i32) -> PlacementInput {
    PlacementInput {
        estimated: true,
        window_size: (width, CAPTION),
        ..input(slot(width - 1 - 138 - 4 - 20, 20), 0, 96)
    }
}

fn placed(input: &PlacementInput, scan: &TitleBarScan) -> PinSlot {
    match decide(input, ScanState::Ready(scan)) {
        Placement::Show(s) => s,
        other => panic!("expected a slot, got {other:?}"),
    }
}

/// Occupied columns of `scan` (every row alike) as sorted column numbers.
fn occupied(scan: &TitleBarScan) -> Vec<usize> {
    free_columns(scan, BACKGROUND_TOLERANCE)
        .iter()
        .enumerate()
        .filter(|&(_, &free)| !free)
        .map(|(x, _)| x)
        .collect()
}

/// Asserts the pin overlaps no content and keeps `CONTENT_MARGIN_DP` to the nearest content on its left.
fn assert_clear_of_content(scan: &TitleBarScan, pin: PinSlot) {
    let (start, end) = (pin.x as usize, (pin.x + pin.size) as usize);
    let cols = occupied(scan);
    assert!(
        cols.iter().all(|&x| x < start || x >= end),
        "pin {start}..{end} overlaps content"
    );
    let left_neighbour = cols.iter().rev().find(|&&x| x < start).copied();
    if let Some(x) = left_neighbour {
        assert!(
            start - x > 4,
            "content at {x} is closer than 4 px to {start}"
        );
    }
}

#[test]
fn a_one_button_window_puts_the_pin_next_to_its_only_button() {
    // PowerToys Color Picker: 446 px wide, a close button at the right, title text at the left.
    let scan = scan_with(446, &[(400, 410), (47, 120)]);
    let input = custom_bar(446);
    let s = placed(&input, &scan);
    assert_eq!(
        s.x + s.size,
        446 - 1 - 46 - 4,
        "4 px left of the close button"
    );
    assert_clear_of_content(&scan, s);
}

#[test]
fn a_three_button_window_keeps_the_default_slot() {
    let scan = scan_with(800, &[(771, 781), (725, 735), (679, 689), (20, 200)]);
    let input = custom_bar(800);
    assert_eq!(placed(&input, &scan), input.slot);
}

#[test]
fn narrower_buttons_do_not_push_the_pin_into_them() {
    // Three 32 px buttons: symbols 16, 48 and 80 px from the right edge.
    let scan = scan_with(800, &[(778, 788), (746, 756), (714, 724)]);
    let s = placed(&custom_bar(800), &scan);
    let real_group_left = 800 - 1 - 3 * 32;
    assert!(s.x + s.size <= real_group_left, "{s:?}");
    assert!(s.x + s.size >= real_group_left - 46, "{s:?}");
}

#[test]
fn a_hover_highlighted_button_does_not_move_the_pin() {
    let hovered = scan_with(800, &[(753, 799), (725, 735), (679, 689)]);
    let input = custom_bar(800);
    assert_eq!(placed(&input, &hovered), input.slot);
}

#[test]
fn layout_icons_next_to_the_buttons_keep_the_pin_four_px_away_on_both_sides() {
    // VS Code: three buttons, the layout icons in the 38 px right before them, an Update button before that.
    let scan = scan_with(
        800,
        &[
            (771, 781),
            (725, 735),
            (679, 689),
            (630, 648),
            (600, 618),
            (570, 588),
            (460, 560),
        ],
    );
    let s = placed(&custom_bar(800), &scan);
    assert_clear_of_content(&scan, s);
    assert!(s.x + s.size <= 460 - 4, "left of the Update button: {s:?}");
}

#[test]
fn content_directly_left_of_the_group_is_kept_clear() {
    let scan = scan_with(800, &[(771, 781), (725, 735), (679, 689), (640, 660)]);
    let s = placed(&custom_bar(800), &scan);
    assert_clear_of_content(&scan, s);
    assert!(s.x + s.size <= 640 - 4 || s.x >= 660 + 4, "{s:?}");
}

#[test]
fn the_pin_keeps_the_margin_on_both_sides_in_a_small_free_run() {
    let scan = scan_with(800, &[(0, 300), (330, 720)]);
    let s = pick(&scan, slot(700, 20), 0).unwrap();
    assert!(s.x >= 300 + 4, "left margin: {s:?}");
    assert!(s.x + s.size <= 330 - 4, "right margin: {s:?}");
    assert_eq!(
        pick(&scan_with(800, &[(0, 300), (316, 720)]), slot(700, 20), 0),
        None
    );
}

#[test]
fn a_stale_scan_is_lined_up_at_the_right_edge() {
    // 900 px wide when scanned, 800 now: an icon 90 px from the right edge covers the default slot.
    let old = TitleBarScan {
        window_size: (900, CAPTION),
        ..scan_with(900, &[(810, 830)])
    };
    let input = input(slot(700, 20), 0, 96);
    let s = placed(&input, &old);
    assert_eq!(s.x + s.size, 710 - 4, "{s:?}");
}

#[test]
fn only_a_scan_of_the_current_size_hides_the_button() {
    let old_full = TitleBarScan {
        window_size: (900, CAPTION),
        ..scan_with(900, &[(0, 900)])
    };
    let current_full = scan_with(800, &[(0, 800)]);
    let input = input(slot(700, 20), 0, 96);
    assert!(matches!(
        decide(&input, ScanState::Ready(&old_full)),
        Placement::Show(_)
    ));
    assert_eq!(
        decide(&input, ScanState::Ready(&current_full)),
        Placement::Hide
    );
}

// ----- maximized windows -----
// A maximized window is 16 px wider than the screen: 8 invisible px on each side, which a capture shows as black columns.

const BORDER_PX: usize = 8;
const BORDER: [u8; 3] = [0, 0, 0];
const COLORED_BAR: [u8; 3] = [200, 60, 60];
const MAXIMIZED_WIDTH: i32 = 2576;
const MAXIMIZED_ORIGIN: i32 = -8;
const RESTORED_WIDTH: i32 = 440;
/// First column of the caption-button group in the maximized window (window-relative).
const MAXIMIZED_GROUP_LEFT: i32 = 2421;
const RESTORED_GROUP_LEFT: i32 = 287;
const GAP_PX: i32 = 4;
const PIN_PX: i32 = 16;

/// A scan of a window with invisible side borders; `title` marks content columns (the title text).
fn framed_scan(width: usize, bar: [u8; 3], title: (usize, usize)) -> TitleBarScan {
    let mut row = vec![bar; width];
    row[..BORDER_PX].fill(BORDER);
    row[width - BORDER_PX..].fill(BORDER);
    row[title.0..title.1].fill(INK);
    TitleBarScan {
        window_size: (width as i32, CAPTION),
        caption_px: 22,
        background: bar,
        caption_ink: None,
        accent: None,
        rows: vec![row; 16],
    }
}

/// A native-caption window: the pin ends `GAP_PX` left of the group at `group_left` (window-relative).
fn native_window(origin: i32, width: i32, group_left: i32) -> PlacementInput {
    PlacementInput {
        slot: PinSlot {
            x: origin + group_left - GAP_PX - PIN_PX,
            y: 3,
            size: PIN_PX,
        },
        window_origin: (origin, -8),
        window_size: (width, CAPTION),
        caption_hint_px: 22,
        group_right: 0,
        waited: Duration::ZERO,
        ..input(slot(0, PIN_PX), origin, 96)
    }
}

fn maximized_window() -> PlacementInput {
    native_window(MAXIMIZED_ORIGIN, MAXIMIZED_WIDTH, MAXIMIZED_GROUP_LEFT)
}

#[test]
fn a_maximized_bar_with_invisible_borders_keeps_the_pin_next_to_the_buttons() {
    for bar in [BG, COLORED_BAR] {
        let scan = framed_scan(MAXIMIZED_WIDTH as usize, bar, (16, 110));
        let input = maximized_window();
        assert_eq!(
            decide(&input, ScanState::Ready(&scan)),
            Placement::Show(input.slot),
            "bar {bar:?}"
        );
    }
}

#[test]
fn a_colored_native_bar_never_reports_no_free_run_when_only_the_title_is_in_it() {
    let scan = framed_scan(MAXIMIZED_WIDTH as usize, COLORED_BAR, (16, 110));
    assert!(matches!(
        decide(&maximized_window(), ScanState::Ready(&scan)),
        Placement::Show(_)
    ));
}

#[test]
fn restored_then_maximized_then_restored_follows_the_buttons_every_time() {
    let restored_scan = framed_scan(RESTORED_WIDTH as usize, COLORED_BAR, (16, 110));
    let maximized_scan = framed_scan(MAXIMIZED_WIDTH as usize, COLORED_BAR, (16, 110));
    let restored = native_window(2080, RESTORED_WIDTH, RESTORED_GROUP_LEFT);
    let maximized = maximized_window();
    let steps = [
        (&restored, &restored_scan),
        // the new size, the scan of the old one still in the cache
        (&maximized, &restored_scan),
        (&maximized, &maximized_scan),
        (&restored, &maximized_scan),
        (&restored, &restored_scan),
    ];
    for (i, (input, scan)) in steps.into_iter().enumerate() {
        assert_eq!(
            decide(input, ScanState::Ready(scan)),
            Placement::Show(input.slot),
            "step {i}"
        );
    }
}

#[test]
fn a_stale_scan_from_before_a_maximize_does_not_leave_the_pin_at_the_old_width() {
    // The pin was seen 1933 px left of the buttons in a 2560 px window: the old scan ended at 500 px.
    let old = framed_scan(RESTORED_WIDTH as usize, BG, (16, 110));
    let input = maximized_window();
    let Placement::Show(s) = decide(&input, ScanState::Ready(&old)) else {
        panic!("a stale scan must not hide the button");
    };
    assert_eq!(s, input.slot);
}

#[test]
fn the_pin_never_reaches_into_the_buttons_while_a_window_shrinks_with_a_stale_scan() {
    // Every step has a new size and a scan of the previous one; native and estimated slots alike.
    let mut scan = framed_scan(900, BG, (16, 110));
    for width in (500..=880).rev().step_by(20) {
        let native = native_window(100, width, width - 153);
        let estimated = PlacementInput {
            estimated: true,
            ..native
        };
        for input in [native, estimated] {
            let Placement::Show(s) = decide(&input, ScanState::Ready(&scan)) else {
                panic!("width {width}: button hidden");
            };
            let group_left = 100 + width - 153;
            assert!(
                s.x + s.size <= group_left - GAP_PX,
                "width {width} estimated {}: pin {s:?} ends right of {}",
                input.estimated,
                group_left - GAP_PX
            );
        }
        scan = framed_scan(width as usize, BG, (16, 110));
    }
}

// ----- caption-button group read from the pixels -----

/// Glyph columns of buttons `button` wide, counted from the visible right edge `right`: `centers_from_right`
/// are the glyph centers as distances from it; every glyph is 10 columns wide.
fn glyphs(right: usize, centers_from_right: &[usize]) -> Vec<(usize, usize)> {
    centers_from_right
        .iter()
        .map(|&c| (right - c - 5, right - c + 5))
        .collect()
}

/// A native window of `width` columns whose DWM group edge is `dwm_left` and whose drawn buttons end at `right`.
fn drawn_buttons(width: i32, dwm_left: i32, right: i32) -> PlacementInput {
    PlacementInput {
        group_right: right,
        waited: Duration::ZERO,
        ..native_window(0, width, dwm_left)
    }
}

fn pin_right(placement: Placement) -> i32 {
    let Placement::Show(s) = placement else {
        panic!("expected a shown pin, got {placement:?}");
    };
    s.x + s.size
}

#[test]
fn a_native_group_that_dwm_reports_too_wide_gets_the_exact_gap_to_the_drawn_buttons() {
    // DWM: group from 647; drawn: three 46 px buttons ending at 792, so the group starts at 654.
    let scan = scan_with(800, &glyphs(792, &[23, 69, 115]));
    let input = drawn_buttons(800, 647, 792);
    let placed = decide(&input, ScanState::Ready(&scan));
    assert_eq!(pin_right(placed), 654 - GAP_PX);
}

#[test]
fn without_the_visible_edge_the_dwm_edge_stands() {
    let scan = scan_with(800, &glyphs(792, &[23, 69, 115]));
    let input = drawn_buttons(800, 647, 0);
    assert_eq!(
        pin_right(decide(&input, ScanState::Ready(&scan))),
        647 - GAP_PX
    );
}

#[test]
fn one_drawn_button_of_three_assumed_keeps_the_dwm_edge() {
    // Only the close glyph shows (the others are invisible on this bar): 99 px from the DWM edge, more
    // than one button, so the pixels are not trusted and the pin stays clear of the assumed group.
    let scan = scan_with(800, &glyphs(792, &[23]));
    let input = drawn_buttons(800, 647, 792);
    assert_eq!(
        pin_right(decide(&input, ScanState::Ready(&scan))),
        647 - GAP_PX
    );
}

#[test]
fn a_native_close_button_alone_is_a_group_of_one_button() {
    // DWM reports 49 px for the single button (744), the drawn button is 46 px (746).
    let scan = scan_with(800, &glyphs(792, &[23]));
    let input = drawn_buttons(800, 744, 792);
    assert_eq!(
        pin_right(decide(&input, ScanState::Ready(&scan))),
        746 - GAP_PX
    );
}

#[test]
fn the_exact_gap_holds_at_150_percent_dpi() {
    // 69 px buttons, the pin gap is 6 px; DWM reports the group 12 px too wide.
    let right = 1200_i32;
    let scan = scan_with(1210, &glyphs(right as usize, &[34, 103, 172]));
    let input = PlacementInput {
        dpi: 144,
        ..drawn_buttons(1210, right - 3 * 69 - 12, right)
    };
    assert_eq!(
        pin_right(decide(&input, ScanState::Ready(&scan))),
        right - 3 * 69 - 6
    );
}

#[test]
fn a_hover_highlighted_native_button_does_not_move_the_edge() {
    let mut content = glyphs(792, &[23, 69, 115]);
    content[2] = (792 - 138, 792 - 92);
    let scan = scan_with(800, &content);
    let input = drawn_buttons(800, 647, 792);
    assert_eq!(
        pin_right(decide(&input, ScanState::Ready(&scan))),
        654 - GAP_PX
    );
}

#[test]
fn the_pin_keeps_the_content_margin_left_of_a_tab_list_button_next_to_the_group() {
    // A 12 px chevron button 6 px left of the group (as in Firefox's tab strip) joins the group.
    let mut content = glyphs(1008, &[23, 69, 115]);
    content.push((848, 860));
    let scan = scan_with(1016, &content);
    let input = PlacementInput {
        estimated: true,
        ..drawn_buttons(1016, 870, 1008)
    };
    assert_eq!(
        pin_right(decide(&input, ScanState::Ready(&scan))),
        848 - GAP_PX
    );
}
