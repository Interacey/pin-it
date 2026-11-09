//! GDI text for the popup windows. Segoe UI renders more crisply through GDI ClearType than
//! anything tiny-skia can do without a font engine, so the pictures are drawn with tiny-skia
//! and the text is put on top with GDI at positions the layout gives.

use windows::Win32::Foundation::{COLORREF, RECT, SIZE};
use windows::Win32::Graphics::Gdi::{
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateFontW, DEFAULT_CHARSET, DEFAULT_PITCH,
    DRAW_TEXT_FORMAT, DeleteObject, DrawTextW, FF_DONTCARE, GetTextExtentPoint32W, HDC, HFONT,
    HGDIOBJ, OUT_TT_PRECIS, SelectObject, SetBkMode, SetTextAlign, SetTextColor, TA_BASELINE,
    TA_LEFT, TRANSPARENT, TextOutW,
};
use windows::core::w;

use crate::win32::colorref;

/// A Segoe UI font, released on drop.
pub(crate) struct Font(HFONT);

impl Font {
    /// A font of `height_dp` at `scale` pixels per dp and the given GDI weight.
    pub(crate) fn new(height_dp: f32, scale: f32, weight: i32) -> windows::core::Result<Font> {
        let height = -((height_dp * scale).round() as i32);
        // SAFETY: plain font creation; Windows substitutes a font when the face is missing.
        let font = unsafe {
            CreateFontW(
                height,
                0,
                0,
                0,
                weight,
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_TT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                CLEARTYPE_QUALITY,
                (DEFAULT_PITCH.0 | FF_DONTCARE.0).into(),
                w!("Segoe UI"),
            )
        };
        if font.is_invalid() {
            return Err(windows::core::Error::from_thread());
        }
        Ok(Font(font))
    }

    pub(crate) fn handle(&self) -> HFONT {
        self.0
    }
}

impl Drop for Font {
    fn drop(&mut self) {
        // SAFETY: the font was created by `new` and is not selected into a DC any more.
        // A failed delete only leaks the handle.
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.0.0));
        }
    }
}

pub(crate) fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// Text color for the following output on `dc`; the background stays as drawn.
pub(crate) fn set_color(dc: HDC, rgb: [u8; 3]) {
    // SAFETY: `dc` is a live memory DC.
    unsafe {
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, COLORREF(colorref(rgb)));
    }
}

pub(crate) fn text_width(dc: HDC, font: &Font, text: &[u16]) -> f32 {
    let mut size = SIZE::default();
    // SAFETY: `dc` is a live memory DC and `size` a valid out buffer.
    unsafe {
        SelectObject(dc, HGDIOBJ(font.0.0));
        // A failed measurement leaves the width 0, which only shifts the text.
        let _ = GetTextExtentPoint32W(dc, text, &mut size);
    }
    size.cx as f32
}

pub(crate) fn draw_at_baseline(dc: HDC, font: &Font, at: (f32, f32), text: &[u16]) {
    // SAFETY: `dc` is a live memory DC; `text` is a valid slice.
    unsafe {
        SetTextAlign(dc, TA_BASELINE | TA_LEFT);
        SelectObject(dc, HGDIOBJ(font.0.0));
        // A failed draw leaves the text empty for one frame; the next paint draws it again.
        let _ = TextOutW(dc, at.0.round() as i32, at.1.round() as i32, text);
    }
}

/// Draws `text` into `rect` as `format` says (alignment, single line, ellipsis).
pub(crate) fn draw_in_rect(dc: HDC, font: &Font, rect: RECT, text: &str, format: DRAW_TEXT_FORMAT) {
    let mut rect = rect;
    let mut wide = utf16(text);
    // SAFETY: `dc` is a live memory DC; `wide` and `rect` outlive the call.
    unsafe {
        SelectObject(dc, HGDIOBJ(font.0.0));
        DrawTextW(dc, &mut wide, &mut rect, format);
    }
}
