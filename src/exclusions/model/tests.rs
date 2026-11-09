use super::*;

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

fn numbered(count: usize) -> Vec<String> {
    (0..count).map(|i| format!("app{i:02}.exe")).collect()
}

fn list_at(scale: f32, count: usize) -> ListState {
    ListState::new(&numbered(count), Layout::new(scale))
}

// --- normalize_entry ---

#[test]
fn valid_names_are_trimmed_and_lower_cased() {
    for (raw, expected) in [
        ("chrome.exe", "chrome.exe"),
        ("Chrome.EXE", "chrome.exe"),
        ("  notepad.exe  ", "notepad.exe"),
        ("\tcode.exe\n", "code.exe"),
        ("my app.exe", "my app.exe"),
        ("7zFM.exe", "7zfm.exe"),
        ("a.b.c", "a.b.c"),
        (".exe", ".exe"),
        ("tool.bat", "tool.bat"),
        ("ÄÖÜ.exe", "äöü.exe"),
        ("programm-ü.exe", "programm-ü.exe"),
        ("日本語.exe", "日本語.exe"),
    ] {
        assert_eq!(normalize_entry(raw).as_deref(), Ok(expected), "{raw:?}");
    }
}

#[test]
fn a_name_without_a_dot_gets_exe_appended() {
    for (raw, expected) in [
        ("chrome", "chrome.exe"),
        ("  Chrome ", "chrome.exe"),
        ("my app", "my app.exe"),
        ("日本語", "日本語.exe"),
    ] {
        assert_eq!(normalize_entry(raw).as_deref(), Ok(expected), "{raw:?}");
    }
}

#[test]
fn empty_and_blank_input_is_refused() {
    for raw in ["", " ", "   \t ", "\n"] {
        assert_eq!(normalize_entry(raw), Err(EntryError::Empty), "{raw:?}");
    }
}

#[test]
fn the_longest_name_is_255_characters() {
    let name = |len: usize| format!("{}.exe", "a".repeat(len - 4));
    assert_eq!(normalize_entry(&name(255)).map(|n| n.len()), Ok(255));
    assert_eq!(normalize_entry(&name(256)), Err(EntryError::TooLong));
    assert_eq!(normalize_entry(&name(1000)), Err(EntryError::TooLong));
}

#[test]
fn the_added_extension_counts_toward_the_limit() {
    assert!(normalize_entry(&"a".repeat(251)).is_ok());
    assert_eq!(
        normalize_entry(&"a".repeat(252)),
        Err(EntryError::TooLong),
        "252 + .exe is 256"
    );
}

#[test]
fn the_limit_counts_utf16_units_like_windows_does() {
    // U+1F600 is one char but two UTF-16 units.
    let emoji = |n: usize| format!("{}.x", "\u{1F600}".repeat(n));
    assert!(normalize_entry(&emoji(126)).is_ok(), "252 + 2 = 254 units");
    assert_eq!(normalize_entry(&emoji(127)), Err(EntryError::TooLong));
}

#[test]
fn path_separators_and_invalid_file_name_characters_are_refused() {
    for ch in ['\\', '/', ':', '*', '?', '"', '<', '>', '|'] {
        for raw in [
            format!("a{ch}b.exe"),
            format!("{ch}a.exe"),
            format!("a.exe{ch}"),
        ] {
            assert_eq!(
                normalize_entry(&raw),
                Err(EntryError::InvalidChar),
                "{raw:?}"
            );
        }
    }
    assert_eq!(
        normalize_entry("c:\\windows\\notepad.exe"),
        Err(EntryError::InvalidChar)
    );
}

#[test]
fn control_characters_are_refused() {
    for raw in [
        "a\u{0}b.exe",
        "a\tb.exe",
        "a\nb.exe",
        "a\u{7f}b.exe",
        "a\u{85}b",
    ] {
        assert_eq!(
            normalize_entry(raw),
            Err(EntryError::InvalidChar),
            "{raw:?}"
        );
    }
}

#[test]
fn a_trailing_dot_is_refused_even_with_trailing_spaces_around_it() {
    for raw in ["chrome.", "chrome. ", "chrome . ", ".", "..", "a.b."] {
        assert_eq!(
            normalize_entry(raw),
            Err(EntryError::TrailingDot),
            "{raw:?}"
        );
    }
}

#[test]
fn a_trailing_space_is_trimmed_not_kept() {
    assert_eq!(
        normalize_entry("chrome.exe   ").as_deref(),
        Ok("chrome.exe")
    );
}

#[test]
fn normalizing_twice_changes_nothing() {
    for raw in ["Chrome", "A B.EXE", "x.y"] {
        let once = normalize_entry(raw).unwrap();
        assert_eq!(normalize_entry(&once).as_deref(), Ok(once.as_str()));
    }
}

// --- list, scroll, selection ---

#[test]
fn a_short_list_does_not_scroll_and_shows_no_indicator() {
    let mut list = list_at(1.0, 3);
    list.scroll_by(500.0);
    assert_eq!(list.scroll(), 0.0);
    assert!(!list.overflows() && list.scrollbar_thumb().is_none());
    assert_eq!(list.visible(), 0..3);
}

#[test]
fn a_long_list_scrolls_within_its_bounds() {
    // 20 rows of 36 dp in a 260 dp list: 460 dp of overflow.
    let mut list = list_at(1.0, 20);
    assert!(list.overflows());
    list.scroll_by(-50.0);
    assert_eq!(list.scroll(), 0.0, "not above the first row");
    list.scroll_by(100.0);
    assert_eq!(list.scroll(), 100.0);
    list.scroll_by(10_000.0);
    assert_eq!(list.scroll(), 20.0 * 36.0 - 260.0, "not past the last row");
}

#[test]
fn the_wheel_moves_one_row_per_notch_and_down_means_towards_the_start() {
    let mut list = list_at(2.0, 20);
    list.scroll_wheel(-120);
    assert_eq!(list.scroll(), 72.0);
    list.scroll_wheel(-240);
    assert_eq!(list.scroll(), 216.0);
    list.scroll_wheel(120);
    assert_eq!(list.scroll(), 144.0);
    list.scroll_wheel(120 * 100);
    assert_eq!(list.scroll(), 0.0);
}

#[test]
fn visible_rows_follow_the_scroll_position() {
    let mut list = list_at(1.0, 20);
    assert_eq!(
        list.visible(),
        0..8,
        "7.2 rows: eight are at least partly visible"
    );
    list.scroll_by(40.0);
    assert_eq!(list.visible(), 1..9);
    list.scroll_by(10_000.0);
    assert_eq!(list.visible().end, 20);
}

#[test]
fn the_thumb_moves_along_the_track_and_never_leaves_it() {
    let mut list = list_at(1.0, 20);
    let track = list.layout.scrollbar_track();
    let top = list.scrollbar_thumb().unwrap();
    assert_eq!(top.y, track.y);
    assert!(top.h >= list.layout.scrollbar_min_thumb() && top.h < track.h);
    list.scroll_by(10_000.0);
    let bottom = list.scrollbar_thumb().unwrap();
    assert!((bottom.bottom() - track.bottom()).abs() < 0.01);
    list.scroll_by(-100.0);
    let middle = list.scrollbar_thumb().unwrap();
    assert!(middle.y > top.y && middle.bottom() < bottom.bottom());
}

#[test]
fn the_thumb_has_a_minimum_height_for_very_long_lists() {
    let list = list_at(1.0, 500);
    assert_eq!(list.scrollbar_thumb().unwrap().h, 24.0);
}

#[test]
fn down_selects_the_first_row_and_up_the_last_and_both_stop_at_the_ends() {
    let mut list = list_at(1.0, 3);
    list.select_step(1);
    assert_eq!(list.selected(), Some(0));
    list.select_step(-1);
    assert_eq!(list.selected(), Some(0));
    list.select_step(1);
    list.select_step(1);
    list.select_step(1);
    assert_eq!(list.selected(), Some(2));
    assert_eq!(list.selected_name(), Some("app02.exe"));
    list.clear_selection();
    list.select_step(-1);
    assert_eq!(
        list.selected(),
        Some(2),
        "up from nothing starts at the end"
    );
}

#[test]
fn moving_the_selection_scrolls_it_into_view() {
    let mut list = list_at(1.0, 20);
    for _ in 0..10 {
        list.select_step(1);
    }
    // Row 9 ends at 360 dp, the list shows 260 dp.
    assert_eq!(list.scroll(), 100.0);
    list.select_step(-1);
    list.select_step(-1);
    assert_eq!(list.scroll(), 100.0, "rows 7 and 8 are inside already");
    for _ in 0..8 {
        list.select_step(-1);
    }
    assert_eq!(list.scroll(), 0.0);
}

#[test]
fn selection_does_nothing_on_an_empty_list() {
    let mut list = list_at(1.0, 0);
    list.select_step(1);
    list.select_step(-1);
    list.select(0);
    assert_eq!(list.selected(), None);
}

#[test]
fn refresh_keeps_the_selection_on_its_name_and_moves_it_to_the_neighbour_when_it_is_gone() {
    let mut list = ListState::new(&names(&["a.exe", "b.exe", "c.exe"]), Layout::new(1.0));
    list.select(1);
    list.refresh(&names(&["a.exe", "b.exe", "c.exe", "d.exe"]));
    assert_eq!(list.selected_name(), Some("b.exe"));
    list.refresh(&names(&["0.exe", "a.exe", "b.exe"]));
    assert_eq!(list.selected_name(), Some("b.exe"), "follows the name");
    list.refresh(&names(&["0.exe", "a.exe", "c.exe"]));
    assert_eq!(
        list.selected_name(),
        Some("c.exe"),
        "the row that took its place"
    );
    list.refresh(&names(&["0.exe"]));
    assert_eq!(list.selected_name(), Some("0.exe"), "clamped to the end");
    list.refresh(&[]);
    assert_eq!(list.selected(), None);
}

#[test]
fn refresh_clamps_the_scroll_position_when_the_list_gets_shorter() {
    let mut list = list_at(1.0, 20);
    list.scroll_by(10_000.0);
    list.refresh(&numbered(9));
    assert_eq!(list.scroll(), 9.0 * 36.0 - 260.0);
    list.refresh(&numbered(3));
    assert_eq!(list.scroll(), 0.0);
}

// --- hit testing ---

/// The client point `dx`, `dy` pixels into row `index` of `list` (relative to the row's origin).
fn in_row(list: &ListState, index: usize, dx: f32, dy: f32) -> (i32, i32) {
    let row = list.row_area(index);
    ((row.x + dx) as i32, (row.y + dy) as i32)
}

#[test]
fn rows_and_trash_buttons_are_hit_at_several_dpi_values() {
    for dpi in [96.0_f32, 120.0, 144.0, 192.0] {
        let scale = dpi / 96.0;
        let list = list_at(scale, 5);
        let layout = Layout::new(scale);
        for index in 0..5 {
            let row = list.row_area(index);
            let trash = layout.trash_box(&row);
            let name = in_row(&list, index, layout.px(40.0), row.h / 2.0);
            assert_eq!(list.hit(name), Some(Hit::Row(index)), "dpi {dpi}");
            let center = (
                (trash.x + trash.w / 2.0) as i32,
                (trash.y + trash.h / 2.0) as i32,
            );
            assert_eq!(list.hit(center), Some(Hit::Trash(index)), "dpi {dpi}");
            let corner = (trash.x as i32, trash.y as i32);
            assert_eq!(
                list.hit(corner),
                Some(Hit::Trash(index)),
                "dpi {dpi} corner"
            );
            let left_of = (trash.x as i32 - 1, center.1);
            assert_eq!(list.hit(left_of), Some(Hit::Row(index)), "dpi {dpi} edge");
        }
    }
}

#[test]
fn points_outside_the_rows_hit_nothing() {
    let list = list_at(1.0, 3);
    let l = list.layout.list();
    let rows_bottom = (l.y + 3.0 * 36.0) as i32;
    assert_eq!(list.hit((50, rows_bottom + 4)), None, "below the last row");
    assert_eq!(list.hit((50, (l.y - 1.0) as i32)), None, "above the list");
    assert_eq!(list.hit(((l.x - 1.0) as i32, 80)), None, "left of the list");
    let row = list.row_area(0);
    assert_eq!(
        list.hit((row.right() as i32, 70)),
        None,
        "in the scroll gutter"
    );
    let input = list.layout.input_box();
    assert_eq!(
        list.hit(((input.x + 20.0) as i32, (input.y + 10.0) as i32)),
        None
    );
}

#[test]
fn hit_testing_accounts_for_the_scroll_and_the_list_clip() {
    let mut list = list_at(1.0, 20);
    list.scroll_by(36.0 * 3.0 + 10.0);
    let viewport = list.layout.list();
    let point = ((viewport.x + 60.0) as i32, (viewport.y + 5.0) as i32);
    assert_eq!(
        list.hit(point),
        Some(Hit::Row(3)),
        "10 px of row 3 scrolled away"
    );
    // Row 10 starts below the visible part of the list (260 dp) and must not be hit through
    // the input area.
    let below = ((viewport.x + 60.0) as i32, (viewport.bottom() + 2.0) as i32);
    assert_eq!(list.hit(below), None);
    let trash = list.layout.trash_box(&list.row_area(5));
    let at = ((trash.x + 5.0) as i32, (trash.y + 5.0) as i32);
    assert_eq!(list.hit(at), Some(Hit::Trash(5)));
}

#[test]
fn hover_reports_changes_only() {
    let mut list = list_at(1.0, 3);
    assert!(!list.set_hover(None));
    assert!(list.set_hover(Some(Hit::Row(1))));
    assert!(!list.set_hover(Some(Hit::Row(1))));
    assert!(list.set_hover(Some(Hit::Trash(1))));
    assert!(list.set_hover(None));
}

// --- flash ---

#[test]
fn a_highlight_flashes_the_row_fades_out_and_ends() {
    let mut list = ListState::new(&names(&["a.exe", "b.exe"]), Layout::new(1.0));
    assert!(!list.is_animating());
    list.highlight("b.exe");
    assert_eq!((list.flash_level(0), list.flash_level(1)), (0.0, 1.0));
    assert!(list.tick(0.3));
    let half = list.flash_level(1);
    assert!(half > 0.0 && half < 1.0, "{half}");
    assert!(!list.tick(0.5));
    assert_eq!(list.flash_level(1), 0.0);
    assert!(!list.is_animating());
}

#[test]
fn a_highlight_for_a_row_that_arrives_later_scrolls_to_it_once() {
    let mut list = list_at(1.0, 20);
    list.highlight("zzz.exe");
    assert_eq!(list.scroll(), 0.0, "not in the list yet");
    let mut entries = numbered(20);
    entries.push("zzz.exe".into());
    list.refresh(&entries);
    assert_eq!(
        list.scroll(),
        21.0 * 36.0 - 260.0,
        "scrolled to the new last row"
    );
    list.scroll_by(-100.0);
    list.refresh(&entries);
    assert_eq!(
        list.scroll(),
        21.0 * 36.0 - 260.0 - 100.0,
        "only the first refresh reveals"
    );
}

#[test]
fn highlighting_a_known_row_reveals_it_at_once() {
    let mut list = list_at(1.0, 20);
    list.highlight("app15.exe");
    assert_eq!(list.scroll(), 16.0 * 36.0 - 260.0);
}
