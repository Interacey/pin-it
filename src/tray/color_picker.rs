//! The Windows color dialog behind "Fixed accent color...".

use super::open_guard::{OpenGuard, PICKER_OPEN};
use crate::win32::colorref;
use std::sync::Mutex;
use windows::Win32::Foundation::{COLORREF, HWND};
use windows::Win32::UI::Controls::Dialogs::{CC_FULLOPEN, CC_RGBINIT, CHOOSECOLORW, ChooseColorW};

const CUSTOM_COLOR_SLOTS: usize = 16;
const WHITE: COLORREF = COLORREF(0x00FF_FFFF);

/// Custom colors of the dialog, kept so they survive between openings.
static CUSTOM_COLORS: Mutex<[COLORREF; CUSTOM_COLOR_SLOTS]> =
    Mutex::new([WHITE; CUSTOM_COLOR_SLOTS]);

/// Shows the dialog starting at `initial`; `None` if cancelled or another picker is already open.
pub(super) fn pick_color(owner: HWND, initial: [u8; 3]) -> Option<[u8; 3]> {
    let _open = OpenGuard::acquire(&PICKER_OPEN)?;
    // Work on a copy: the lock must not be held across the modal dialog.
    let mut custom = *CUSTOM_COLORS.lock().ok()?;
    let mut dialog = CHOOSECOLORW {
        lStructSize: std::mem::size_of::<CHOOSECOLORW>() as u32,
        hwndOwner: owner,
        rgbResult: COLORREF(colorref(initial)),
        lpCustColors: custom.as_mut_ptr(),
        Flags: CC_FULLOPEN | CC_RGBINIT,
        ..Default::default()
    };
    // SAFETY: dialog and the 16-entry custom color array stay alive for the modal call.
    let confirmed = unsafe { ChooseColorW(&mut dialog) }.as_bool();
    if let Ok(mut shared) = CUSTOM_COLORS.lock() {
        *shared = custom;
    }
    confirmed.then(|| rgb_from_colorref(dialog.rgbResult))
}

fn rgb_from_colorref(color: COLORREF) -> [u8; 3] {
    let c = color.0;
    [c as u8, (c >> 8) as u8, (c >> 16) as u8]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colorref_round_trips_rgb() {
        let rgb = [1, 2, 3];
        assert_eq!(rgb_from_colorref(COLORREF(colorref(rgb))), rgb);
    }
}
