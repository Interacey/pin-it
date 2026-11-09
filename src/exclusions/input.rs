//! The one-line input: a real EDIT control, so clipboard, selection, undo and IME work as in any
//! other program. It has no border of its own (the window paints the field around it) and takes
//! its colors from `WM_CTLCOLOREDIT`. A window subclass hands Enter, Esc, Up, Down and Delete to
//! the window.

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateSolidBrush, DeleteObject, HBRUSH, HDC, HGDIOBJ, SetBkColor, SetTextColor,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{EM_SETCUEBANNER, EM_SETLIMITTEXT, EM_SETMARGINS};
#[cfg(not(test))]
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, EC_LEFTMARGIN, EC_RIGHTMARGIN, ES_AUTOHSCROLL, ES_LEFT, GetWindowTextLengthW,
    GetWindowTextW, SendMessageW, SetWindowTextW, WINDOW_STYLE, WM_CHAR, WM_KEYDOWN, WM_NCDESTROY,
    WM_SETFONT, WS_CHILD, WS_VISIBLE,
};
use windows::core::{PCWSTR, w};

use super::layout::Layout;
use super::model::MAX_NAME_LEN;
use super::text::CUE_TEXT;
use super::window::key_pressed;
use crate::popup::text::Font;
use crate::win32::{colorref, wide};

const SUBCLASS_ID: usize = 1;
/// The characters Enter and Esc produce; the window handles the keys, so the control must not
/// beep for them.
const CHAR_ENTER: usize = 0x0D;
const CHAR_ESCAPE: usize = 0x1B;

/// Creates the EDIT control inside `parent`, placed by `layout`, with the font, the length limit
/// of a file name, the cue banner and the key subclass.
pub(super) fn create(parent: HWND, layout: &Layout, font: &Font) -> windows::core::Result<HWND> {
    let area = layout.edit();
    // SAFETY: module handle query and window creation with a system class.
    let edit = unsafe {
        let hinst = GetModuleHandleW(None)?;
        CreateWindowExW(
            Default::default(),
            w!("EDIT"),
            PCWSTR::null(),
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE((ES_LEFT | ES_AUTOHSCROLL) as u32),
            area.x.round() as i32,
            area.y.round() as i32,
            area.w.round() as i32,
            area.h.round() as i32,
            Some(parent),
            None,
            Some(hinst.into()),
            None,
        )?
    };
    let cue = wide(CUE_TEXT);
    let margins = (EC_LEFTMARGIN | EC_RIGHTMARGIN) as usize;
    // SAFETY: our own control; `cue` outlives the call and the control copies it.
    unsafe {
        SendMessageW(
            edit,
            WM_SETFONT,
            Some(WPARAM(font.handle().0 as usize)),
            Some(LPARAM(1)),
        );
        SendMessageW(edit, EM_SETLIMITTEXT, Some(WPARAM(MAX_NAME_LEN)), None);
        SendMessageW(edit, EM_SETMARGINS, Some(WPARAM(margins)), Some(LPARAM(0)));
        SendMessageW(
            edit,
            EM_SETCUEBANNER,
            Some(WPARAM(1)),
            Some(LPARAM(cue.as_ptr() as isize)),
        );
    }
    // SAFETY: the procedure is a static function; the subclass is removed when the control dies.
    if !unsafe { SetWindowSubclass(edit, Some(subclass_proc), SUBCLASS_ID, 0) }.as_bool() {
        return Err(windows::core::Error::from_thread());
    }
    Ok(edit)
}

/// The text in the control.
pub(super) fn text(edit: HWND) -> String {
    // SAFETY: our own control; the buffer has room for the length and the terminating NUL.
    unsafe {
        let len = GetWindowTextLengthW(edit).max(0) as usize;
        let mut buf = vec![0u16; len + 1];
        let copied = GetWindowTextW(edit, &mut buf).max(0) as usize;
        String::from_utf16_lossy(&buf[..copied])
    }
}

pub(super) fn is_empty(edit: HWND) -> bool {
    // SAFETY: our own control.
    unsafe { GetWindowTextLengthW(edit) == 0 }
}

/// Replaces the text; an empty string clears the field.
pub(super) fn set_text(edit: HWND, text: &str) -> windows::core::Result<()> {
    let text = wide(text);
    // SAFETY: our own control; `text` is NUL-terminated and outlives the call.
    unsafe { SetWindowTextW(edit, PCWSTR(text.as_ptr())) }
}

/// Gives the control the keyboard focus. Fails while the window is not the active one, which
/// is the normal case when the system refused the focus at opening.
#[cfg(not(test))]
pub(super) fn focus(edit: HWND) {
    // SAFETY: our own control. The previous focus window is of no interest.
    let _ = unsafe { SetFocus(Some(edit)) };
}

#[cfg(test)]
thread_local! {
    /// How often the window asked for the focus.
    pub(super) static FOCUS_REQUESTS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Tests only count the request. Taking the focus would activate the test window in this
/// thread's queue, and after that the system treats the windows of the other tests differently
/// (their `HWND_TOPMOST` is ignored, measured with the pin tests).
#[cfg(test)]
pub(super) fn focus(_edit: HWND) {
    FOCUS_REQUESTS.with(|n| n.set(n.get() + 1));
}

/// The background brush of the control; deleted on drop.
pub(super) struct Brush(HBRUSH);

impl Brush {
    pub(super) fn new(rgb: [u8; 3]) -> windows::core::Result<Self> {
        // SAFETY: plain GDI object creation.
        let brush = unsafe { CreateSolidBrush(COLORREF(colorref(rgb))) };
        if brush.is_invalid() {
            return Err(windows::core::Error::from_thread());
        }
        Ok(Self(brush))
    }

    /// Answer to `WM_CTLCOLOREDIT`: sets the text and background color on `dc` and returns the
    /// brush for the rest of the control's background.
    pub(super) fn paint_edit(&self, dc: HDC, text: [u8; 3], background: [u8; 3]) -> LRESULT {
        // SAFETY: `dc` is the control's device context handed to us by the message.
        unsafe {
            SetTextColor(dc, COLORREF(colorref(text)));
            SetBkColor(dc, COLORREF(colorref(background)));
        }
        LRESULT(self.0.0 as isize)
    }
}

impl Drop for Brush {
    fn drop(&mut self) {
        // SAFETY: the brush was created in `new`; the control that used it is gone by now.
        // A failed delete only leaks the handle.
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.0.0));
        }
    }
}

unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    match msg {
        WM_KEYDOWN if key_pressed(wparam.0 as u16) => LRESULT(0),
        WM_CHAR if matches!(wparam.0, CHAR_ENTER | CHAR_ESCAPE) => LRESULT(0),
        WM_NCDESTROY => {
            // SAFETY: removes the subclass installed by `create`, then default handling.
            unsafe {
                let _ = RemoveWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID);
                DefSubclassProc(hwnd, msg, wparam, lparam)
            }
        }
        // SAFETY: default handling.
        _ => unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) },
    }
}
