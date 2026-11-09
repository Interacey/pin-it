use super::*;
use crate::exclusions::colors::{TRASH_REST, trash_hover};
use crate::popup::Theme;

const EDGE: f32 = 20.0;

/// The icon alone on a transparent pixmap of its own box size.
fn icon_pixmap(scale: f32, rgb: [u8; 3]) -> Pixmap {
    let side = (EDGE * scale).round() as u32;
    let mut pm = Pixmap::new(side, side).expect("pixmap");
    let area = Area {
        x: 0.0,
        y: 0.0,
        w: side as f32,
        h: side as f32,
    };
    draw_trash(&mut pm, &TrashIcon { area, rgb }, None);
    pm
}

fn alpha_at(pm: &Pixmap, grid: (f32, f32)) -> u8 {
    let k = pm.width() as f32 / GRID;
    pm.pixel((grid.0 * k) as u32, (grid.1 * k) as u32)
        .expect("pixel inside")
        .alpha()
}

/// Bounding box of the inked pixels: left, top, right, bottom (exclusive).
fn ink_bounds(pm: &Pixmap) -> (u32, u32, u32, u32) {
    let (mut l, mut t, mut r, mut b) = (u32::MAX, u32::MAX, 0, 0);
    for y in 0..pm.height() {
        for x in 0..pm.width() {
            if pm.pixel(x, y).unwrap().alpha() > 0 {
                (l, t, r, b) = (l.min(x), t.min(y), r.max(x + 1), b.max(y + 1));
            }
        }
    }
    (l, t, r, b)
}

#[test]
fn the_path_has_the_five_parts_of_the_art() {
    let path = trash_path().expect("path");
    let move_count = path
        .segments()
        .filter(|s| matches!(s, tiny_skia::PathSegment::MoveTo(_)))
        .count();
    assert_eq!(move_count, 5, "lid, handle, body and two inner lines");
}

#[test]
fn fully_covered_pixels_have_exactly_the_given_color_and_the_rest_is_a_partial_cover() {
    for rgb in [
        TRASH_REST,
        trash_hover(&Theme::DARK),
        trash_hover(&Theme::LIGHT),
    ] {
        let pm = icon_pixmap(4.0, rgb);
        let mut solid = 0;
        for px in pm.pixels() {
            // Premultiplied: no channel above alpha.
            assert!(px.red() <= px.alpha() && px.green() <= px.alpha() && px.blue() <= px.alpha());
            if px.alpha() == 255 {
                solid += 1;
                assert_eq!([px.red(), px.green(), px.blue()], rgb);
            } else if px.alpha() >= 128 {
                let c = px.demultiply();
                for (got, want) in [c.red(), c.green(), c.blue()].into_iter().zip(rgb) {
                    assert!(
                        got.abs_diff(want) <= 2,
                        "{got} vs {want} at alpha {}",
                        px.alpha()
                    );
                }
            }
        }
        assert!(solid > 300, "only {solid} solid pixels for {rgb:?}");
    }
}

#[test]
fn the_art_is_on_the_strokes_and_empty_between_them() {
    let pm = icon_pixmap(4.0, TRASH_REST);
    for (what, at) in [
        ("lid", (12.0, 6.0)),
        ("lid end", (3.6, 6.0)),
        ("handle top", (12.0, 2.9)),
        ("body left", (5.9, 13.0)),
        ("body right", (18.1, 13.0)),
        ("body bottom", (12.0, 20.2)),
        ("inner left", (9.75, 13.5)),
        ("inner right", (14.25, 13.5)),
    ] {
        assert_eq!(alpha_at(&pm, at), 255, "{what} at {at:?}");
    }
    for (what, at) in [
        ("between handle and lid", (12.0, 4.5)),
        ("between the inner lines", (12.0, 13.0)),
        ("outside the lid", (1.0, 6.0)),
        ("under the body", (12.0, 23.0)),
        ("inside the body left of the line", (7.6, 13.0)),
    ] {
        assert_eq!(alpha_at(&pm, at), 0, "{what} at {at:?}");
    }
}

#[test]
fn the_ink_is_centered_in_its_box_at_every_scale() {
    for scale in [1.0, 1.25, 1.5, 2.0, 3.0] {
        let pm = icon_pixmap(scale, TRASH_REST);
        let (l, t, r, b) = ink_bounds(&pm);
        let side = pm.width() as f32;
        let (cx, cy) = ((l + r) as f32 / 2.0, (t + b) as f32 / 2.0);
        assert!(
            (cx - side / 2.0).abs() <= 1.0,
            "x {cx} of {side} at scale {scale}"
        );
        assert!(
            (cy - side / 2.0).abs() <= 1.0,
            "y {cy} of {side} at scale {scale}"
        );
        assert!(
            l >= 1 && t >= 1 && r < pm.width() && b < pm.height(),
            "room to breathe"
        );
    }
}

#[test]
fn the_stroke_scales_with_the_box() {
    let ink = |scale: f32| {
        let pm = icon_pixmap(scale, TRASH_REST);
        pm.pixels()
            .iter()
            .map(|p| u32::from(p.alpha()))
            .sum::<u32>() as f32
    };
    // Twice the edge: twice the length and twice the width, so about four times the ink.
    let ratio = ink(2.0) / ink(1.0);
    assert!((3.6..4.4).contains(&ratio), "{ratio}");
}

#[test]
fn a_clip_mask_keeps_the_icon_out_of_the_masked_part() {
    let mut pm = Pixmap::new(80, 80).unwrap();
    let mut mask = Mask::new(80, 80).unwrap();
    let top_half = tiny_skia::Rect::from_xywh(0.0, 0.0, 80.0, 40.0).unwrap();
    mask.fill_path(
        &PathBuilder::from_rect(top_half),
        tiny_skia::FillRule::Winding,
        false,
        Transform::identity(),
    );
    let area = Area {
        x: 0.0,
        y: 0.0,
        w: 80.0,
        h: 80.0,
    };
    draw_trash(
        &mut pm,
        &TrashIcon {
            area,
            rgb: TRASH_REST,
        },
        Some(&mask),
    );
    let (_, _, _, bottom) = ink_bounds(&pm);
    assert!(bottom <= 40, "ink down to {bottom}");
}
