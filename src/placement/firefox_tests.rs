//! The pin in a Firefox-like caption strip: next to the list-all-tabs chevron, never on it, and never at the
//! estimated default slot before a scan of the painted window is there.

use super::firefox_strip::FirefoxStrip;
use super::*;

const WIDTH: usize = 1100;
const SIZE: i32 = 24;
const GAP_PX: usize = 4;

/// An estimated 96 dpi window at the origin whose default slot sits `GAP_PX` left of the three buttons.
fn window(strip: &FirefoxStrip, waited: Duration) -> PlacementInput {
    PlacementInput {
        slot: PinSlot {
            x: (strip.span_left() - GAP_PX) as i32 - SIZE,
            y: 4,
            size: SIZE,
        },
        estimated: true,
        dpi: 96,
        window_origin: (0, 0),
        window_size: (strip.width as i32, 32),
        caption_hint_px: 32,
        group_right: strip.group_right as i32,
        waited,
    }
}

fn placed(strip: &FirefoxStrip) -> PinSlot {
    match decide(
        &window(strip, Duration::ZERO),
        ScanState::Ready(&strip.scan()),
    ) {
        Placement::Show(slot) => slot,
        other => panic!("expected a slot, got {other:?}"),
    }
}

fn pin_end(slot: PinSlot) -> usize {
    (slot.x + slot.size) as usize
}

fn assert_next_to_chevron(strip: &FirefoxStrip) {
    let slot = placed(strip);
    assert_eq!(
        strip.content_left() - pin_end(slot),
        GAP_PX,
        "gap to the chevron, pin at {slot:?}"
    );
    assert_eq!(slot.size, SIZE);
}

#[test]
fn the_pin_sits_four_dp_left_of_the_chevron_in_dark_and_light_strips() {
    for dark in [true, false] {
        for tabs in [1, 3, 20] {
            assert_next_to_chevron(&FirefoxStrip::new(WIDTH, dark, tabs));
        }
    }
}

#[test]
fn a_hovered_chevron_counts_with_its_highlight() {
    let strip = FirefoxStrip {
        hover_chevron: true,
        ..FirefoxStrip::new(WIDTH, true, 3)
    };
    assert_next_to_chevron(&strip);
}

#[test]
fn a_gradient_background_does_not_turn_the_chevron_into_free_space() {
    for dark in [true, false] {
        let strip = FirefoxStrip {
            gradient: true,
            ..FirefoxStrip::new(WIDTH, dark, 3)
        };
        assert_next_to_chevron(&strip);
    }
}

#[test]
fn a_crowded_strip_leaves_the_pin_exactly_where_the_free_run_fits() {
    let size_and_margins = SIZE as usize + 2 * GAP_PX;
    let strip = FirefoxStrip {
        free_before_chevron: Some(size_and_margins),
        ..FirefoxStrip::new(WIDTH, true, 20)
    };
    let slot = placed(&strip);
    assert_eq!(strip.content_left() - pin_end(slot), GAP_PX);
    assert_eq!(
        slot.x as usize - (strip.content_left() - size_and_margins),
        GAP_PX
    );
}

#[test]
fn a_free_run_that_is_one_pixel_too_small_shrinks_the_pin_instead_of_covering_content() {
    let strip = FirefoxStrip {
        free_before_chevron: Some(SIZE as usize + 2 * GAP_PX - 1),
        ..FirefoxStrip::new(WIDTH, true, 20)
    };
    let slot = placed(&strip);
    assert_eq!(strip.content_left() - pin_end(slot), GAP_PX);
    assert!(slot.size < SIZE, "{slot:?}");
}

#[test]
fn the_free_space_closest_to_the_buttons_is_chosen_not_the_widest() {
    let strip = FirefoxStrip::new(WIDTH, true, 1);
    let slot = placed(&strip);
    // The big free run left of the tabs' end is wider, but the one at the chevron is closer to the buttons.
    assert!(
        pin_end(slot) > strip.chevron_left() - 2 * SIZE as usize,
        "{slot:?}"
    );
}

#[test]
fn an_unpainted_or_half_painted_strip_is_not_trusted_during_the_wait() {
    let strip = FirefoxStrip::new(WIDTH, true, 3);
    for scan in [strip.unpainted_scan(), strip.half_painted_scan()] {
        let early = window(&strip, SCAN_WAIT - Duration::from_millis(1));
        assert_eq!(decide(&early, ScanState::Ready(&scan)), Placement::Wait);
    }
}

#[test]
fn a_painted_strip_is_used_at_once() {
    let strip = FirefoxStrip::new(WIDTH, true, 3);
    let early = window(&strip, Duration::ZERO);
    assert!(matches!(
        decide(&early, ScanState::Ready(&strip.scan())),
        Placement::Show(_)
    ));
}

#[test]
fn a_strip_that_never_shows_caption_symbols_is_used_after_the_wait() {
    let strip = FirefoxStrip::new(WIDTH, true, 3);
    let over = window(&strip, SCAN_WAIT);
    assert!(matches!(
        decide(&over, ScanState::Ready(&strip.unpainted_scan())),
        Placement::Show(_)
    ));
}

#[test]
fn a_native_window_does_not_wait_for_painted_symbols() {
    let strip = FirefoxStrip::new(WIDTH, true, 3);
    let native = PlacementInput {
        estimated: false,
        ..window(&strip, Duration::ZERO)
    };
    assert!(matches!(
        decide(&native, ScanState::Ready(&strip.unpainted_scan())),
        Placement::Show(_)
    ));
}

#[test]
fn a_shaded_strip_with_a_filled_active_tab_still_puts_the_pin_next_to_the_chevron() {
    for tabs in [1, 3, 20] {
        let strip = FirefoxStrip {
            colorway: true,
            ..FirefoxStrip::new(WIDTH, true, tabs)
        };
        assert_next_to_chevron(&strip);
    }
}
