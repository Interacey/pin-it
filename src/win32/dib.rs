//! Memory DC with a top-down 32-bit DIB section: the pixel source of the pin overlay
//! (`UpdateLayeredWindow`) and of the dial window (`BitBlt`, with GDI text drawn on top).

use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS,
    DeleteDC, DeleteObject, GdiFlush, HBITMAP, HDC, HGDIOBJ, SelectObject,
};

const BYTES_PER_PIXEL: u32 = 4;

/// Memory DC with a top-down 32-bit DIB section of one fixed size.
pub(crate) struct Dib {
    pub(crate) dc: HDC,
    bmp: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl Dib {
    pub(crate) fn new(width: u32, height: u32) -> windows::core::Result<Self> {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32),
                biPlanes: 1,
                biBitCount: (BYTES_PER_PIXEL * 8) as u16,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        // SAFETY: plain GDI object creation; `bits` receives the section memory.
        unsafe {
            let dc = CreateCompatibleDC(None);
            if dc.is_invalid() {
                return Err(windows::core::Error::from_thread());
            }
            match CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0) {
                Ok(bmp) => {
                    let old = SelectObject(dc, bmp.into());
                    Ok(Self {
                        dc,
                        bmp,
                        old,
                        bits: bits.cast(),
                        width,
                        height,
                    })
                }
                Err(e) => {
                    // The creation error is the one worth returning; a failed delete adds nothing.
                    let _ = DeleteDC(dc);
                    Err(e)
                }
            }
        }
    }

    fn len(&self) -> usize {
        (self.width * self.height * BYTES_PER_PIXEL) as usize
    }

    /// Replaces the pixels with `bgra` (premultiplied, `width * height * 4` bytes).
    pub(crate) fn write(&mut self, bgra: &[u8]) {
        assert_eq!(bgra.len(), self.len());
        // SAFETY: the section holds exactly `len` bytes and lives as long as `self`; pending GDI
        // drawing is flushed first so it cannot land on the new pixels.
        unsafe {
            // A failed flush leaves at most a stale text fragment that the copy overwrites.
            let _ = GdiFlush();
            std::ptr::copy_nonoverlapping(bgra.as_ptr(), self.bits, self.len());
        }
    }

    /// The pixels as RGBA with alpha forced opaque (GDI text leaves alpha at 0), for tests.
    #[cfg(test)]
    pub(crate) fn read_rgba(&self) -> Vec<u8> {
        // SAFETY: see `write`.
        let mut data = unsafe {
            let _ = GdiFlush();
            std::slice::from_raw_parts(self.bits, self.len()).to_vec()
        };
        for px in data.chunks_exact_mut(4) {
            px.swap(0, 2);
            px[3] = u8::MAX;
        }
        data
    }
}

impl Drop for Dib {
    fn drop(&mut self) {
        // SAFETY: handles were created in `new` and are released once.
        // Delete failures cannot be acted on in a destructor.
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bmp.into());
            let _ = DeleteDC(self.dc);
        }
    }
}
