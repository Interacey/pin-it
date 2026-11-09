use std::time::Instant;

use super::*;
use crate::dial::model::{DETENTS, STEP_DEG, TickClass, angle_of_percent, tick_class};
use crate::popup::theme::BRAND_ACCENT;

const THEMES: [Theme; 2] = [Theme::DARK, Theme::LIGHT];
const TICK_BAND_DP: f32 = 96.0;
/// How much darker than the surface (or how close to the brand color) a cut pixel is.
const CUT_DELTA: i32 = 12;

fn scene(angle: f32) -> Scene {
    Scene {
        angle,
        flash: None,
        glyph_px: 24,
    }
}

fn render(scale: f32, theme: Theme, scene: &Scene) -> Pixmap {
    render_dial(Layout::new(scale), theme, scene).expect("pixmap")
}

fn rgb_at(pm: &Pixmap, point: (f32, f32)) -> [u8; 3] {
    let p = pm
        .pixel(point.0.round() as u32, point.1.round() as u32)
        .expect("pixel inside");
    [p.red(), p.green(), p.blue()]
}

fn distance(a: [u8; 3], b: [u8; 3]) -> i32 {
    a.iter()
        .zip(b)
        .map(|(&x, y)| (i32::from(x) - i32::from(y)).abs())
        .max()
        .unwrap_or(0)
}

fn close(a: [u8; 3], b: [u8; 3], tolerance: i32) -> bool {
    distance(a, b) <= tolerance
}

fn luma(c: [u8; 3]) -> f32 {
    0.2126 * f32::from(c[0]) + 0.7152 * f32::from(c[1]) + 0.0722 * f32::from(c[2])
}

/// The pixel at `at` belongs to the tick of `percent`: the cut is darker than the surface around
/// it (its light edge below is not counted), the major tick is the brand color.
fn is_cut(pm: &Pixmap, at: (f32, f32), surface: [u8; 3], percent: u32) -> bool {
    let px = rgb_at(pm, at);
    if tick_class(percent) == TickClass::Major {
        close(px, BRAND_ACCENT, CUT_DELTA)
    } else {
        luma(px) < luma(surface) - CUT_DELTA as f32
    }
}

/// A tick cut within 1.5 degrees of the angle of `percent` at `radius_dp`.
fn has_cut(pm: &Pixmap, layout: &Layout, percent: u32, radius_dp: f32) -> bool {
    let surface = surface_at(pm, layout, percent, radius_dp);
    let angle = angle_of_percent(percent as f32);
    (-15..=15).any(|i| {
        let at = layout.polar(angle + i as f32 / 10.0, radius_dp);
        is_cut(pm, at, surface, percent)
    })
}

/// A point between two ticks at the same radius, which shows the empty surface.
fn surface_at(pm: &Pixmap, layout: &Layout, percent: u32, radius_dp: f32) -> [u8; 3] {
    let between = angle_of_percent(percent as f32) + STEP_DEG / 2.0;
    rgb_at(pm, layout.polar(between, radius_dp))
}

/// Length of the tick of `percent` in dp, measured on the picture along its radius.
fn drawn_length_dp(pm: &Pixmap, layout: &Layout, percent: u32) -> f32 {
    let angle = angle_of_percent(percent as f32);
    let step = 0.25;
    let mut length = 0.0;
    let mut radius = 106.0;
    while radius > 78.0 {
        let surface = surface_at(pm, layout, percent, radius);
        if is_cut(pm, layout.polar(angle, radius), surface, percent) {
            length += step;
        }
        radius -= step;
    }
    length
}

/// Width of the tick of `percent` in dp, measured on the picture along the arc at 96 dp.
fn drawn_width_dp(pm: &Pixmap, layout: &Layout, percent: u32) -> f32 {
    let angle = angle_of_percent(percent as f32);
    let surface = surface_at(pm, layout, percent, TICK_BAND_DP);
    let arc_per_degree = TICK_BAND_DP * std::f32::consts::PI / 180.0;
    let step = 0.05;
    let mut inked = 0;
    let mut offset = -4.0f32;
    while offset <= 4.0 {
        if is_cut(
            pm,
            layout.polar(angle + offset, TICK_BAND_DP),
            surface,
            percent,
        ) {
            inked += 1;
        }
        offset += step;
    }
    inked as f32 * step * arc_per_degree
}

#[test]
fn size_follows_the_scale() {
    let s = scene(0.0);
    for (scale, size) in [(1.0, (280, 356)), (1.5, (420, 534)), (2.0, (560, 712))] {
        let pm = render(scale, Theme::DARK, &s);
        assert_eq!((pm.width(), pm.height()), size);
    }
}

#[test]
fn every_pixel_is_premultiplied_and_opaque_in_every_state() {
    let flash = Some(Flash {
        percent: 105,
        level: 1.0,
    });
    for theme in THEMES {
        for s in [
            scene(0.0),
            scene(angle_of_percent(125.0)),
            scene(angle_of_percent(80.0) + 4.0),
            Scene {
                flash,
                ..scene(angle_of_percent(105.0))
            },
        ] {
            let pm = render(1.5, theme, &s);
            for px in pm.pixels() {
                assert_eq!(px.alpha(), 255);
                assert!(
                    px.red() <= px.alpha() && px.green() <= px.alpha() && px.blue() <= px.alpha()
                );
            }
        }
    }
}

#[test]
fn background_and_border_use_the_theme() {
    for theme in THEMES {
        let pm = render(1.0, theme, &scene(0.0));
        assert_eq!(rgb_at(&pm, (140.0, 350.0)), theme.background);
        assert_eq!(rgb_at(&pm, (0.0, 170.0)), theme.hairline);
    }
}

#[test]
fn the_major_tick_is_longer_than_the_big_ones_which_are_longer_than_the_small_ones() {
    for theme in THEMES {
        let layout = Layout::new(4.0);
        let pm = render(4.0, theme, &scene(angle_of_percent(60.0)));
        let length = |p| drawn_length_dp(&pm, &layout, p);
        let (major, big, small) = (length(100), length(125), length(105));
        assert!(
            major > big + 3.0 && big > small + 2.0,
            "{major} {big} {small}"
        );
        assert!((major - 18.0).abs() < 1.5, "major {major}");
        assert!((big - 12.0).abs() < 1.5, "big {big}");
        assert!((small - 7.0).abs() < 1.5, "small {small}");
        assert!(
            (length(75) - big).abs() < 0.8
                && (length(50) - big).abs() < 0.8
                && (length(150) - big).abs() < 0.8
        );
        assert!((length(65) - small).abs() < 0.8 && (length(140) - small).abs() < 0.8);
    }
}

#[test]
fn the_major_tick_is_thicker_than_the_big_ones_which_are_thicker_than_the_small_ones() {
    for theme in THEMES {
        let layout = Layout::new(4.0);
        let pm = render(4.0, theme, &scene(angle_of_percent(60.0)));
        let width = |p| drawn_width_dp(&pm, &layout, p);
        let (major, big, small) = (width(100), width(75), width(65));
        assert!(
            major > big + 0.5 && big > small + 0.5,
            "{major} {big} {small}"
        );
        assert!((major - 3.0).abs() < 0.7 && (big - 2.0).abs() < 0.7 && (small - 1.0).abs() < 0.7);
    }
}

#[test]
fn there_is_a_tick_at_every_detent_angle_and_none_between() {
    for theme in THEMES {
        let layout = Layout::new(4.0);
        let pm = render(4.0, theme, &scene(angle_of_percent(50.0)));
        for percent in DETENTS {
            assert!(
                has_cut(&pm, &layout, percent, TICK_BAND_DP),
                "no tick at {percent}"
            );
            let surface = surface_at(&pm, &layout, percent, TICK_BAND_DP);
            let angle = angle_of_percent(percent as f32) + STEP_DEG / 2.0;
            for offset in [-2.0, 0.0, 2.0] {
                let at = layout.polar(angle + offset, TICK_BAND_DP);
                assert!(!is_cut(&pm, at, surface, 105), "stray mark after {percent}");
            }
        }
    }
}

#[test]
fn the_major_tick_is_the_brand_accent_and_the_others_are_not() {
    for theme in THEMES {
        let layout = Layout::new(4.0);
        let pm = render(4.0, theme, &scene(angle_of_percent(50.0)));
        let major = rgb_at(&pm, layout.polar(0.0, TICK_BAND_DP));
        assert!(close(major, BRAND_ACCENT, 6), "{major:?}");
        let big = rgb_at(&pm, layout.polar(angle_of_percent(125.0), TICK_BAND_DP));
        assert!(distance(big, BRAND_ACCENT) > 20, "{big:?}");
    }
}

#[test]
fn the_tick_ring_is_symmetric_about_the_vertical_axis() {
    for theme in THEMES {
        let scale = 2.0;
        let layout = Layout::new(scale);
        let pm = render(scale, theme, &scene(0.0));
        let (cx, cy) = layout.center();
        let width = pm.width() as i32;
        let mut checked = 0;
        for y in 0..pm.height() as i32 {
            for x in 0..width / 2 {
                let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                let r = dx.hypot(dy) / scale;
                if !(78.0..=104.0).contains(&r) {
                    continue;
                }
                let a = rgb_at(&pm, (x as f32, y as f32));
                let b = rgb_at(&pm, ((width - 1 - x) as f32, y as f32));
                assert!(close(a, b, 10), "({x}, {y}): {a:?} vs {b:?}");
                checked += 1;
            }
        }
        assert!(checked > 5000);
    }
}

#[test]
fn the_indicator_points_up_at_100_percent_and_turns_with_the_knob() {
    for theme in THEMES {
        let layout = Layout::new(4.0);
        let up = render(4.0, theme, &scene(0.0));
        let core_up = rgb_at(&up, layout.polar(0.0, 32.0));
        assert!(close(core_up, BRAND_ACCENT, 6), "{core_up:?}");
        let beside = rgb_at(&up, layout.polar(8.0, 32.0));
        assert!(
            distance(beside, BRAND_ACCENT) > 20,
            "not 8 degrees off: {beside:?}"
        );
        let turned = render(4.0, theme, &scene(angle_of_percent(125.0)));
        let core = rgb_at(&turned, layout.polar(angle_of_percent(125.0), 32.0));
        assert!(close(core, BRAND_ACCENT, 6), "{core:?}");
        let old_place = rgb_at(&turned, layout.polar(0.0, 32.0));
        assert!(distance(old_place, BRAND_ACCENT) > 20);
    }
}

#[test]
fn the_inlay_runs_from_twelve_oclock_to_the_knob_on_either_side() {
    for theme in THEMES {
        let layout = Layout::new(4.0);
        let track = |pm: &Pixmap, angle: f32| rgb_at(pm, layout.polar(angle, arc::TRACK_RADIUS_DP));
        let right = render(4.0, theme, &scene(40.0));
        assert!(
            close(track(&right, 20.0), BRAND_ACCENT, 8),
            "{:?}",
            track(&right, 20.0)
        );
        assert!(close(track(&right, 38.0), BRAND_ACCENT, 8));
        assert!(
            distance(track(&right, 60.0), BRAND_ACCENT) > 20,
            "beyond the knob"
        );
        assert!(
            distance(track(&right, -20.0), BRAND_ACCENT) > 20,
            "other side"
        );
        let left = render(4.0, theme, &scene(-40.0));
        assert!(close(track(&left, -20.0), BRAND_ACCENT, 8));
        assert!(distance(track(&left, 20.0), BRAND_ACCENT) > 20);
        let center = render(4.0, theme, &scene(0.0));
        for angle in [-30.0, 30.0] {
            assert!(
                distance(track(&center, angle), BRAND_ACCENT) > 20,
                "empty at 100"
            );
        }
    }
}

#[test]
fn the_inlay_stops_at_the_ends_of_the_arc() {
    let layout = Layout::new(4.0);
    let at_end = render(4.0, Theme::DARK, &scene(135.0));
    let past = render(4.0, Theme::DARK, &scene(170.0));
    let track = |pm: &Pixmap, angle: f32| rgb_at(pm, layout.polar(angle, arc::TRACK_RADIUS_DP));
    assert!(close(track(&at_end, 134.0), BRAND_ACCENT, 8));
    assert!(close(track(&past, 134.0), BRAND_ACCENT, 8));
    assert!(
        distance(track(&past, 160.0), BRAND_ACCENT) > 20,
        "no inlay in the gap"
    );
}

#[test]
fn the_inlay_glows_on_the_dark_theme() {
    let layout = Layout::new(4.0);
    let empty = render(4.0, Theme::DARK, &scene(0.0));
    let lit = render(4.0, Theme::DARK, &scene(60.0));
    let beside = layout.polar(30.0, arc::TRACK_RADIUS_DP + 4.0);
    assert!(luma(rgb_at(&lit, beside)) > luma(rgb_at(&empty, beside)) + 8.0);
}

#[test]
fn a_flash_brightens_and_glows_around_the_tick_and_fades_out() {
    for theme in THEMES {
        let layout = Layout::new(2.0);
        let angle = angle_of_percent(125.0);
        let quiet = render(2.0, theme, &scene(angle));
        let flash = |level| Scene {
            flash: Some(Flash {
                percent: 125,
                level,
            }),
            ..scene(angle)
        };
        let lit = render(2.0, theme, &flash(1.0));
        let half = render(2.0, theme, &flash(0.3));
        let on_tick = layout.polar(angle, 96.0);
        let beside = layout.polar(angle + 3.0, 96.0);
        let change = |pm: &Pixmap, at| distance(rgb_at(pm, at), rgb_at(&quiet, at));
        assert!(change(&lit, on_tick) > 40, "tick {}", change(&lit, on_tick));
        assert!(change(&lit, beside) > 8, "glow {}", change(&lit, beside));
        assert!(change(&half, on_tick) < change(&lit, on_tick));
        let far = layout.polar(angle_of_percent(80.0), 96.0);
        assert_eq!(rgb_at(&lit, far), rgb_at(&quiet, far), "only that tick");
        let none = render(2.0, theme, &flash(0.0));
        assert_eq!(none.data(), quiet.data());
    }
}

/// Mean brightness of the rim between two angles at the middle of the rim ring.
fn rim_brightness(pm: &Pixmap, layout: &Layout, from: f32, to: f32) -> f32 {
    let samples = 40;
    (0..samples)
        .map(|i| {
            let a = from + (to - from) * i as f32 / (samples - 1) as f32;
            luma(rgb_at(pm, layout.polar(a, 50.5)))
        })
        .sum::<f32>()
        / samples as f32
}

#[test]
fn the_rim_is_lit_at_the_top_left_and_shaded_at_the_bottom_right() {
    for theme in THEMES {
        let layout = Layout::new(4.0);
        let pm = render(4.0, theme, &scene(0.0));
        let lit = rim_brightness(&pm, &layout, -65.0, -25.0);
        let shaded = rim_brightness(&pm, &layout, 115.0, 155.0);
        assert!(lit > shaded + 25.0, "{lit} vs {shaded}");
    }
}

#[test]
fn the_face_is_lighter_near_the_light_than_at_its_far_edge() {
    for theme in THEMES {
        let layout = Layout::new(4.0);
        let pm = render(4.0, theme, &scene(0.0));
        let near = luma(rgb_at(&pm, layout.polar(-45.0, 30.0)));
        let far = luma(rgb_at(&pm, layout.polar(135.0, 38.0)));
        assert!(near > far + 8.0, "{near} vs {far}");
    }
}

#[test]
fn the_highlight_stays_put_while_the_ridges_turn() {
    for theme in THEMES {
        let layout = Layout::new(4.0);
        let (a, b) = (
            render(4.0, theme, &scene(0.0)),
            render(4.0, theme, &scene(6.43)),
        );
        let highlight = layout.polar(-50.0, 31.0);
        assert!(close(rgb_at(&a, highlight), rgb_at(&b, highlight), 3));
        let ridge_band: Vec<(f32, f32)> = (0..360).map(|d| layout.polar(d as f32, 51.0)).collect();
        let moved = ridge_band
            .iter()
            .filter(|&&p| distance(rgb_at(&a, p), rgb_at(&b, p)) > 6)
            .count();
        assert!(moved > 100, "only {moved} of 360 ridge samples changed");
    }
}

#[test]
fn the_ridges_repeat_28_times_around_the_rim() {
    let layout = Layout::new(4.0);
    let pm = render(4.0, Theme::DARK, &scene(0.0));
    let samples = 3600;
    let values: Vec<f32> = (0..samples)
        .map(|i| {
            luma(rgb_at(
                &pm,
                layout.polar(i as f32 * 360.0 / samples as f32, 51.0),
            ))
        })
        .collect();
    // The strongest repetition rate around the rim, from the discrete Fourier transform.
    let strength = |k: usize| {
        let (mut re, mut im) = (0.0f32, 0.0f32);
        for (i, v) in values.iter().enumerate() {
            let phase = std::f32::consts::TAU * (k * i) as f32 / samples as f32;
            re += v * phase.cos();
            im += v * phase.sin();
        }
        re.hypot(im)
    };
    let strongest = (15..=45).max_by(|&a, &b| strength(a).total_cmp(&strength(b)));
    assert_eq!(strongest, Some(28));
}

#[test]
fn the_knob_casts_a_shadow_down_and_to_the_right() {
    for theme in THEMES {
        let layout = Layout::new(4.0);
        let pm = render(4.0, theme, &scene(0.0));
        let below = luma(rgb_at(&pm, layout.polar(135.0, 59.0)));
        let above = luma(rgb_at(&pm, layout.polar(-45.0, 59.0)));
        assert!(below < above - 6.0, "{below} vs {above}");
        let far = luma(rgb_at(&pm, layout.polar(135.0, 66.0)));
        assert!(far > below, "the shadow fades out");
    }
}

#[test]
fn the_dark_theme_is_charcoal_and_the_light_theme_light_grey() {
    let layout = Layout::new(2.0);
    let dark = render(2.0, Theme::DARK, &scene(0.0));
    let light = render(2.0, Theme::LIGHT, &scene(0.0));
    let face = |pm: &Pixmap| luma(rgb_at(pm, layout.polar(100.0, 30.0)));
    assert!(face(&dark) < 100.0 && face(&light) > 170.0);
}

#[test]
fn the_preview_glyph_changes_with_its_size() {
    let mut small = scene(0.0);
    small.glyph_px = 16;
    let mut large = small;
    large.glyph_px = 40;
    let changed = |a: &Pixmap, b: &Pixmap| a.data() != b.data();
    assert!(changed(
        &render(1.0, Theme::DARK, &small),
        &render(1.0, Theme::DARK, &large)
    ));
}

#[test]
fn a_zero_sized_glyph_does_not_break_the_picture() {
    let mut s = scene(0.0);
    s.glyph_px = 0;
    assert_eq!(render(1.0, Theme::DARK, &s).width(), 280);
}

#[test]
fn a_frame_from_the_cache_equals_a_full_render() {
    for theme in THEMES {
        let layout = Layout::new(1.5);
        let renderer = Renderer::new(layout, theme).expect("renderer");
        for angle in [0.0, 40.0, -77.0] {
            let s = scene(angle);
            assert_eq!(
                renderer.frame(&s).data(),
                render_dial(layout, theme, &s).expect("pixmap").data()
            );
        }
    }
}

#[test]
fn the_per_frame_time_stays_under_12_ms_at_100_and_200_percent_dpi() {
    // The 12 ms budget is for the release profile; an unoptimized build only gets a sanity limit.
    let limit_ms = if cfg!(debug_assertions) { 150.0 } else { 12.0 };
    for (dpi, scale) in [(96, 1.0), (192, 2.0)] {
        let layout = Layout::new(scale);
        let renderer = Renderer::new(layout, Theme::DARK).expect("renderer");
        let s = Scene {
            flash: Some(Flash {
                percent: 115,
                level: 0.8,
            }),
            ..scene(angle_of_percent(110.0) + 3.0)
        };
        let mut times: Vec<f32> = (0..30)
            .map(|_| {
                let start = Instant::now();
                std::hint::black_box(renderer.frame(&s));
                start.elapsed().as_secs_f32() * 1000.0
            })
            .collect();
        times.sort_by(f32::total_cmp);
        let median = times[times.len() / 2];
        let worst = times[times.len() - 1];
        println!("frame time at {dpi} dpi: median {median:.2} ms, worst {worst:.2} ms");
        assert!(
            median < limit_ms,
            "{dpi} dpi: median {median} ms, limit {limit_ms} ms"
        );
    }
}
