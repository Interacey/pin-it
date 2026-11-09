//! Which top-level windows get a pin button.

use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GA_ROOT, GWL_EXSTYLE, GWL_STYLE, GetAncestor, GetWindowLongW, GetWindowRect,
    IsIconic, IsWindowVisible, WS_CAPTION, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};
use windows::core::BOOL;

use crate::Hwnd;
use crate::win32::{class_name, is_cloaked, process_id, to_hwnd};

/// Window classes of the shell and of our own overlay; never pinnable.
const SHELL_CLASSES: [&str; 5] = [
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "PinItOverlay",
];

/// Whether `class` belongs to the desktop, the taskbar or Pin It's own overlay.
pub(crate) fn is_shell_class(class: &str) -> bool {
    SHELL_CLASSES.contains(&class)
}

/// True when the window should get a pin button. Called for every window on each sweep, so the checks run cheapest first.
pub(crate) fn is_pinnable(hwnd: Hwnd) -> bool {
    let w = to_hwnd(hwnd);
    if !is_visible_root(w) || !has_pinnable_styles(w) {
        return false;
    }
    if is_shell_class(&class_name(w)) {
        return false;
    }
    if !has_area(w) || is_own_process(w) {
        return false;
    }
    // The DWM query comes last: it is the most expensive check.
    !is_cloaked(w)
}

/// All currently pinnable top-level windows.
pub(crate) fn enumerate_pinnable() -> Vec<Hwnd> {
    unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: lparam is the &mut Vec<Hwnd> passed below for the duration of EnumWindows.
        let list = unsafe { &mut *(lparam.0 as *mut Vec<Hwnd>) };
        if is_pinnable(hwnd.0 as isize) {
            list.push(hwnd.0 as isize);
        }
        true.into()
    }
    let mut list: Vec<Hwnd> = Vec::new();
    // SAFETY: the callback only uses lparam as the Vec above.
    // A failed walk keeps the windows found so far.
    let _ = unsafe { EnumWindows(Some(collect), LPARAM(&mut list as *mut _ as isize)) };
    list
}

fn is_visible_root(w: HWND) -> bool {
    // SAFETY: plain queries on a handle that may be stale; they fail harmlessly.
    unsafe {
        IsWindowVisible(w).as_bool() && !IsIconic(w).as_bool() && GetAncestor(w, GA_ROOT) == w
    }
}

/// A title bar (WS_CAPTION) and neither tool window nor no-activate.
fn has_pinnable_styles(w: HWND) -> bool {
    // SAFETY: plain queries on a handle that may be stale; they fail harmlessly.
    let (style, ex) = unsafe {
        (
            GetWindowLongW(w, GWL_STYLE) as u32,
            GetWindowLongW(w, GWL_EXSTYLE) as u32,
        )
    };
    style & WS_CAPTION.0 == WS_CAPTION.0 && ex & (WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0) == 0
}

fn has_area(w: HWND) -> bool {
    let mut r = RECT::default();
    // SAFETY: r is a valid out param.
    let queried = unsafe { GetWindowRect(w, &mut r) }.is_ok();
    queried && r.right > r.left && r.bottom > r.top
}

fn is_own_process(w: HWND) -> bool {
    // SAFETY: no arguments.
    process_id(w) == unsafe { GetCurrentProcessId() }
}
