//! Pin It: pins windows on top of the others from a button in the title bar.

pub(crate) mod anim;
pub mod app;
pub(crate) mod autostart;
pub(crate) mod color;
pub(crate) mod config;
pub(crate) mod dial;
pub(crate) mod events;
pub(crate) mod exclusions;
pub(crate) mod hotkey;
pub(crate) mod overlay;
pub(crate) mod pin_stack;
pub(crate) mod placement;
pub(crate) mod popup;
pub(crate) mod tray;
pub(crate) mod win32;
pub(crate) mod windows_filter;
pub(crate) mod zorder;

/// Window handle as a plain integer (`HWND.0 as isize`).
pub(crate) type Hwnd = isize;

/// Serializes tests that create windows, hooks, tray icons or hotkeys, which are process-wide resources.
#[cfg(test)]
static WINDOW_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Takes the window test lock, ignoring poisoning from a failed earlier test.
#[cfg(test)]
pub(crate) fn window_test_guard() -> std::sync::MutexGuard<'static, ()> {
    WINDOW_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Class and title of the foreground window, for failure messages of desktop-dependent tests.
#[cfg(test)]
pub(crate) fn describe_foreground() -> String {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetForegroundWindow, GetWindowTextW,
    };
    let fg = unsafe { GetForegroundWindow() };
    let (mut c, mut t) = ([0u16; 128], [0u16; 128]);
    let (cn, tn) = unsafe { (GetClassNameW(fg, &mut c), GetWindowTextW(fg, &mut t)) };
    format!(
        "foreground {:#x} class={} title={}",
        fg.0 as isize,
        String::from_utf16_lossy(&c[..cn.max(0) as usize]),
        String::from_utf16_lossy(&t[..tn.max(0) as usize])
    )
}
