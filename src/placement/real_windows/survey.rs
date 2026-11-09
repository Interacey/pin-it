//! Spacing survey of every visible captioned window: the placement's pin rect against the caption-button
//! group read from the strip pixels. Set `PLACE_OUT` to the output directory.
//! Run with `cargo test spacing_survey -- --ignored --nocapture`.

use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GA_ROOT, GWL_STYLE, GetAncestor, GetWindowLongW, IsIconic, IsWindowVisible,
    WS_CAPTION,
};
use windows::core::BOOL;

use super::*;
use crate::win32::{BASE_DPI, class_name, is_cloaked};
use crate::windows_filter::is_shell_class;

/// Columns closer than this many dp belong to one glyph.
const GLYPH_MERGE_DP: i32 = 3;
/// How many button widths left of the visible right edge glyphs are searched.
const GROUP_SEARCH_BUTTONS: i32 = 3;
/// A glyph counts as the one of a button when its center is this close to the button middle.
const GLYPH_CENTER_TOLERANCE_PX: i32 = 4;
/// Tolerance in pixels when comparing gaps with the spec.
const ROUNDING_PX: i32 = 1;

/// Every visible, non-minimized, uncloaked root window that has a title bar.
fn captioned_windows() -> Vec<isize> {
    unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: lparam is the &mut Vec passed below.
        let list = unsafe { &mut *(lparam.0 as *mut Vec<isize>) };
        if is_candidate(hwnd) {
            list.push(hwnd.0 as isize);
        }
        true.into()
    }
    let mut list: Vec<isize> = Vec::new();
    // SAFETY: the callback only uses lparam as the Vec above.
    let _ = unsafe { EnumWindows(Some(collect), LPARAM(&mut list as *mut _ as isize)) };
    list
}

fn is_candidate(w: HWND) -> bool {
    // SAFETY: plain queries on a possibly stale handle.
    let (visible, iconic, root, style) = unsafe {
        (
            IsWindowVisible(w).as_bool(),
            IsIconic(w).as_bool(),
            GetAncestor(w, GA_ROOT) == w,
            GetWindowLongW(w, GWL_STYLE) as u32,
        )
    };
    visible
        && !iconic
        && root
        && style & WS_CAPTION.0 == WS_CAPTION.0
        && !is_shell_class(&class_name(w))
        && !is_cloaked(w)
}
/// Column ranges (window-relative, half-open) of non-background pixels, glyph pixels merged when close.
fn occupied_runs(scan: &TitleBarScan, merge: usize) -> Vec<(i32, i32)> {
    let free = free_columns(scan, BACKGROUND_TOLERANCE);
    let mut runs: Vec<(i32, i32)> = Vec::new();
    for x in (0..free.len()).filter(|&x| !free[x]) {
        let x = x as i32;
        match runs.last_mut() {
            Some(last) if x <= last.1 + merge as i32 => last.1 = x + 1,
            _ => runs.push((x, x + 1)),
        }
    }
    runs
}

/// Left edge of the caption-button group measured from the glyphs: buttons are `button` wide, every glyph
/// sits in the middle of its button, counted from the visible right edge `right`. A glyph counts when its
/// center is within `GLYPH_CENTER_TOLERANCE_PX` of the middle of the next button; the first button without
/// one ends the group. `None` when the close button has no glyph.
fn glyph_group_left(runs: &[(i32, i32)], right: i32, button: i32) -> Option<i32> {
    let in_button = |k: i32| {
        let middle = right - k * button - button / 2;
        runs.iter()
            .any(|r| ((r.0 + r.1) / 2 - middle).abs() <= GLYPH_CENTER_TOLERANCE_PX)
    };
    let buttons = (0..GROUP_SEARCH_BUTTONS)
        .take_while(|&k| in_button(k))
        .count() as i32;
    (buttons > 0).then_some(right - buttons * button)
}

/// End of the nearest occupied run left of the pin and start of the nearest one right of it.
fn neighbours(runs: &[(i32, i32)], x: i32, size: i32) -> (Option<i32>, Option<i32>) {
    let left = runs.iter().filter(|r| r.1 <= x).map(|r| r.1).max();
    let right = runs.iter().filter(|r| r.0 >= x + size).map(|r| r.0).min();
    (left, right)
}

fn px_to_dp(px: i32, dpi: u32) -> f32 {
    px as f32 * BASE_DPI as f32 / dpi as f32
}

/// The measured facts of one window, shared by the before and after rows.
struct Truth {
    runs: Vec<(i32, i32)>,
    real_edge: Option<i32>,
    right: i32,
}

fn truth_of(case: &Case) -> Truth {
    let dpi = case.def.dpi;
    let right = case.def.group_right;
    let runs = occupied_runs(&case.scan, scale_dp(GLYPH_MERGE_DP, dpi) as usize);
    let button = scale_dp(FALLBACK_BUTTON_WIDTH_DP, dpi);
    Truth {
        real_edge: glyph_group_left(&runs, right, button),
        runs,
        right,
    }
}

fn input_of(case: &Case, group_right: i32) -> PlacementInput {
    let (l, t, r, b) = case.def.window_rect;
    PlacementInput {
        slot: case.slot,
        estimated: case.def.estimated,
        dpi: case.def.dpi,
        window_origin: (l, t),
        window_size: (r - l, b - t),
        caption_hint_px: caption_hint_px(case.def.slot.size, case.def.dpi),
        group_right,
        waited: SCAN_WAIT,
    }
}

/// One table row: the chosen pin against the measured group and the measured neighbours.
fn report(case: &Case, truth: &Truth, placed: Placement) -> String {
    let (l, _, _, _) = case.def.window_rect;
    let dpi = case.def.dpi;
    let gap = scale_dp(GAP_TO_CAPTION_BUTTONS_DP, dpi);
    let margin = scale_dp(CONTENT_MARGIN_DP, dpi);
    let Placement::Show(p) = placed else {
        return format!("{placed:?}");
    };
    let pin = (p.x - l, p.x - l + p.size);
    let (left, right_neighbour) = neighbours(&truth.runs, pin.0, p.size);
    let gap_left = left.map(|c| pin.0 - c);
    let Some(edge) = truth.real_edge else {
        return format!("pin={pin:?} group edge unknown");
    };
    let to_group = edge - pin.1;
    let content_between = right_neighbour.is_some_and(|c| c < edge);
    let gap_right = right_neighbour.filter(|&c| c < edge).map(|c| c - pin.1);
    let mut off = Vec::new();
    if to_group < 0 {
        off.push("overlaps group");
    } else if !content_between && (to_group - gap).abs() > ROUNDING_PX {
        off.push("gap to group != 4dp");
    }
    if gap_right.is_some_and(|g| (g - gap).abs() > ROUNDING_PX) {
        off.push("gap to content right != 4dp");
    }
    if gap_left.is_some_and(|g| g < margin - ROUNDING_PX) {
        off.push("left margin < 4dp");
    }
    let verdict = if off.is_empty() {
        "ok".to_string()
    } else {
        off.join(", ")
    };
    format!(
        "pin={pin:?} gap_group={:.1}dp{} gap_right={} gap_left={}dp -> {verdict}",
        px_to_dp(to_group, dpi),
        if content_between {
            " (content between)"
        } else {
            ""
        },
        gap_right.map_or("-".to_string(), |g| format!("{:.1}dp", px_to_dp(g, dpi))),
        gap_left.map_or("-".to_string(), |g| format!("{:.1}", px_to_dp(g, dpi))),
    )
}

#[test]
#[ignore]
fn spacing_survey() {
    // SAFETY: process-wide DPI mode, set before any window is measured.
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let out = std::path::PathBuf::from(std::env::var("PLACE_OUT").expect("set PLACE_OUT"));
    std::fs::create_dir_all(&out).unwrap();
    for (i, hwnd) in captioned_windows().into_iter().enumerate() {
        let Some(mut case) = analyze(hwnd) else {
            println!("{hwnd:#x} skipped (hung, no placement or no scan)");
            continue;
        };
        let truth = truth_of(&case);
        let l = case.def.window_rect.0;
        println!(
            "s{i} {} {} dpi={} {} dwm_edge={} real_edge={:?} visible_right={}",
            exe_name(hwnd).unwrap_or_default(),
            class_name(to_hwnd(hwnd)),
            case.def.dpi,
            if case.def.estimated {
                "estimated"
            } else {
                "native"
            },
            case.slot.x + case.slot.size + scale_dp(GAP_TO_CAPTION_BUTTONS_DP, case.def.dpi) - l,
            truth.real_edge,
            truth.right,
        );
        for (name, group_right) in [("before", 0), ("after ", case.def.group_right)] {
            let placed = decide(&input_of(&case, group_right), ScanState::Ready(&case.scan));
            println!("   {name}: {}", report(&case, &truth, placed));
            if group_right != 0 {
                case.placed = placed;
            }
        }
        case.group_mark = truth.real_edge.map(|e| e + l);
        save_scan_strip(&case, &out.join(format!("s{i}.png")));
    }
}
