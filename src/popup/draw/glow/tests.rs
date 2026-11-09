use tiny_skia::PathBuilder;

use super::*;
use crate::popup::draw::color;

const SIZE: u32 = 120;
const CENTER: f32 = 60.0;
const SHAPE_RADIUS: f32 = 12.0;

fn background(theme: &Theme) -> Pixmap {
    let mut pm = Pixmap::new(SIZE, SIZE).expect("pixmap");
    pm.fill(color(theme.background));
    pm
}

fn circle(cx: f32, cy: f32) -> Path {
    PathBuilder::from_circle(cx, cy, SHAPE_RADIUS).expect("circle")
}

fn rgba(pm: &Pixmap, x: u32, y: u32) -> [u8; 4] {
    let p = pm.pixel(x, y).expect("pixel");
    [p.red(), p.green(), p.blue(), p.alpha()]
}

fn sum_rgb(pm: &Pixmap) -> u64 {
    pm.pixels()
        .iter()
        .map(|p| u64::from(p.red()) + u64::from(p.green()) + u64::from(p.blue()))
        .sum()
}

fn dark_glow() -> Glow {
    Glow::brand(&Theme::DARK, 8.0, 1.0)
}

#[test]
fn glow_is_brighter_than_the_background_next_to_the_shape() {
    let mut pm = background(&Theme::DARK);
    let before = rgba(&pm, 60 + SHAPE_RADIUS as u32 + 2, 60);
    draw_glow(
        &mut pm,
        &GlowShape::Fill(&circle(CENTER, CENTER)),
        &dark_glow(),
    );
    let after = rgba(&pm, 60 + SHAPE_RADIUS as u32 + 2, 60);
    assert!(after[0] > before[0] + 20, "{before:?} -> {after:?}");
    assert!(after[2] > before[2] + 10, "{before:?} -> {after:?}");
}

#[test]
fn glow_falls_off_with_distance_and_reaches_nothing_far_away() {
    let mut pm = background(&Theme::DARK);
    let bg = rgba(&pm, 0, 0);
    draw_glow(
        &mut pm,
        &GlowShape::Fill(&circle(CENTER, CENTER)),
        &dark_glow(),
    );
    let along: Vec<u8> = (0..30)
        .map(|d| rgba(&pm, 60 + SHAPE_RADIUS as u32 + d, 60)[0])
        .collect();
    assert!(along.windows(2).all(|w| w[1] <= w[0]), "{along:?}");
    assert!(along[0] > along[8] && along[8] > along[16], "{along:?}");
    assert_eq!(rgba(&pm, 0, 0), bg);
    assert_eq!(rgba(&pm, SIZE - 1, SIZE - 1), bg);
}

#[test]
fn a_zero_radius_or_zero_strength_draws_nothing() {
    let shape = circle(CENTER, CENTER);
    for glow in [
        Glow::brand(&Theme::DARK, 0.0, 1.0),
        Glow::brand(&Theme::DARK, 8.0, 0.0),
        Glow::brand(&Theme::DARK, -3.0, 1.0),
    ] {
        let mut pm = background(&Theme::DARK);
        let before = pm.clone();
        draw_glow(&mut pm, &GlowShape::Fill(&shape), &glow);
        assert_eq!(pm.data(), before.data());
    }
}

#[test]
fn a_shape_at_or_beyond_the_edge_neither_panics_nor_writes_outside() {
    for (cx, cy) in [
        (0.0, 0.0),
        (-4.0, 60.0),
        (SIZE as f32 + 3.0, 60.0),
        (60.0, 119.0),
    ] {
        let mut pm = background(&Theme::DARK);
        draw_glow(&mut pm, &GlowShape::Fill(&circle(cx, cy)), &dark_glow());
        assert_eq!(pm.width(), SIZE);
    }
    // A shape completely outside, further than the glow reaches, leaves everything alone.
    let mut pm = background(&Theme::DARK);
    let before = pm.clone();
    draw_glow(
        &mut pm,
        &GlowShape::Fill(&circle(-200.0, -200.0)),
        &dark_glow(),
    );
    assert_eq!(pm.data(), before.data());
}

#[test]
fn a_shape_cut_by_the_edge_still_glows_into_the_pixmap() {
    let mut pm = background(&Theme::DARK);
    let before = rgba(&pm, 10, 60);
    draw_glow(&mut pm, &GlowShape::Fill(&circle(-2.0, 60.0)), &dark_glow());
    assert!(rgba(&pm, 10, 60)[0] > before[0]);
}

#[test]
fn every_pixel_stays_premultiplied_for_each_blend_and_surface() {
    let shape = circle(CENTER, CENTER);
    for theme in [Theme::DARK, Theme::LIGHT] {
        for glow in [
            Glow::brand(&theme, 8.0, 1.0),
            Glow::shadow(8.0, 0.35, (3.0, 3.0)),
            Glow {
                blend: Blend::Plus,
                ..Glow::brand(&theme, 20.0, 1.0)
            },
        ] {
            let mut opaque = background(&theme);
            let mut clear = Pixmap::new(SIZE, SIZE).expect("pixmap");
            for pm in [&mut opaque, &mut clear] {
                draw_glow(pm, &GlowShape::Fill(&shape), &glow);
                for p in pm.pixels() {
                    assert!(
                        p.red() <= p.alpha() && p.green() <= p.alpha() && p.blue() <= p.alpha()
                    );
                }
            }
            assert!(opaque.pixels().iter().all(|p| p.alpha() == 255));
            assert!(clear.pixels().iter().any(|p| p.alpha() > 0));
        }
    }
}

#[test]
fn on_a_light_surface_the_halo_is_tinted_not_washed_out() {
    let mut pm = background(&Theme::LIGHT);
    draw_glow(
        &mut pm,
        &GlowShape::Fill(&circle(CENTER, CENTER)),
        &Glow::brand(&Theme::LIGHT, 8.0, 0.6),
    );
    let near = rgba(&pm, 60 + SHAPE_RADIUS as u32 + 2, 60);
    let bg = Theme::LIGHT.background;
    assert!(
        near[1] < bg[1] - 20,
        "green drops toward the rose tint: {near:?}"
    );
    assert!(near[0] > near[1] + 10, "rose, not grey: {near:?}");
}

#[test]
fn a_shadow_darkens_and_sits_at_its_offset() {
    let mut pm = background(&Theme::LIGHT);
    let shape = circle(CENTER, CENTER);
    draw_glow(
        &mut pm,
        &GlowShape::Fill(&shape),
        &Glow::shadow(6.0, 0.35, (6.0, 6.0)),
    );
    let down_right = rgba(&pm, 60 + 14, 60 + 14);
    let up_left = rgba(&pm, 60 - 14, 60 - 14);
    assert!(
        down_right[0] < up_left[0] - 10,
        "{down_right:?} vs {up_left:?}"
    );
}

#[test]
fn a_stroke_glows_around_its_line_and_blur_keeps_the_total_light() {
    let mut line = PathBuilder::new();
    line.move_to(30.0, 60.0);
    line.line_to(90.0, 60.0);
    let line = line.finish().expect("line");
    let shape = GlowShape::Stroke {
        path: &line,
        width: 3.0,
        cap: LineCap::Round,
    };
    let mut small = Pixmap::new(SIZE, SIZE).expect("pixmap");
    let mut large = small.clone();
    draw_glow(&mut small, &shape, &Glow::brand(&Theme::DARK, 3.0, 1.0));
    draw_glow(&mut large, &shape, &Glow::brand(&Theme::DARK, 12.0, 1.0));
    let (a, b) = (sum_rgb(&small) as f64, sum_rgb(&large) as f64);
    assert!(
        (a - b).abs() / a < 0.08,
        "light is spread, not added: {a} vs {b}"
    );
    assert!(rgba(&large, 60, 60 - 10)[0] > rgba(&small, 60, 60 - 10)[0]);
}

#[test]
fn the_box_width_is_odd_and_grows_with_the_radius() {
    let widths: Vec<usize> = [1.0, 4.0, 8.0, 16.0]
        .iter()
        .map(|&r| box_width(r))
        .collect();
    assert!(widths.iter().all(|w| w % 2 == 1));
    assert!(widths.windows(2).all(|w| w[1] >= w[0]));
}
