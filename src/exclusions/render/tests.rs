use super::*;
use crate::exclusions::colors::{TRASH_HOVER_DARK, TRASH_HOVER_LIGHT, TRASH_REST};

const ACCENT: [u8; 3] = BRAND_ACCENT;

fn entries(count: usize) -> Vec<String> {
    const NAMES: [&str; 3] = ["chrome.exe", "code.exe", "notepad.exe"];
    (0..count)
        .map(|i| {
            NAMES
                .get(i)
                .map_or_else(|| format!("program{i:02}.exe"), |n| (*n).to_owned())
        })
        .collect()
}

fn list_of(scale: f32, count: usize) -> ListState {
    ListState::new(&entries(count), Layout::new(scale))
}

fn render(scale: f32, theme: Theme, list: &ListState, input: InputLook) -> Pixmap {
    let scene = Scene { theme, list, input };
    render_window(&Layout::new(scale), &scene).expect("pixmap")
}

fn rgb_at(pm: &Pixmap, point: (f32, f32)) -> [u8; 3] {
    let p = pm
        .pixel(point.0.round() as u32, point.1.round() as u32)
        .expect("pixel inside");
    [p.red(), p.green(), p.blue()]
}

fn count_of(pm: &Pixmap, rgb: [u8; 3]) -> usize {
    pm.pixels()
        .iter()
        .filter(|p| [p.red(), p.green(), p.blue()] == rgb)
        .count()
}

/// A point inside row `index`, left of the name area's text start so no icon is near.
fn row_point(list: &ListState, layout: &Layout, index: usize) -> (f32, f32) {
    let row = list.row_area(index);
    (row.x + layout.px(6.0), row.y + row.h - layout.px(3.0))
}

#[test]
fn the_picture_follows_the_scale() {
    for (scale, size) in [(1.0, (320, 400)), (1.5, (480, 600)), (2.0, (640, 800))] {
        let pm = render(scale, Theme::DARK, &list_of(scale, 3), InputLook::Idle);
        assert_eq!((pm.width(), pm.height()), size);
    }
}

#[test]
fn every_pixel_is_premultiplied_and_opaque() {
    for theme in [Theme::DARK, Theme::LIGHT] {
        let mut list = list_of(1.5, 20);
        list.select(2);
        list.set_hover(Some(Hit::Trash(1)));
        list.highlight("code.exe");
        for look in [InputLook::Idle, InputLook::Focused, InputLook::Invalid] {
            let pm = render(1.5, theme, &list, look);
            for px in pm.pixels() {
                assert_eq!(px.alpha(), 255);
                assert!(px.red() <= px.alpha() && px.green() <= px.alpha());
            }
        }
    }
}

#[test]
fn background_and_border_use_the_theme() {
    let list = list_of(1.0, 0);
    for theme in [Theme::DARK, Theme::LIGHT] {
        let pm = render(1.0, theme, &list, InputLook::Idle);
        assert_eq!(rgb_at(&pm, (160.0, 390.0)), theme.background);
        assert_eq!(rgb_at(&pm, (0.0, 200.0)), theme.hairline);
    }
}

#[test]
fn an_empty_list_draws_no_trash_icons_and_no_indicator() {
    let pm = render(2.0, Theme::DARK, &list_of(2.0, 0), InputLook::Idle);
    assert_eq!(count_of(&pm, TRASH_REST), 0);
    let track = Layout::new(2.0).scrollbar_track();
    assert_eq!(
        rgb_at(&pm, (track.x + 2.0, track.y + 20.0)),
        Theme::DARK.background
    );
}

#[test]
fn every_visible_row_gets_a_trash_icon_in_the_rest_color() {
    let layout = Layout::new(4.0);
    let list = list_of(4.0, 3);
    let pm = render(4.0, Theme::DARK, &list, InputLook::Idle);
    for index in 0..3 {
        let icon = layout.trash_icon(&list.row_area(index));
        let region = |pm: &Pixmap| {
            let mut n = 0;
            for y in icon.y as u32..icon.bottom() as u32 {
                for x in icon.x as u32..icon.right() as u32 {
                    if rgb_at(pm, (x as f32, y as f32)) == TRASH_REST {
                        n += 1;
                    }
                }
            }
            n
        };
        assert!(region(&pm) > 300, "row {index}");
    }
}

#[test]
fn the_hovered_trash_turns_red_in_the_color_of_the_theme_and_only_that_one() {
    for (theme, red) in [
        (Theme::DARK, TRASH_HOVER_DARK),
        (Theme::LIGHT, TRASH_HOVER_LIGHT),
    ] {
        let layout = Layout::new(4.0);
        let mut list = list_of(4.0, 3);
        list.set_hover(Some(Hit::Trash(1)));
        let pm = render(4.0, theme, &list, InputLook::Idle);
        let icon = |i| layout.trash_icon(&list.row_area(i));
        let count_in = |area: Area, rgb| {
            (area.y as u32..area.bottom() as u32)
                .flat_map(|y| (area.x as u32..area.right() as u32).map(move |x| (x, y)))
                .filter(|&(x, y)| rgb_at(&pm, (x as f32, y as f32)) == rgb)
                .count()
        };
        assert!(count_in(icon(1), red) > 300, "hovered icon is red");
        assert_eq!(count_in(icon(1), TRASH_REST), 0, "and not grey");
        assert!(count_in(icon(0), TRASH_REST) > 300, "the others stay grey");
        assert_eq!(count_in(icon(2), red), 0);
    }
}

#[test]
fn hovering_the_name_of_a_row_leaves_its_icon_grey_but_lights_the_row() {
    let layout = Layout::new(4.0);
    let mut list = list_of(4.0, 3);
    list.set_hover(Some(Hit::Row(1)));
    let pm = render(4.0, Theme::DARK, &list, InputLook::Idle);
    assert_eq!(rgb_at(&pm, row_point(&list, &layout, 1)), Theme::DARK.strip);
    assert_eq!(
        rgb_at(&pm, row_point(&list, &layout, 0)),
        Theme::DARK.background
    );
    assert_eq!(count_of(&pm, TRASH_HOVER_DARK), 0);
}

#[test]
fn the_selected_row_is_one_step_stronger_and_carries_the_accent_mark() {
    let layout = Layout::new(4.0);
    let mut list = list_of(4.0, 3);
    list.select(2);
    for theme in [Theme::DARK, Theme::LIGHT] {
        let pm = render(4.0, theme, &list, InputLook::Idle);
        assert_eq!(rgb_at(&pm, row_point(&list, &layout, 2)), theme.track);
        let mark = layout.select_mark(&list.row_area(2));
        let middle = (mark.x + mark.w / 2.0, mark.y + mark.h / 2.0);
        assert_eq!(rgb_at(&pm, middle), ACCENT);
    }
}

#[test]
fn a_flash_leans_the_row_toward_the_accent_and_fades_back() {
    let layout = Layout::new(4.0);
    let mut list = list_of(4.0, 3);
    list.highlight("code.exe");
    let point = row_point(&list, &layout, 1);
    let lit = rgb_at(&render(4.0, Theme::DARK, &list, InputLook::Idle), point);
    let bg = Theme::DARK.background;
    assert!(
        lit[0] > bg[0] + 30 && lit[0] > lit[1] + 25 && lit[0] > lit[2] + 10,
        "{lit:?} vs {bg:?}"
    );
    list.tick(0.3);
    let half = rgb_at(&render(4.0, Theme::DARK, &list, InputLook::Idle), point);
    assert!(half[0] > bg[0] && half[0] < lit[0]);
    list.tick(1.0);
    assert_eq!(
        rgb_at(&render(4.0, Theme::DARK, &list, InputLook::Idle), point),
        bg
    );
}

#[test]
fn the_scroll_indicator_appears_only_when_the_list_overflows() {
    let few = list_of(2.0, 5);
    assert!(!few.overflows());
    let many = list_of(2.0, 20);
    let thumb = many.scrollbar_thumb().expect("thumb");
    let middle = (thumb.x + thumb.w / 2.0, thumb.y + thumb.h / 2.0);
    let pm_many = render(2.0, Theme::LIGHT, &many, InputLook::Idle);
    assert_eq!(rgb_at(&pm_many, middle), Theme::LIGHT.tick_idle);
    let pm_few = render(2.0, Theme::LIGHT, &few, InputLook::Idle);
    assert_eq!(rgb_at(&pm_few, middle), Theme::LIGHT.background);
}

#[test]
fn rows_cut_off_by_the_list_do_not_paint_into_the_gap_or_the_header() {
    let layout = Layout::new(2.0);
    let mut list = list_of(2.0, 20);
    list.scroll_by(layout.row_height() * 0.5);
    list.set_hover(Some(Hit::Row(0)));
    let pm = render(2.0, Theme::DARK, &list, InputLook::Idle);
    let view = layout.list();
    let probe_y = |y: f32| (view.x + 40.0, y);
    // Row 0 is half scrolled away: its hover tone shows inside the list ...
    assert_eq!(rgb_at(&pm, probe_y(view.y + 4.0)), Theme::DARK.strip);
    // ... but not above it, and the last visible row ends at the list's bottom edge.
    assert_eq!(rgb_at(&pm, probe_y(view.y - 4.0)), Theme::DARK.background);
    let gap_icons = (view.bottom() as u32..layout.input_box().y as u32)
        .flat_map(|y| (0..pm.width()).map(move |x| (x, y)))
        .filter(|&(x, y)| rgb_at(&pm, (x as f32, y as f32)) == TRASH_REST)
        .count();
    assert_eq!(gap_icons, 0, "no icon below the list");
}

#[test]
fn the_input_field_shows_idle_focus_and_error_through_its_outline() {
    let layout = Layout::new(2.0);
    let list = list_of(2.0, 3);
    let field = layout.input_box();
    let left_edge = (field.x + 1.0, field.y + field.h / 2.0);
    let inside = (field.x + 40.0, field.y + field.h / 2.0);
    for theme in [Theme::DARK, Theme::LIGHT] {
        let at = |look| render(2.0, theme, &list, look);
        assert_eq!(rgb_at(&at(InputLook::Idle), left_edge), theme.hairline);
        assert_eq!(rgb_at(&at(InputLook::Focused), left_edge), ACCENT);
        assert_eq!(rgb_at(&at(InputLook::Invalid), left_edge), theme.error);
        for look in [InputLook::Idle, InputLook::Focused, InputLook::Invalid] {
            assert_eq!(rgb_at(&at(look), inside), theme.surface, "{look:?}");
        }
    }
}

/// The focus ring is 1.5 dp wide: at 4x that is 6 pixels, drawn inside the field's edge.
#[test]
fn the_focus_ring_is_the_crisp_brand_color_of_1_5_dp() {
    let layout = Layout::new(4.0);
    let list = list_of(4.0, 3);
    let field = layout.input_box();
    let mid_y = field.y + field.h / 2.0;
    for theme in [Theme::DARK, Theme::LIGHT] {
        let pm = render(4.0, theme, &list, InputLook::Focused);
        let across: Vec<[u8; 3]> = (0..12)
            .map(|i| rgb_at(&pm, (field.x + i as f32, mid_y)))
            .collect();
        let ring_pixels = across.iter().take_while(|&&c| c == BRAND_ACCENT).count();
        assert_eq!(ring_pixels, 6, "{across:?}");
        assert_eq!(across[8], theme.surface, "the field itself is untouched");
    }
}

#[test]
fn the_focus_ring_glows_around_the_field_on_both_themes_and_the_glow_fades_out() {
    let layout = Layout::new(2.0);
    let list = list_of(2.0, 3);
    let field = layout.input_box();
    let mid_y = field.y + field.h / 2.0;
    for theme in [Theme::DARK, Theme::LIGHT] {
        let idle = render(2.0, theme, &list, InputLook::Idle);
        let focused = render(2.0, theme, &list, InputLook::Focused);
        let change = |dx: f32| {
            let at = (field.x - dx, mid_y);
            distance(rgb_at(&focused, at), rgb_at(&idle, at))
        };
        assert!(
            change(2.0) > 12,
            "{theme:?} next to the ring: {}",
            change(2.0)
        );
        assert!(change(2.0) > change(8.0), "falls off");
        assert_eq!(change(24.0), 0, "nothing beyond the reach of the 6 dp glow");
        let invalid = render(2.0, theme, &list, InputLook::Invalid);
        let at = (field.x - 2.0, mid_y);
        assert_eq!(rgb_at(&invalid, at), rgb_at(&idle, at), "only focus glows");
    }
}

#[test]
fn the_dark_glow_brightens_and_the_light_glow_tints() {
    let layout = Layout::new(2.0);
    let list = list_of(2.0, 3);
    let field = layout.input_box();
    let at = (field.x - 2.0, field.y + field.h / 2.0);
    let lum = |c: [u8; 3]| u32::from(c[0]) + u32::from(c[1]) + u32::from(c[2]);
    let (dark_idle, dark_focus) = (
        rgb_at(&render(2.0, Theme::DARK, &list, InputLook::Idle), at),
        rgb_at(&render(2.0, Theme::DARK, &list, InputLook::Focused), at),
    );
    assert!(lum(dark_focus) > lum(dark_idle) + 30);
    let (light_idle, light_focus) = (
        rgb_at(&render(2.0, Theme::LIGHT, &list, InputLook::Idle), at),
        rgb_at(&render(2.0, Theme::LIGHT, &list, InputLook::Focused), at),
    );
    assert!(
        light_focus[1] + 10 < light_idle[1],
        "{light_idle:?} -> {light_focus:?}"
    );
}

fn distance(a: [u8; 3], b: [u8; 3]) -> i32 {
    a.iter()
        .zip(b)
        .map(|(&x, y)| (i32::from(x) - i32::from(y)).abs())
        .max()
        .unwrap_or(0)
}

#[test]
fn the_error_color_reads_on_both_backgrounds() {
    // WCAG-style contrast of the error text against the window background.
    fn lum(c: [u8; 3]) -> f32 {
        let f = |v: u8| {
            let v = f32::from(v) / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * f(c[0]) + 0.7152 * f(c[1]) + 0.0722 * f(c[2])
    }
    for theme in [Theme::DARK, Theme::LIGHT] {
        let (a, b) = (lum(theme.error), lum(theme.background));
        let ratio = (a.max(b) + 0.05) / (a.min(b) + 0.05);
        assert!(ratio >= 4.5, "contrast {ratio}");
    }
}

#[test]
fn mixing_is_linear_and_exact_at_the_ends() {
    assert_eq!(mix([0, 100, 200], [100, 200, 0], 0.0), [0, 100, 200]);
    assert_eq!(mix([0, 100, 200], [100, 200, 0], 1.0), [100, 200, 0]);
    assert_eq!(mix([0, 100, 200], [100, 200, 0], 0.5), [50, 150, 100]);
}

/// Writes the pictures for a visual check when `PIN_IT_EXCLUSIONS_PNG_DIR` is set.
#[test]
fn write_pictures_for_review() {
    let Some(dir) = std::env::var_os("PIN_IT_EXCLUSIONS_PNG_DIR") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).expect("png dir");
    for (name, theme) in [("dark", Theme::DARK), ("light", Theme::LIGHT)] {
        let mut list = list_of(2.0, 20);
        list.set_hover(Some(Hit::Trash(1)));
        list.select(2);
        let pm = render(2.0, theme, &list, InputLook::Focused);
        pm.save_png(dir.join(format!("pixmap-{name}.png")))
            .expect("save png");
    }
}
