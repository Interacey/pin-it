//! Pixel-level tests of the button renderer.

use tiny_skia::{Pixmap, Point};

use super::render::{
    Glyph, MIN_HEAD_CONTRAST, MIN_STROKE_PX, Palette, glyph_transform, render_glyph_bgra, stroke_px,
};
use super::{ButtonStyle, render_button};
use crate::anim::ButtonAnim;
use crate::color::contrast_ratio;

const LIGHT_BAR: [u8; 3] = [243, 243, 243];
const DARK_BAR: [u8; 3] = [32, 32, 36];
const BLUE_BAR: [u8; 3] = [0, 120, 215];
/// Smallest contrast the outline ink must have against its background.
const MIN_FILL_CONTRAST: f32 = 1.5;
/// Smallest contrast of the outline ink against the title bar (WCAG AA for text).
const MIN_INK_CONTRAST: f32 = 4.5;
/// A point in the middle of the cap of the pin head, in glyph space.
const CAP_CENTRE: (f32, f32) = (0.0, -6.0);
/// A point on the needle, in glyph space.
const NEEDLE_MIDDLE: (f32, f32) = (0.0, 5.5);
/// Largest distance in pixels between the glyph's bounding-box center and the box center.
const MAX_CENTRE_ERROR_PX: f32 = 0.5;
/// The same for odd box sizes, where the center lies on a pixel center.
const MAX_CENTRE_ERROR_ODD_PX: f32 = 1.0;

fn px(buf: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * size + x) * 4) as usize;
    [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
}

fn rgb_of(bgra: [u8; 4]) -> [u8; 3] {
    [bgra[2], bgra[1], bgra[0]]
}

fn pinned_anim() -> ButtonAnim {
    let mut a = ButtonAnim::new(true);
    a.set_pinned(true);
    a
}

fn hovered_anim(pinned: bool) -> ButtonAnim {
    let mut a = ButtonAnim::new(false);
    a.set_pinned(pinned);
    a.on_hover(true);
    a.step(1.0);
    a
}

fn pressed_anim(pinned: bool) -> ButtonAnim {
    let mut a = ButtonAnim::new(false);
    a.set_pinned(pinned);
    a.on_press();
    a.step(0.3);
    a
}

fn pinned_style(accent: [u8; 3], title_bar: [u8; 3]) -> ButtonStyle {
    ButtonStyle {
        accent,
        title_bar,
        pinned: true,
        ..Default::default()
    }
}

/// Pixel of glyph-space point `at` for a button of `size` at rest.
fn glyph_pixel(size: u32, at: (f32, f32)) -> (u32, u32) {
    let glyph = Glyph::new().unwrap();
    let transform = glyph_transform(size as f32, &glyph, &ButtonAnim::new(true)).unwrap();
    let mut point = Point::from_xy(at.0, at.1);
    transform.map_points(std::slice::from_mut(&mut point));
    (point.x as u32, point.y as u32)
}

fn cap_pixel(size: u32) -> (u32, u32) {
    glyph_pixel(size, CAP_CENTRE)
}

fn alpha_sum(buf: &[u8]) -> u32 {
    buf.chunks_exact(4).map(|p| u32::from(p[3])).sum()
}

fn max_alpha(buf: &[u8]) -> u8 {
    buf.chunks_exact(4).map(|p| p[3]).max().unwrap_or(0)
}

/// Pixel bounding box (x0, y0, x1, y1 inclusive) of everything with alpha above 0.
fn alpha_bounds(buf: &[u8], size: u32) -> (u32, u32, u32, u32) {
    let (mut x0, mut y0, mut x1, mut y1) = (size, size, 0, 0);
    for (i, p) in buf.chunks_exact(4).enumerate() {
        if p[3] > 0 {
            let (x, y) = (i as u32 % size, i as u32 / size);
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
        }
    }
    assert!(x1 >= x0, "size {size}: nothing drawn");
    (x0, y0, x1, y1)
}

/// Distance of the alpha bounding-box center from the box center, per axis, in pixels.
fn centre_error(buf: &[u8], size: u32) -> (f32, f32) {
    let (x0, y0, x1, y1) = alpha_bounds(buf, size);
    let middle = size as f32 / 2.0;
    (
        ((x0 + x1 + 1) as f32 / 2.0 - middle).abs(),
        ((y0 + y1 + 1) as f32 / 2.0 - middle).abs(),
    )
}

#[test]
fn output_length_and_corner_at_the_alpha_floor() {
    for size in [16u32, 20, 32, 48] {
        for (style, anim) in [
            (pinned_style(BLUE_BAR, LIGHT_BAR), pinned_anim()),
            (ButtonStyle::default(), hovered_anim(false)),
        ] {
            let buf = render_button(size, &style, &anim);
            assert_eq!(buf.len(), (size * size * 4) as usize);
            for (x, y) in [(0, 0), (size - 1, 0), (0, size - 1), (size - 1, size - 1)] {
                assert_eq!(
                    px(&buf, size, x, y),
                    [0, 0, 0, 1],
                    "size {size} corner {x},{y}"
                );
            }
        }
    }
    assert!(render_button(0, &ButtonStyle::default(), &ButtonAnim::new(true)).is_empty());
}

#[test]
fn pinned_head_is_the_accent_and_stands_out_from_the_title_bar() {
    let cases = [
        ([200, 40, 40], LIGHT_BAR),
        ([200, 40, 40], DARK_BAR),
        (BLUE_BAR, BLUE_BAR),
        (LIGHT_BAR, LIGHT_BAR),
        (DARK_BAR, DARK_BAR),
    ];
    for size in [20u32, 24, 32] {
        for (accent, bar) in cases {
            let buf = render_glyph_bgra(size, &pinned_style(accent, bar), &pinned_anim());
            let (x, y) = cap_pixel(size);
            let cap = px(&buf, size, x, y);
            assert_eq!(cap[3], 255, "size {size} accent {accent:?} bar {bar:?}");
            let color = rgb_of(cap);
            assert_eq!(color, Palette::of(&pinned_style(accent, bar)).head);
            assert_ne!(color, bar);
            let contrast = contrast_ratio(color, bar);
            assert!(
                contrast >= MIN_HEAD_CONTRAST,
                "{accent:?} on {bar:?}: {contrast}"
            );
        }
    }
}

#[test]
fn a_good_accent_is_kept_as_it_is() {
    let style = pinned_style([200, 40, 40], LIGHT_BAR);
    assert_eq!(Palette::of(&style).head, [200, 40, 40]);
}

#[test]
fn outline_ink_reads_on_the_title_bar() {
    for bar in [LIGHT_BAR, DARK_BAR, BLUE_BAR] {
        let palette = Palette::of(&ButtonStyle {
            title_bar: bar,
            ..Default::default()
        });
        assert!(contrast_ratio(palette.ink, bar) >= MIN_INK_CONTRAST);
    }
}

#[test]
fn only_the_head_takes_the_accent_the_rest_is_ink() {
    for (accent, bar) in [([200, 40, 40], LIGHT_BAR), ([255, 140, 0], DARK_BAR)] {
        let style = pinned_style(accent, bar);
        let palette = Palette::of(&style);
        let buf = render_glyph_bgra(32, &style, &pinned_anim());
        let colors = |want: [u8; 3]| {
            buf.chunks_exact(4)
                .filter(|p| p[3] == 255 && rgb_of([p[0], p[1], p[2], p[3]]) == want)
                .count()
        };
        assert!(colors(palette.head) > 40, "{accent:?}: head pixels");
        assert!(colors(palette.ink) > 10, "{accent:?}: ink pixels");
        let (x, y) = glyph_pixel(32, NEEDLE_MIDDLE);
        let needle = px(&buf, 32, x, y);
        assert!(needle[3] > 100, "{accent:?}: needle is drawn");
        let ink = [2, 1, 0].map(|c| (u32::from(needle[c]) * 255 / u32::from(needle[3])) as u8);
        for (got, want) in ink.into_iter().zip(palette.ink) {
            assert!(got.abs_diff(want) <= 2, "{accent:?}: needle {ink:?}");
        }
    }
}

#[test]
fn premultiplied_invariant_holds() {
    for (accent, bar) in [
        ([255, 255, 255], LIGHT_BAR),
        ([0, 0, 0], DARK_BAR),
        ([30, 144, 255], BLUE_BAR),
    ] {
        for pinned in [false, true] {
            let style = ButtonStyle {
                accent,
                title_bar: bar,
                pinned,
                ..Default::default()
            };
            for a in [
                hovered_anim(pinned),
                pressed_anim(pinned),
                pinned_anim(),
                ButtonAnim::new(true),
            ] {
                let buf = render_glyph_bgra(32, &style, &a);
                for p in buf.chunks_exact(4) {
                    assert!(p[0] <= p[3] && p[1] <= p[3] && p[2] <= p[3], "{p:?}");
                }
            }
        }
    }
}

/// Chrome's caption symbols on a dark bar: grey at rest.
const NEIGHBOUR_INK: [u8; 3] = [0x55; 3];
/// Largest per-channel difference between a rendered ink pixel and the expected color.
const INK_TOLERANCE: u8 = 6;
/// Button size whose needle stroke is wide enough to hold fully covered pixels.
const INK_TEST_SIZE: u32 = 48;

fn inked_style(bar: [u8; 3], ink: Option<[u8; 3]>) -> ButtonStyle {
    ButtonStyle {
        title_bar: bar,
        ink,
        ..Default::default()
    }
}

/// Un-premultiplied color of the pixel in the middle of the needle; asserts it is opaque.
fn needle_color(style: &ButtonStyle, anim: &ButtonAnim) -> [u8; 3] {
    let buf = render_glyph_bgra(INK_TEST_SIZE, style, anim);
    let (x, y) = glyph_pixel(INK_TEST_SIZE, NEEDLE_MIDDLE);
    let needle = px(&buf, INK_TEST_SIZE, x, y);
    assert_eq!(needle[3], 255, "the needle pixel is not fully covered");
    rgb_of(needle)
}

fn assert_close(got: [u8; 3], want: [u8; 3]) {
    for (g, w) in got.into_iter().zip(want) {
        assert!(g.abs_diff(w) <= INK_TOLERANCE, "{got:?} is not {want:?}");
    }
}

#[test]
fn outline_at_rest_is_the_sampled_neighbour_ink() {
    for (bar, ink) in [(DARK_BAR, NEIGHBOUR_INK), (LIGHT_BAR, [0x60; 3])] {
        let style = inked_style(bar, Some(ink));
        assert_close(needle_color(&style, &ButtonAnim::new(false)), ink);
    }
}

#[test]
fn hover_ink_is_closer_to_the_extreme_than_rest() {
    let style = inked_style(DARK_BAR, Some(NEIGHBOUR_INK));
    let rest = needle_color(&style, &ButtonAnim::new(false));
    let hovered = needle_color(&style, &hovered_anim(false));
    assert!(hovered[0] > rest[0] + 60, "{rest:?} -> {hovered:?}");
    assert_close(hovered, [0xa0; 3]);
    let light = inked_style(LIGHT_BAR, Some([0x90; 3]));
    let rest = needle_color(&light, &ButtonAnim::new(false));
    assert!(needle_color(&light, &hovered_anim(false))[0] < rest[0]);
}

#[test]
fn pressed_uses_the_rest_ink() {
    let style = inked_style(DARK_BAR, Some(NEIGHBOUR_INK));
    assert_close(needle_color(&style, &pressed_anim(false)), NEIGHBOUR_INK);
}

#[test]
fn an_invisible_sampled_ink_is_pushed_to_the_minimum_contrast() {
    let style = inked_style(DARK_BAR, Some([34, 34, 38]));
    let palette = Palette::of(&style);
    assert!(contrast_ratio(palette.ink, DARK_BAR) >= MIN_FILL_CONTRAST);
}

#[test]
fn a_visible_sampled_ink_is_kept_exactly() {
    let palette = Palette::of(&inked_style(DARK_BAR, Some(NEIGHBOUR_INK)));
    assert_eq!(palette.ink, NEIGHBOUR_INK);
}

#[test]
fn pinned_outline_stands_out_from_the_head_fill() {
    let style = ButtonStyle {
        accent: [0x55, 0x55, 0x56],
        title_bar: DARK_BAR,
        pinned: true,
        ink: Some(NEIGHBOUR_INK),
        ..Default::default()
    };
    let palette = Palette::of(&style);
    assert!(contrast_ratio(palette.ink, palette.head) >= MIN_FILL_CONTRAST);
}

#[test]
fn without_a_sample_the_outline_is_black_or_white_and_dimmed_at_rest() {
    let style = inked_style(DARK_BAR, None);
    assert_eq!(Palette::of(&style).ink, [255; 3]);
    let buf = render_glyph_bgra(INK_TEST_SIZE, &style, &ButtonAnim::new(false));
    let (x, y) = glyph_pixel(INK_TEST_SIZE, NEEDLE_MIDDLE);
    let alpha = px(&buf, INK_TEST_SIZE, x, y)[3];
    assert!(alpha < 255 && alpha > 150, "rest alpha {alpha}");
}

#[test]
fn premultiplied_invariant_holds_with_a_sampled_ink() {
    for pinned in [false, true] {
        let style = ButtonStyle {
            pinned,
            ..inked_style(DARK_BAR, Some(NEIGHBOUR_INK))
        };
        for a in [hovered_anim(pinned), pressed_anim(pinned), pinned_anim()] {
            let buf = render_glyph_bgra(32, &style, &a);
            for p in buf.chunks_exact(4) {
                assert!(p[0] <= p[3] && p[1] <= p[3] && p[2] <= p[3], "{p:?}");
            }
        }
    }
}

#[test]
fn bgra_channel_order() {
    let buf = render_glyph_bgra(32, &pinned_style([255, 0, 0], LIGHT_BAR), &pinned_anim());
    let (x, y) = cap_pixel(32);
    assert_eq!(
        px(&buf, 32, x, y),
        [0, 0, 255, 255],
        "blue, green, red, alpha"
    );
}

#[test]
fn unpinned_is_an_outline_and_pinned_is_filled() {
    let size = 64;
    let (x, y) = cap_pixel(size);
    let outline = render_glyph_bgra(size, &ButtonStyle::default(), &ButtonAnim::new(true));
    assert_eq!(px(&outline, size, x, y)[3], 0, "unpinned cap is empty");
    let filled = render_glyph_bgra(size, &pinned_style(BLUE_BAR, LIGHT_BAR), &pinned_anim());
    assert_eq!(px(&filled, size, x, y)[3], 255, "pinned cap is filled");
    assert!(alpha_sum(&filled) > 2 * alpha_sum(&outline));
}

#[test]
fn nothing_is_drawn_around_the_glyph() {
    // The old disc covered the box; now every edge pixel row and column of the box stays clear at rest.
    for size in [20u32, 32, 64] {
        let buf = render_glyph_bgra(size, &pinned_style(BLUE_BAR, LIGHT_BAR), &pinned_anim());
        let (x0, y0, x1, y1) = alpha_bounds(&buf, size);
        assert!(
            x0 >= 1 && y0 >= 1 && x1 < size - 1 && y1 < size - 1,
            "size {size}"
        );
        let clear_at_middle_of_edges = [(size / 2, 0), (0, size / 2), (size - 1, size / 2)];
        for (x, y) in clear_at_middle_of_edges {
            assert_eq!(px(&buf, size, x, y)[3], 0, "size {size} at {x},{y}");
        }
    }
}

#[test]
fn hover_makes_the_glyph_stronger_and_larger_without_a_background() {
    let style = ButtonStyle::default();
    let rest = render_glyph_bgra(32, &style, &ButtonAnim::new(true));
    let hover = render_glyph_bgra(32, &style, &hovered_anim(false));
    assert!(max_alpha(&hover) > max_alpha(&rest), "ink alpha rises");
    assert!(alpha_sum(&hover) > alpha_sum(&rest));
    let (rx0, _, rx1, _) = alpha_bounds(&rest, 32);
    let (hx0, _, hx1, _) = alpha_bounds(&hover, 32);
    assert!(hx1 - hx0 >= rx1 - rx0, "bump does not shrink the glyph");
    let corner = px(&hover, 32, 2, 2)[3];
    assert_eq!(corner, 0, "no hover background shape");
}

#[test]
fn fill_follows_the_animation() {
    let style = pinned_style(BLUE_BAR, LIGHT_BAR);
    let (x, y) = cap_pixel(32);
    let mut anim = ButtonAnim::new(false);
    anim.set_pinned(true);
    anim.step(0.04);
    let mid = px(&render_glyph_bgra(32, &style, &anim), 32, x, y)[3];
    assert!(mid > 0 && mid < 255, "mid-transition alpha {mid}");
}

#[test]
fn glyph_scales_with_button_size() {
    let style = pinned_style([200, 40, 40], LIGHT_BAR);
    for size in [16u32, 24, 32, 48] {
        let buf = render_glyph_bgra(size, &style, &pinned_anim());
        let (x0, y0, x1, y1) = alpha_bounds(&buf, size);
        let span = ((x1 - x0 + 1).max(y1 - y0 + 1)) as f32 / size as f32;
        assert!((0.7..0.9).contains(&span), "size {size}: span {span}");
    }
}

#[test]
fn glyph_is_centered_in_the_box_in_every_state() {
    let open = ButtonStyle::default();
    let pinned = pinned_style([200, 40, 40], LIGHT_BAR);
    for size in [16u32, 17, 20, 24, 25, 28, 32] {
        let limit = if size % 2 == 0 {
            MAX_CENTRE_ERROR_PX
        } else {
            MAX_CENTRE_ERROR_ODD_PX
        };
        let states = [
            ("unpinned rest", &open, ButtonAnim::new(true)),
            ("unpinned hover", &open, hovered_anim(false)),
            ("unpinned pressed", &open, pressed_anim(false)),
            ("pinned rest", &pinned, pinned_anim()),
            ("pinned hover", &pinned, hovered_anim(true)),
            ("pinned pressed", &pinned, pressed_anim(true)),
        ];
        for (name, style, anim) in states {
            let (ex, ey) = centre_error(&render_glyph_bgra(size, style, &anim), size);
            assert!(
                ex <= limit && ey <= limit,
                "{name} at {size}: off by {ex}, {ey}"
            );
        }
    }
}

#[test]
fn stroke_never_thinner_than_minimum() {
    for size in [16u32, 20, 24, 32] {
        let w = stroke_px(size as f32);
        assert!(w >= MIN_STROKE_PX, "size {size}: {w}");
    }
    assert_eq!(stroke_px(16.0), 1.0);
    assert!(stroke_px(8.0) >= MIN_STROKE_PX);
    assert!(stroke_px(32.0) > stroke_px(16.0));
}

#[test]
fn press_shrinks_the_button() {
    let style = ButtonStyle::default();
    let rest = alpha_sum(&render_glyph_bgra(32, &style, &ButtonAnim::new(false)));
    let mut pressed = ButtonAnim::new(false);
    pressed.on_press();
    pressed.step(0.3);
    assert!(alpha_sum(&render_glyph_bgra(32, &style, &pressed)) < rest);
}

/// Writes preview PNGs when PIN_IT_PNG_DIR is set; does nothing otherwise.
#[test]
fn dump_previews() {
    let Ok(dir) = std::env::var("PIN_IT_PNG_DIR") else {
        return;
    };
    let cases: [(&str, [u8; 3], [u8; 3]); 7] = [
        ("light-own", LIGHT_BAR, LIGHT_BAR),
        ("neutral-dark", [32, 32, 32], [32, 32, 32]),
        ("dark-own", DARK_BAR, DARK_BAR),
        ("blue-own", BLUE_BAR, BLUE_BAR),
        ("light-red", LIGHT_BAR, [200, 40, 40]),
        ("dark-orange", DARK_BAR, [255, 140, 0]),
        ("blue-yellow", BLUE_BAR, [255, 200, 0]),
    ];
    for (name, bar, accent) in cases {
        for size in [20u32, 24, 32] {
            let states = preview_states(size, bar, accent);
            write_sheet(&format!("{dir}/{name}_{size}.png"), size, bar, &states);
        }
    }
}

/// Unpinned, hover, pressed, half-way pinned, pinned, pinned hover, pinned pressed.
fn preview_states(size: u32, bar: [u8; 3], accent: [u8; 3]) -> Vec<Vec<u8>> {
    let open = ButtonStyle {
        accent,
        title_bar: bar,
        ..Default::default()
    };
    let pinned = ButtonStyle {
        pinned: true,
        ..open
    };
    let held = |pinned_state: bool| {
        let mut a = ButtonAnim::new(false);
        a.set_pinned(pinned_state);
        a.on_press();
        a.step(0.3);
        a
    };
    let hovered = |pinned_state: bool| {
        let mut a = ButtonAnim::new(false);
        a.set_pinned(pinned_state);
        a.on_hover(true);
        a.step(1.0);
        a
    };
    let mut mid = ButtonAnim::new(false);
    mid.set_pinned(true);
    mid.step(0.04);
    vec![
        render_glyph_bgra(size, &open, &ButtonAnim::new(true)),
        render_glyph_bgra(size, &open, &hovered(false)),
        render_glyph_bgra(size, &open, &held(false)),
        render_glyph_bgra(size, &pinned, &mid),
        render_glyph_bgra(size, &pinned, &pinned_anim()),
        render_glyph_bgra(size, &pinned, &hovered(true)),
        render_glyph_bgra(size, &pinned, &held(true)),
    ]
}

/// Composes states side by side on a title-bar-like background, magnified 8x.
fn write_sheet(path: &str, size: u32, bg: [u8; 3], states: &[Vec<u8>]) {
    let zoom = 8u32;
    let pad = 4u32;
    let cell = size + 2 * pad;
    let (w, h) = (cell * states.len() as u32 * zoom, cell * zoom);
    let mut pm = Pixmap::new(w, h).unwrap();
    let data = pm.data_mut();
    for (i, st) in states.iter().enumerate() {
        for y in 0..cell * zoom {
            for x in 0..cell * zoom {
                let (sx, sy) = (x / zoom, y / zoom);
                let (ox, oy) = (sx.wrapping_sub(pad), sy.wrapping_sub(pad));
                let mut p = [bg[0], bg[1], bg[2]];
                if ox < size && oy < size {
                    let s = px(st, size, ox, oy);
                    let a = u32::from(s[3]);
                    let src = [s[2], s[1], s[0]];
                    for c in 0..3 {
                        p[c] = (u32::from(src[c]) + u32::from(p[c]) * (255 - a) / 255) as u8;
                    }
                }
                let o = ((y * w + i as u32 * cell * zoom + x) * 4) as usize;
                data[o..o + 4].copy_from_slice(&[p[0], p[1], p[2], 255]);
            }
        }
    }
    pm.save_png(path).unwrap();
}

#[test]
fn the_head_is_clearly_lighter_or_darker_than_a_neutral_bar() {
    for (bar, lighter) in [([32, 32, 32], true), ([243, 243, 243], false)] {
        // A window without an accent of its own: the accent is the bar color.
        for accent in [bar, mix_towards(bar, 2), [0, 0, 0], [255, 255, 255]] {
            let head = Palette::of(&pinned_style(accent, bar)).head;
            assert!(
                contrast_ratio(head, bar) >= MIN_HEAD_CONTRAST,
                "{accent:?} on {bar:?}: head {head:?}"
            );
            if accent == bar {
                assert_eq!(luminance_order(head, bar), lighter, "{bar:?}: {head:?}");
            }
        }
    }
}

#[test]
fn the_outline_keeps_the_lower_contrast_rule() {
    let style = inked_style(DARK_BAR, Some([34, 34, 38]));
    let ink = Palette::of(&style).ink;
    let contrast = contrast_ratio(ink, DARK_BAR);
    assert!(
        (MIN_FILL_CONTRAST..MIN_HEAD_CONTRAST).contains(&contrast),
        "{contrast}"
    );
}

/// True when `a` is lighter than `b`.
fn luminance_order(a: [u8; 3], b: [u8; 3]) -> bool {
    a.iter().map(|&c| u32::from(c)).sum::<u32>() > b.iter().map(|&c| u32::from(c)).sum::<u32>()
}

fn mix_towards(color: [u8; 3], delta: u8) -> [u8; 3] {
    color.map(|c| c.saturating_add(delta))
}

/// Source-over of premultiplied BGRA `src` on an opaque RGB background.
fn composite(src: [u8; 4], bar: [u8; 3]) -> [u8; 3] {
    let keep = 255 - u32::from(src[3]);
    let channel = |s: u8, b: u8| ((u32::from(s) * 255 + u32::from(b) * keep + 127) / 255) as u8;
    [
        channel(src[2], bar[0]),
        channel(src[1], bar[1]),
        channel(src[0], bar[2]),
    ]
}

fn style_anim_cases() -> Vec<(ButtonStyle, ButtonAnim)> {
    vec![
        (pinned_style(BLUE_BAR, LIGHT_BAR), pinned_anim()),
        (pinned_style([200, 40, 40], DARK_BAR), pressed_anim(true)),
        (ButtonStyle::default(), hovered_anim(false)),
        (ButtonStyle::default(), ButtonAnim::new(true)),
    ]
}

#[test]
fn every_pixel_takes_mouse_input_and_stays_premultiplied() {
    for size in [16u32, 20, 32, 48] {
        for (style, anim) in style_anim_cases() {
            let buf = render_button(size, &style, &anim);
            for (i, p) in buf.chunks_exact(4).enumerate() {
                assert!(p[3] >= 1, "size {size} pixel {i} has alpha 0");
                assert!(
                    p[0] <= p[3] && p[1] <= p[3] && p[2] <= p[3],
                    "size {size} pixel {i}: {p:?}"
                );
            }
        }
    }
}

#[test]
fn alpha_floor_changes_no_title_bar_pixel_by_more_than_one_level() {
    for size in [16u32, 20, 32, 48] {
        for bar in [LIGHT_BAR, DARK_BAR, BLUE_BAR] {
            for (style, anim) in style_anim_cases() {
                let floored = render_button(size, &style, &anim);
                let glyph = render_glyph_bgra(size, &style, &anim);
                for (f, g) in floored.chunks_exact(4).zip(glyph.chunks_exact(4)) {
                    let (f, g) = (
                        composite([f[0], f[1], f[2], f[3]], bar),
                        composite([g[0], g[1], g[2], g[3]], bar),
                    );
                    let worst = (0..3).map(|c| f[c].abs_diff(g[c])).max().unwrap();
                    assert!(worst <= 1, "size {size} bar {bar:?}: {f:?} vs {g:?}");
                }
            }
        }
    }
}
