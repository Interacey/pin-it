//! Small window queries shared by the app modules.

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::WindowsAndMessaging::{
    GA_ROOT, GWL_EXSTYLE, GetAncestor, GetWindowLongW, IsIconic, IsWindow, IsWindowVisible,
    WS_EX_TOOLWINDOW,
};

use crate::Hwnd;
use crate::hotkey;
use crate::win32::{class_name, is_cloaked, process_id, to_hwnd};
use crate::windows_filter::is_shell_class;

pub(super) fn window_alive(h: Hwnd) -> bool {
    // SAFETY: plain query on a possibly stale handle.
    unsafe { IsWindow(Some(to_hwnd(h))) }.as_bool()
}

pub(super) fn is_minimized(h: Hwnd) -> bool {
    // SAFETY: plain query on a possibly stale handle.
    unsafe { IsIconic(to_hwnd(h)) }.as_bool()
}

pub(super) fn is_visible(h: Hwnd) -> bool {
    // SAFETY: plain query on a possibly stale handle.
    unsafe { IsWindowVisible(to_hwnd(h)) }.as_bool()
}

#[cfg(test)]
pub(crate) static FOREGROUND_OVERRIDE: std::sync::atomic::AtomicIsize =
    std::sync::atomic::AtomicIsize::new(0);

/// The foreground root window; tests can replace it so they need not take the focus.
pub(super) fn current_foreground() -> Option<Hwnd> {
    #[cfg(test)]
    {
        let overridden = FOREGROUND_OVERRIDE.load(std::sync::atomic::Ordering::Relaxed);
        if overridden != 0 {
            return Some(overridden);
        }
    }
    hotkey::foreground_window()
}

pub(super) fn root_window(h: Hwnd) -> Hwnd {
    // SAFETY: plain query.
    let root = unsafe { GetAncestor(to_hwnd(h), GA_ROOT) };
    if root.0.is_null() { h } else { root.0 as Hwnd }
}

/// Whether a window outside of `RunOptions::only` may be pinned with the hotkey even though it
/// gets no button (custom title bar, no caption): a visible, restored, top-level window of another
/// process that is not cloaked, not a tool window and not part of the shell.
pub(super) fn hotkey_pinnable(h: Hwnd) -> bool {
    let w = to_hwnd(h);
    is_visible_top_level(w)
        && !is_tool_window(w)
        && belongs_to_other_process(w)
        && !is_cloaked(w)
        && !is_shell_window(w)
}

fn is_visible_top_level(w: HWND) -> bool {
    // SAFETY: plain queries on a possibly stale handle.
    unsafe {
        IsWindowVisible(w).as_bool() && !IsIconic(w).as_bool() && GetAncestor(w, GA_ROOT) == w
    }
}

fn is_tool_window(w: HWND) -> bool {
    // SAFETY: plain query on a possibly stale handle.
    let ex_style = unsafe { GetWindowLongW(w, GWL_EXSTYLE) } as u32;
    ex_style & WS_EX_TOOLWINDOW.0 != 0
}

fn belongs_to_other_process(w: HWND) -> bool {
    let pid = process_id(w);
    // SAFETY: plain query.
    pid != 0 && pid != unsafe { GetCurrentProcessId() }
}

fn is_shell_window(w: HWND) -> bool {
    is_shell_class(&class_name(w))
}
