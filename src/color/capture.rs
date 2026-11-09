//! GDI capture of a window into a small bitmap and readback of single pixels.

use windows::Win32::Foundation::{COLORREF, HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleBitmap, CreateCompatibleDC,
    CreateSolidBrush, DIB_RGB_COLORS, DeleteDC, DeleteObject, FillRect, GetDC, GetDIBits, HBITMAP,
    HBRUSH, HDC, HGDIOBJ, ReleaseDC, SelectObject,
};
use windows::Win32::Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow};
use windows::Win32::UI::WindowsAndMessaging::{IsHungAppWindow, IsIconic, IsWindowVisible};

use crate::win32::{colorref, is_cloaked};

const PW_RENDERFULLCONTENT: u32 = 2;
/// Fill color of the capture bitmap before PrintWindow runs; still there afterwards = failed capture.
pub(super) const SENTINEL: [u8; 3] = [1, 254, 3];
/// Bytes per pixel of the 32-bit readback.
const BYTES_PER_PIXEL: usize = 4;

/// A memory DC with a bitmap and the brush that filled it; every GDI object is released on drop.
struct CaptureSurface {
    screen: HDC,
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    sentinel_brush: HBRUSH,
    width: i32,
    height: i32,
}

impl CaptureSurface {
    /// A `width` x `height` surface filled with `SENTINEL`.
    fn new(width: i32, height: i32) -> Self {
        // SAFETY: the objects are owned by the returned value and released in `drop`.
        unsafe {
            let screen = GetDC(None);
            let dc = CreateCompatibleDC(Some(screen));
            let bitmap = CreateCompatibleBitmap(screen, width, height);
            let previous = SelectObject(dc, bitmap.into());
            let sentinel_brush = CreateSolidBrush(COLORREF(colorref(SENTINEL)));
            let full = RECT {
                left: 0,
                top: 0,
                right: width,
                bottom: height,
            };
            FillRect(dc, &full, sentinel_brush);
            Self {
                screen,
                dc,
                bitmap,
                previous,
                sentinel_brush,
                width,
                height,
            }
        }
    }

    /// Renders the window into the surface; false when PrintWindow fails.
    fn print_window(&self, h: HWND) -> bool {
        // SAFETY: self.dc is a valid DC for the lifetime of the surface.
        unsafe { PrintWindow(h, self.dc, PRINT_WINDOW_FLAGS(PW_RENDERFULLCONTENT)) }.as_bool()
    }

    /// The whole surface as top-down BGRA bytes (one GetDIBits call instead of one call per pixel);
    /// empty when the readback fails.
    fn read_all(&self) -> Vec<u8> {
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: self.width,
                biHeight: -self.height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bytes = vec![0u8; self.width as usize * self.height as usize * BYTES_PER_PIXEL];
        // SAFETY: the buffer holds `height` rows of `width` 32-bit pixels, as the header says.
        let lines = unsafe {
            GetDIBits(
                self.dc,
                self.bitmap,
                0,
                self.height as u32,
                Some(bytes.as_mut_ptr().cast()),
                &mut info,
                DIB_RGB_COLORS,
            )
        };
        if lines == 0 {
            bytes.clear();
        }
        bytes
    }
}

impl Drop for CaptureSurface {
    fn drop(&mut self) {
        // SAFETY: the objects were created in `new` and are released once, in reverse order.
        // A failed delete leaves nothing to recover, so the results are ignored.
        unsafe {
            SelectObject(self.dc, self.previous);
            let _ = DeleteObject(self.sentinel_brush.into());
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.dc);
            ReleaseDC(None, self.screen);
        }
    }
}

/// RGB of the pixel at `(x, y)` of a `width`-wide BGRA readback; `None` outside it or while still the sentinel fill.
fn readable_pixel(bytes: &[u8], width: i32, (x, y): (i32, i32)) -> Option<[u8; 3]> {
    if x < 0 || y < 0 || x >= width {
        return None;
    }
    let at = (y as usize * width as usize + x as usize) * BYTES_PER_PIXEL;
    let px = bytes.get(at..at + 3)?;
    Some([px[2], px[1], px[0]]).filter(|rgb| *rgb != SENTINEL)
}

/// Renders the window once into a `bmp_w` x `bmp_h` bitmap and reads `points` (window-relative).
/// PrintWindow clips to the bitmap, so a caption-rows-only bitmap is enough for title bar points.
/// Returns None if PrintWindow fails or every point still holds the sentinel fill. A point outside the bitmap
/// or still holding the sentinel is `None` in the result, so results line up with `points`.
pub(super) fn capture_pixels(
    h: HWND,
    bmp_w: i32,
    bmp_h: i32,
    points: &[(i32, i32)],
) -> Option<Vec<Option<[u8; 3]>>> {
    let surface = CaptureSurface::new(bmp_w, bmp_h);
    if !surface.print_window(h) {
        return None;
    }
    let bytes = surface.read_all();
    let rgb: Vec<Option<[u8; 3]>> = points
        .iter()
        .map(|&p| readable_pixel(&bytes, bmp_w, p))
        .collect();
    rgb.iter().any(Option::is_some).then_some(rgb)
}

/// The readable pixels of `capture_pixels`.
#[cfg(test)]
pub(super) fn capture_points(
    h: HWND,
    bmp_w: i32,
    bmp_h: i32,
    points: &[(i32, i32)],
) -> Option<Vec<[u8; 3]>> {
    let rgb: Vec<[u8; 3]> = capture_pixels(h, bmp_w, bmp_h, points)?
        .into_iter()
        .flatten()
        .collect();
    (!rgb.is_empty()).then_some(rgb)
}

/// Visible, restored, responsive and not cloaked: a window PrintWindow can render.
pub(super) fn is_usable(h: HWND) -> bool {
    // SAFETY: read-only queries on a window handle.
    let visible = unsafe {
        IsWindowVisible(h).as_bool() && !IsIconic(h).as_bool() && !IsHungAppWindow(h).as_bool()
    };
    visible && !is_cloaked(h)
}
