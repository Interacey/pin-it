//! Read-only check of the caption ink on real windows (ignored test, needs PIN_IT_PNG_DIR for the PNGs).
//!
//! Per window it compares the ink from the PrintWindow capture with the ink read from the screen pixels of the
//! same minimize button, and writes a magnified caption strip and the pin drawn next to it.
//! Run: `PIN_IT_PNG_DIR=<dir> cargo test real_caption_ink -- --ignored --nocapture`.

use tiny_skia::Pixmap;
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CAPTUREBLT, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
    GetPixel, ReleaseDC, SRCCOPY, SelectObject,
};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GA_ROOT, GetAncestor, GetClassNameW, GetWindowRect, WindowFromPoint,
};

use super::capture::is_usable;
use super::ink::glyph_ink;
use super::plan::capture_plan;
use super::scan::sample_title_bar;
use crate::anim::ButtonAnim;
use crate::overlay::{ButtonStyle, render_button};
use crate::win32::to_hwnd;

const STRIP_WIDTH: i32 = 200;
const STRIP_HEIGHT: i32 = 40;
const ZOOM: u32 = 4;
const PIN_GAP: i32 = 4;
const MAX_WINDOWS: usize = 3;
/// Largest per-channel difference that still counts as the same ink.
const MATCH_TOLERANCE: u8 = 12;
const MARKER: [u8; 3] = [255, 0, 255];

type Rgb = [u8; 3];

fn hex(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

fn class_of(h: HWND) -> String {
    let mut buf = [0u16; 64];
    // SAFETY: buf is a valid out buffer.
    let n = unsafe { GetClassNameW(h, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// Screen pixels of one rectangle (row-major), read with a single BitBlt of that rectangle.
fn screen_pixels(left: i32, top: i32, width: i32, height: i32) -> Vec<Rgb> {
    // SAFETY: every GDI object is released before returning.
    unsafe {
        let screen = GetDC(None);
        let dc = CreateCompatibleDC(Some(screen));
        let bitmap = CreateCompatibleBitmap(screen, width, height);
        let previous = SelectObject(dc, bitmap.into());
        let _ = BitBlt(
            dc,
            0,
            0,
            width,
            height,
            Some(screen),
            left,
            top,
            SRCCOPY | CAPTUREBLT,
        );
        let pixels = (0..height)
            .flat_map(|y| (0..width).map(move |x| (x, y)))
            .map(|(x, y)| {
                let p = GetPixel(dc, x, y).0;
                [p as u8, (p >> 8) as u8, (p >> 16) as u8]
            })
            .collect();
        SelectObject(dc, previous);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(dc);
        ReleaseDC(None, screen);
        pixels
    }
}

/// True when the window itself is what the screen shows at every probe point of its caption.
fn caption_is_visible(h: HWND, probes: &[(i32, i32)]) -> bool {
    probes.iter().all(|&(x, y)| {
        // SAFETY: plain queries.
        let top = unsafe { GetAncestor(WindowFromPoint(POINT { x, y }), GA_ROOT) };
        top == h
    })
}

struct Validated {
    class: String,
    bar: Rgb,
    printed: Option<Rgb>,
    screen: Option<Rgb>,
    strip: Vec<Rgb>,
    marker: RECT,
    caption_px: i32,
}

fn validate(h: HWND) -> Option<Validated> {
    let plan = capture_plan(h)?;
    let area = plan.ink_area?;
    let mut window = RECT::default();
    // SAFETY: window is a valid out param.
    unsafe { GetWindowRect(h, &mut window) }.ok()?;
    let probes = [
        ((area.left + area.right) / 2, (area.top + area.bottom) / 2),
        (area.left + 4, area.top + 4),
        (area.right + 4, area.top + 4),
    ]
    .map(|(x, y)| (window.left + x, window.top + y));
    if !caption_is_visible(h, &probes) {
        return None;
    }
    let sample = sample_title_bar(h.0 as isize)?;
    let scan = sample.scan?;
    let button = screen_pixels(
        window.left + area.left,
        window.top + area.top,
        area.right - area.left,
        area.bottom - area.top,
    );
    let strip_left = (window.right - STRIP_WIDTH).max(window.left);
    let strip_top = window.top + area.top;
    Some(Validated {
        class: class_of(h),
        bar: scan.background,
        printed: scan.caption_ink,
        screen: glyph_ink(&button),
        strip: screen_pixels(strip_left, strip_top, STRIP_WIDTH, STRIP_HEIGHT),
        marker: RECT {
            left: window.left + area.left - strip_left,
            top: 0,
            right: window.left + area.right - strip_left,
            bottom: area.bottom - area.top,
        },
        caption_px: plan.caption_px,
    })
}

fn same(a: Option<Rgb>, b: Option<Rgb>) -> bool {
    matches!((a, b), (Some(a), Some(b)) if a.iter().zip(b).all(|(&x, y)| x.abs_diff(y) <= MATCH_TOLERANCE))
}

/// A rendered pin (premultiplied BGRA, `size` square) at a position of the strip.
struct PinOverlay<'a> {
    bgra: &'a [u8],
    size: u32,
    at: (i32, i32),
}

/// A magnified copy of the strip, with the pin drawn into it and the marker outline on top when given.
fn magnified(strip: &[Rgb], marker: Option<RECT>, pin: Option<PinOverlay>) -> Pixmap {
    let mut canvas = strip.to_vec();
    if let Some(pin) = pin {
        for (i, p) in pin.bgra.chunks_exact(4).enumerate() {
            let x = pin.at.0 + (i as u32 % pin.size) as i32;
            let y = pin.at.1 + (i as u32 / pin.size) as i32;
            if (0..STRIP_WIDTH).contains(&x) && (0..STRIP_HEIGHT).contains(&y) {
                let under = &mut canvas[(y * STRIP_WIDTH + x) as usize];
                let src = [p[2], p[1], p[0]];
                *under = std::array::from_fn(|c| {
                    (u32::from(src[c]) + u32::from(under[c]) * (255 - u32::from(p[3])) / 255) as u8
                });
            }
        }
    }
    let mut pm = Pixmap::new(STRIP_WIDTH as u32 * ZOOM, STRIP_HEIGHT as u32 * ZOOM).unwrap();
    let width = pm.width();
    for (i, out) in pm.data_mut().chunks_exact_mut(4).enumerate() {
        let (x, y) = ((i as u32 % width) / ZOOM, (i as u32 / width) / ZOOM);
        let mut c = canvas[(y * STRIP_WIDTH as u32 + x) as usize];
        if marker.is_some_and(|m| on_outline(&m, x as i32, y as i32)) {
            c = MARKER;
        }
        out.copy_from_slice(&[c[0], c[1], c[2], 255]);
    }
    pm
}

fn on_outline(r: &RECT, x: i32, y: i32) -> bool {
    let inside = x >= r.left && x < r.right && y >= r.top && y < r.bottom;
    inside && (x == r.left || x == r.right - 1 || y == r.top || y == r.bottom - 1)
}

fn save(dir: &str, index: usize, v: &Validated) {
    magnified(&v.strip, Some(v.marker), None)
        .save_png(format!("{dir}/window{index}_strip.png"))
        .unwrap();
    let size = (v.caption_px * 3 / 4).clamp(16, STRIP_HEIGHT);
    let at = (v.marker.left - size - PIN_GAP, (v.caption_px - size) / 2);
    let style = ButtonStyle {
        title_bar: v.bar,
        ink: v.printed,
        ..Default::default()
    };
    let mut hover = ButtonAnim::new(false);
    hover.on_hover(true);
    hover.step(1.0);
    let [rest, hovered] = [ButtonAnim::new(false), hover].map(|anim| {
        let bgra = render_button(size as u32, &style, &anim);
        let pin = PinOverlay {
            bgra: &bgra,
            size: size as u32,
            at,
        };
        magnified(&v.strip, None, Some(pin))
    });
    let mut both = Pixmap::new(rest.width(), rest.height() * 2).unwrap();
    let half = rest.data().len();
    both.data_mut()[..half].copy_from_slice(rest.data());
    both.data_mut()[half..].copy_from_slice(hovered.data());
    both.save_png(format!("{dir}/window{index}_pin_rest_above_hover.png"))
        .unwrap();
}

#[test]
#[ignore]
fn real_caption_ink_matches_the_screen() {
    let _guard = crate::window_test_guard();
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let dir = std::env::var("PIN_IT_PNG_DIR").expect("PIN_IT_PNG_DIR");
    std::fs::create_dir_all(&dir).unwrap();
    let mut seen_exes = Vec::new();
    let mut done = 0;
    println!("exe | class | bar | printed ink | screen ink | match");
    for handle in crate::windows_filter::enumerate_pinnable() {
        let h = to_hwnd(handle);
        let exe = crate::windows_filter::exe_name(handle).unwrap_or_default();
        if done == MAX_WINDOWS || seen_exes.contains(&exe) || !is_usable(h) {
            continue;
        }
        let Some(v) = validate(h) else {
            println!("{exe}: skipped (caption covered or no capture)");
            continue;
        };
        seen_exes.push(exe.clone());
        let fmt = |c: Option<Rgb>| c.map_or("none".into(), hex);
        println!(
            "{exe} | {} | {} | {} | {} | {}",
            v.class,
            hex(v.bar),
            fmt(v.printed),
            fmt(v.screen),
            if same(v.printed, v.screen) {
                "yes"
            } else {
                "no"
            }
        );
        save(&dir, done, &v);
        done += 1;
    }
    assert!(done > 0, "no uncovered window could be validated");
}
