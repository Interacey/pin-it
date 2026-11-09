//! The system accent color.

use windows::Win32::Graphics::Dwm::DwmGetColorizationColor;
use windows::core::BOOL;

/// Windows blue, used when DWM reports no colorization color.
const FALLBACK_ACCENT: [u8; 3] = [0, 120, 215];

/// System accent color from DwmGetColorizationColor, or Windows blue (0,120,215) if the call fails.
pub(crate) fn system_accent() -> [u8; 3] {
    let (mut col, mut opaque) = (0u32, BOOL(0));
    // SAFETY: both out params are valid for the call.
    match unsafe { DwmGetColorizationColor(&mut col, &mut opaque) } {
        Ok(()) => [(col >> 16) as u8, (col >> 8) as u8, col as u8],
        Err(_) => FALLBACK_ACCENT,
    }
}
