//! Messages that say the system colors or theme changed, and the read of their one string argument.

use windows::Win32::Foundation::LPARAM;
use windows::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows::Win32::System::Threading::GetCurrentProcess;
use windows::Win32::UI::WindowsAndMessaging::WM_SETTINGCHANGE;

/// `WM_DWMCOLORIZATIONCOLORCHANGED`: the accent color changed.
pub(super) const WM_DWM_COLORIZATION_CHANGED: u32 = 0x0320;
/// `WM_THEMECHANGED`.
pub(super) const WM_THEME_CHANGED: u32 = 0x031A;

/// The `WM_SETTINGCHANGE` string that announces a light/dark or accent change.
const IMMERSIVE_COLOR_SET: &str = "ImmersiveColorSet";

/// Whether the message says the system colors changed. `lparam` of a `WM_SETTINGCHANGE` points to a
/// string that is valid only while the message is being sent, so it is read here and never kept.
pub(super) fn is_color_change(id: u32, lparam: LPARAM) -> bool {
    match id {
        WM_DWM_COLORIZATION_CHANGED | WM_THEME_CHANGED => true,
        WM_SETTINGCHANGE => names_immersive_color_set(&read_wide_prefix(lparam.0 as *const u16)),
        _ => false,
    }
}

/// True when `text` is exactly "ImmersiveColorSet" (an optional terminating nul is ignored).
fn names_immersive_color_set(text: &[u16]) -> bool {
    let end = text.iter().position(|&c| c == 0).unwrap_or(text.len());
    text[..end]
        .iter()
        .copied()
        .eq(IMMERSIVE_COLOR_SET.encode_utf16())
}

/// Reads at most one more unit than the expected string is long. Any process may send this message
/// with any pointer, so the read goes through `ReadProcessMemory` on this process: a bad address
/// gives an empty or short result instead of a fault.
fn read_wide_prefix(ptr: *const u16) -> Vec<u16> {
    const UNITS: usize = IMMERSIVE_COLOR_SET.len() + 2;
    let mut buffer = [0u16; UNITS];
    if ptr.is_null() {
        return Vec::new();
    }
    let mut bytes_read = 0usize;
    // SAFETY: the buffer is ours and big enough; the source address is only read by the system call.
    // A partial copy still fills `bytes_read` bytes, which is all that is used.
    let _ = unsafe {
        ReadProcessMemory(
            GetCurrentProcess(),
            ptr.cast(),
            buffer.as_mut_ptr().cast(),
            std::mem::size_of_val(&buffer),
            Some(&mut bytes_read),
        )
    };
    buffer[..bytes_read / 2].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain([0]).collect()
    }

    #[test]
    fn only_the_exact_name_matches() {
        assert!(names_immersive_color_set(&wide("ImmersiveColorSet")));
        assert!(!names_immersive_color_set(&wide("ImmersiveColorSets")));
        assert!(!names_immersive_color_set(&wide("Environment")));
        assert!(!names_immersive_color_set(&wide("")));
        assert!(!names_immersive_color_set(&[]));
    }

    #[test]
    fn settingchange_reads_the_string_behind_lparam() {
        let text = wide("ImmersiveColorSet");
        assert!(is_color_change(
            WM_SETTINGCHANGE,
            LPARAM(text.as_ptr() as isize)
        ));
        let other = wide("Policy");
        assert!(!is_color_change(
            WM_SETTINGCHANGE,
            LPARAM(other.as_ptr() as isize)
        ));
    }

    #[test]
    fn a_bad_or_null_pointer_is_not_a_color_change_and_does_not_fault() {
        assert!(!is_color_change(WM_SETTINGCHANGE, LPARAM(0)));
        assert!(!is_color_change(WM_SETTINGCHANGE, LPARAM(0x10)));
        assert!(!is_color_change(WM_SETTINGCHANGE, LPARAM(-1)));
    }

    #[test]
    fn dwm_and_theme_messages_always_count() {
        assert!(is_color_change(WM_DWM_COLORIZATION_CHANGED, LPARAM(0)));
        assert!(is_color_change(WM_THEME_CHANGED, LPARAM(0)));
        assert!(!is_color_change(0x0001, LPARAM(0)));
    }
}
