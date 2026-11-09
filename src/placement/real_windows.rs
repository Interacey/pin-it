//! Read-only check on real windows: scan, placement and a PNG of the title-bar strip with the default slot (blue)
//! and the chosen slot (red). Set `PLACE_OUT` to the output directory.
//! Run with `cargo test real_windows_placement -- --ignored --nocapture`.

use std::ffi::c_void;

use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, SRCCOPY, SelectObject,
};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GA_ROOT, GetAncestor, IsHungAppWindow, IsZoomed, WindowFromPoint,
};

use super::*;
use crate::color::sample_title_bar;
use crate::win32::to_hwnd;
use crate::windows_filter::{DefaultPlacement, default_placement, enumerate_pinnable, exe_name};

const MAX_WINDOWS: usize = 12;
/// Rows above and below the slot that belong to the caption row.
const STRIP_PAD: i32 = 4;
/// Pixels left of the default slot kept in the crop.
const CROP_LEFT: i32 = 700;
/// Nearest-neighbour zoom of the saved PNG.
const ZOOM: i32 = 3;
/// Distance left of the slot at which the window must still own the title bar pixels.
const OWNERSHIP_PROBE_PX: i32 = 40;
const DEFAULT_SLOT_COLOR: [u8; 3] = [0, 80, 255];
const CHOSEN_SLOT_COLOR: [u8; 3] = [255, 0, 0];

/// BGRA pixels of the screen rect, top-down.
fn grab(left: i32, top: i32, w: i32, h: i32) -> Option<Vec<u8>> {
    // SAFETY: GDI objects are released before returning.
    unsafe {
        let screen = GetDC(None);
        let dc = CreateCompatibleDC(Some(screen));
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(dc, bmp.into());
        let ok = BitBlt(dc, 0, 0, w, h, Some(screen), left, top, SRCCOPY).is_ok();
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut buf = vec![0u8; (w * h * 4) as usize];
        let lines = GetDIBits(
            dc,
            bmp,
            0,
            h as u32,
            Some(buf.as_mut_ptr() as *mut c_void),
            &mut info,
            DIB_RGB_COLORS,
        );
        SelectObject(dc, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(dc);
        ReleaseDC(None, screen);
        (ok && lines > 0).then_some(buf)
    }
}

/// A square frame in an RGBA buffer of `w` x `h` pixels.
struct Frame {
    x: i32,
    y: i32,
    size: i32,
    color: [u8; 3],
}

fn outline(rgba: &mut [u8], (w, h): (i32, i32), frame: &Frame) {
    let Frame { x, y, size, color } = *frame;
    for i in 0..size {
        for (px, py) in [
            (x + i, y),
            (x + i, y + size - 1),
            (x, y + i),
            (x + size - 1, y + i),
        ] {
            if (0..w).contains(&px) && (0..h).contains(&py) {
                let at = ((py * w + px) * 4) as usize;
                rgba[at..at + 3].copy_from_slice(&color);
                rgba[at + 3] = 255;
            }
        }
    }
}

fn columns_all_free(scan: &TitleBarScan, from: i32, to: i32) -> bool {
    (from.max(0)..to).all(|x| {
        scan.rows.iter().all(|row| {
            row.get(x as usize)
                .is_some_and(|&p| near(p, scan.background, BACKGROUND_TOLERANCE))
        })
    })
}

/// True when the window itself owns the pixels at the slot and just left of it.
fn title_bar_visible(window: HWND, slot: PinSlot) -> bool {
    let owns = |x: i32| {
        let at = POINT {
            x,
            y: slot.y + slot.size / 2,
        };
        // SAFETY: read-only hit test of a screen point.
        let hit = unsafe { WindowFromPoint(at) };
        // SAFETY: read-only query of the hit window.
        !hit.0.is_null() && unsafe { GetAncestor(hit, GA_ROOT) } == window
    };
    owns(slot.x + slot.size / 2) && owns(slot.x - OWNERSHIP_PROBE_PX)
}

/// One window with its scan and the placement decided for it.
struct Case {
    hwnd: isize,
    def: DefaultPlacement,
    scan: std::sync::Arc<TitleBarScan>,
    slot: PinSlot,
    placed: Placement,
    /// The title bar is on screen, so a screen grab shows it; otherwise the PNG is drawn from the scan.
    on_screen: bool,
    /// Screen x of the caption-button group's real left edge, drawn as a green line in scan strips.
    group_mark: Option<i32>,
}

fn analyze(hwnd: isize) -> Option<Case> {
    let window = to_hwnd(hwnd);
    // SAFETY: read-only query.
    if unsafe { IsHungAppWindow(window) }.as_bool() {
        return None;
    }
    let def = default_placement(hwnd)?;
    let on_screen = title_bar_visible(window, def.slot);
    let Some(sample) = sample_title_bar(hwnd) else {
        println!("{hwnd:#x}: no sample");
        return None;
    };
    let Some(scan) = sample.scan else {
        println!("{hwnd:#x}: no scan");
        return None;
    };
    let (l, t, r, b) = def.window_rect;
    let input = PlacementInput {
        slot: apply_user_scale(def.slot, 100, def.dpi),
        estimated: def.estimated,
        dpi: def.dpi,
        window_origin: (l, t),
        window_size: (r - l, b - t),
        caption_hint_px: caption_hint_px(def.slot.size, def.dpi),
        group_right: def.group_right,
        waited: SCAN_WAIT,
    };
    let slot = input.slot;
    let placed = decide(&input, ScanState::Ready(&scan));
    Some(Case {
        hwnd,
        def,
        scan,
        slot,
        placed,
        on_screen,
        group_mark: None,
    })
}

fn print_report(case: &Case) {
    let Case {
        hwnd,
        def,
        scan,
        slot,
        placed,
        on_screen,
        ..
    } = case;
    let (l, t, r, b) = def.window_rect;
    let free_cols = free_columns(scan, BACKGROUND_TOLERANCE)
        .iter()
        .filter(|&&f| f)
        .count();
    let default_overlaps = !columns_all_free(scan, slot.x - l, slot.x - l + slot.size);
    let new_overlaps = match placed {
        Placement::Show(p) => !columns_all_free(scan, p.x - l, p.x - l + p.size),
        _ => false,
    };
    // SAFETY: read-only query.
    let maximized = unsafe { IsZoomed(to_hwnd(*hwnd)) }.as_bool();
    println!(
        "{hwnd:#x} exe={:?} maximized={maximized} estimated={} dpi={} window=({l},{t},{r},{b}) default={:?} bg={:?} scan={}x{} free_cols={free_cols} \
         default_overlaps_content={default_overlaps} placement={placed:?} new_slot_overlaps_content={new_overlaps} on_screen={on_screen}          ink={:?} accent={:?} head={:?}",
        exe_name(*hwnd),
        def.estimated,
        def.dpi,
        def.slot,
        scan.background,
        scan.width(),
        scan.rows.len(),
        scan.caption_ink,
        scan.accent,
        crate::color::ensure_visible_with(
            scan.accent.unwrap_or(scan.background),
            scan.background,
            crate::overlay::MIN_HEAD_CONTRAST,
        ),
    );
}

/// Saves the strip of the caption rows with both slots outlined, magnified by `ZOOM`; false when the screen grab failed.
fn save_strip(case: &Case, path: &std::path::Path) -> bool {
    if !case.on_screen {
        return save_scan_strip(case, path);
    }
    let (l, _, r, _) = case.def.window_rect;
    let s = case.def.slot;
    let (strip_top, strip_h) = (s.y - STRIP_PAD, s.size + 2 * STRIP_PAD);
    let crop_left = (s.x - CROP_LEFT).max(l);
    let strip_w = r - crop_left;
    let Some(mut rgba) = grab(crop_left, strip_top, strip_w, strip_h) else {
        return false;
    };
    for px in rgba.chunks_exact_mut(4) {
        px.swap(0, 2);
        px[3] = 255;
    }
    let dims = (strip_w, strip_h);
    print_screen_colors(case, &rgba, strip_w);
    let slot = case.slot;
    let frame = |slot: PinSlot, color| Frame {
        x: slot.x - crop_left,
        y: slot.y - strip_top,
        size: slot.size,
        color,
    };
    outline(&mut rgba, dims, &frame(slot, DEFAULT_SLOT_COLOR));
    if let Placement::Show(p) = case.placed {
        outline(&mut rgba, dims, &frame(p, CHOSEN_SLOT_COLOR));
    }
    zoomed_pixmap(&rgba, dims).save_png(path).unwrap();
    true
}

/// The most frequent color of the screen strip and its pixel farthest from that color within the last 92 px
/// (minimize and maximize symbols or the close glyph), to compare with the scan's bar color and ink.
fn print_screen_colors(case: &Case, rgba: &[u8], width: i32) {
    let px = |i: usize| [rgba[i * 4], rgba[i * 4 + 1], rgba[i * 4 + 2]];
    let mut counts = std::collections::HashMap::new();
    for i in 0..rgba.len() / 4 {
        *counts.entry(px(i)).or_insert(0u32) += 1;
    }
    let bar = counts
        .into_iter()
        .max_by_key(|&(c, n)| (n, c))
        .map(|(c, _)| c);
    let Some(bar) = bar else { return };
    let far = |c: [u8; 3]| {
        c.iter()
            .zip(bar)
            .map(|(&a, b)| a.abs_diff(b) as u32)
            .sum::<u32>()
    };
    let rows = rgba.len() as i32 / 4 / width;
    let ink = (0..rows)
        .flat_map(|y| (width - 92..width).map(move |x| (x, y)))
        .map(|(x, y)| px((y * width + x) as usize))
        .max_by_key(|&c| far(c));
    println!(
        "  {:#x} screen: bar={bar:?} (scan bar {:?}) strongest symbol pixel in the last 92 px={ink:?} (scan ink {:?})",
        case.hwnd, case.scan.background, case.scan.caption_ink
    );
}

/// The strip drawn from the scan rows (the window is covered, so the screen shows something else): the
/// scanned rows are centered in a strip as high as the slot, both slots are outlined.
fn save_scan_strip(case: &Case, path: &std::path::Path) -> bool {
    let (l, _, r, _) = case.def.window_rect;
    let s = case.def.slot;
    let crop_left = (s.x - CROP_LEFT).max(l);
    let (strip_w, strip_h) = (r - crop_left, s.size + 2 * STRIP_PAD);
    let mut rgba = Vec::with_capacity((strip_w * strip_h * 4) as usize);
    let first_row = ((strip_h - case.scan.rows.len() as i32) / 2).max(0);
    for y in 0..strip_h {
        for x in 0..strip_w {
            let row = case.scan.rows.get((y - first_row).max(-1) as usize);
            let px = row
                .and_then(|row| row.get((crop_left - l + x) as usize))
                .copied()
                .unwrap_or(case.scan.background);
            rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
        }
    }
    let dims = (strip_w, strip_h);
    let frame = |slot: PinSlot, color| Frame {
        x: slot.x - crop_left,
        y: STRIP_PAD,
        size: slot.size,
        color,
    };
    outline(&mut rgba, dims, &frame(case.slot, DEFAULT_SLOT_COLOR));
    if let Placement::Show(p) = case.placed {
        outline(&mut rgba, dims, &frame(p, CHOSEN_SLOT_COLOR));
    }
    if let Some(x) = case.group_mark {
        mark_column(&mut rgba, dims, x - crop_left);
    }
    zoomed_pixmap(&rgba, dims).save_png(path).unwrap();
    true
}

/// A green vertical line at column `x` of the strip.
fn mark_column(rgba: &mut [u8], (w, h): (i32, i32), x: i32) {
    if !(0..w).contains(&x) {
        return;
    }
    for y in 0..h {
        let at = ((y * w + x) * 4) as usize;
        rgba[at..at + 4].copy_from_slice(&[0, 200, 0, 255]);
    }
}

fn zoomed_pixmap(rgba: &[u8], (w, h): (i32, i32)) -> tiny_skia::Pixmap {
    let mut pm = tiny_skia::Pixmap::new((w * ZOOM) as u32, (h * ZOOM) as u32).unwrap();
    let pw = pm.width() as i32;
    for (i, dst) in pm.pixels_mut().iter_mut().enumerate() {
        let (x, y) = ((i as i32 % pw) / ZOOM, (i as i32 / pw) / ZOOM);
        let at = ((y * w + x) * 4) as usize;
        *dst =
            tiny_skia::ColorU8::from_rgba(rgba[at], rgba[at + 1], rgba[at + 2], 255).premultiply();
    }
    pm
}

#[test]
#[ignore]
fn real_windows_placement() {
    // SAFETY: process-wide DPI mode, set before any window is measured; failure only skews the numbers.
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let out = std::path::PathBuf::from(std::env::var("PLACE_OUT").expect("set PLACE_OUT"));
    std::fs::create_dir_all(&out).unwrap();
    let mut done = 0;
    for hwnd in enumerate_pinnable() {
        if done == MAX_WINDOWS {
            break;
        }
        let Some(case) = analyze(hwnd) else {
            continue;
        };
        print_report(&case);
        if save_strip(&case, &out.join(format!("win{done}.png"))) {
            done += 1;
        }
    }
    assert!(done > 0, "no usable foreign window");
}

mod survey;

/// Saves a magnified screen grab of the caption strip right of `crop_left`, with each of `marks` outlined in
/// its color; the strip is as high as the first mark plus `STRIP_PAD` on both sides. False when the grab failed.
pub(crate) fn save_screen_strip(
    window_rect: (i32, i32, i32, i32),
    marks: &[(PinSlot, [u8; 3])],
    path: &std::path::Path,
) -> bool {
    let (l, _, r, _) = window_rect;
    let Some(&(first, _)) = marks.first() else {
        return false;
    };
    let crop_left = (first.x - CROP_LEFT).max(l);
    let (strip_top, strip_w, strip_h) = (
        first.y - STRIP_PAD,
        r - crop_left,
        first.size + 2 * STRIP_PAD,
    );
    let Some(mut rgba) = grab(crop_left, strip_top, strip_w, strip_h) else {
        return false;
    };
    for px in rgba.chunks_exact_mut(4) {
        px.swap(0, 2);
        px[3] = 255;
    }
    for &(slot, color) in marks {
        let frame = Frame {
            x: slot.x - crop_left,
            y: slot.y - strip_top,
            size: slot.size,
            color,
        };
        outline(&mut rgba, (strip_w, strip_h), &frame);
    }
    zoomed_pixmap(&rgba, (strip_w, strip_h))
        .save_png(path)
        .unwrap();
    true
}
