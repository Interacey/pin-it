//! Global hotkey description and registration.

use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, RegisterHotKey,
    UnregisterHotKey,
};
use windows::Win32::UI::WindowsAndMessaging::{GA_ROOT, GetAncestor, GetForegroundWindow};

use crate::Hwnd;
use crate::win32::to_hwnd;

/// A global hotkey: modifier flags plus a Win32 virtual-key code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Hotkey {
    pub(crate) ctrl: bool,
    pub(crate) alt: bool,
    pub(crate) shift: bool,
    pub(crate) win: bool,
    /// Win32 virtual-key code, 1..=0xFE for a valid hotkey.
    pub(crate) vk: u32,
}

const VK_F1: u32 = 0x70;
const VK_F24: u32 = 0x87;
const MAX_VIRTUAL_KEY: u32 = 0xFE;

impl Hotkey {
    /// Returns text like "Ctrl+Alt+P"; keys without a known name print as "VK 0x..".
    pub(crate) fn label(&self) -> String {
        let modifiers = [
            (self.ctrl, "Ctrl"),
            (self.alt, "Alt"),
            (self.shift, "Shift"),
            (self.win, "Win"),
        ];
        let mut parts: Vec<String> = modifiers
            .into_iter()
            .filter(|(on, _)| *on)
            .map(|(_, name)| name.to_string())
            .collect();
        parts.push(key_label(self.vk));
        parts.join("+")
    }

    /// Returns the RegisterHotKey modifier mask, always including MOD_NOREPEAT.
    fn modifiers(&self) -> HOT_KEY_MODIFIERS {
        let mut m = MOD_NOREPEAT;
        for (on, flag) in [
            (self.ctrl, MOD_CONTROL),
            (self.alt, MOD_ALT),
            (self.shift, MOD_SHIFT),
            (self.win, MOD_WIN),
        ] {
            if on {
                m |= flag;
            }
        }
        m
    }

    /// A key within range and a modifier that does not swallow typing: Shift alone is only
    /// accepted for function keys, otherwise it would take a capital letter system-wide.
    pub(crate) fn is_valid(&self) -> bool {
        let key_in_range = self.vk != 0 && self.vk <= MAX_VIRTUAL_KEY;
        let is_function_key = (VK_F1..=VK_F24).contains(&self.vk);
        let has_shortcut_modifier = self.ctrl || self.alt || self.win;
        key_in_range && (has_shortcut_modifier || (self.shift && is_function_key))
    }
}

fn key_label(vk: u32) -> String {
    match vk {
        0x30..=0x39 | 0x41..=0x5A => char::from(vk as u8).to_string(),
        VK_F1..=VK_F24 => format!("F{}", vk - VK_F1 + 1),
        vk => format!("VK 0x{vk:X}"),
    }
}

/// Registers the hotkey for `hwnd`; returns the Win32 error if the combination is taken.
pub(crate) fn register(hwnd: Hwnd, id: i32, hk: &Hotkey) -> windows::core::Result<()> {
    // SAFETY: plain call with a window handle; the system rejects a handle that is not ours.
    unsafe { RegisterHotKey(Some(to_hwnd(hwnd)), id, hk.modifiers(), hk.vk) }
}

/// Removes a hotkey registered with [`register`]; errors (not registered) are ignored.
pub(crate) fn unregister(hwnd: Hwnd, id: i32) {
    // SAFETY: plain call with a window handle; failure just means nothing was registered.
    let _ = unsafe { UnregisterHotKey(Some(to_hwnd(hwnd)), id) };
}

/// Returns the root window of the current foreground window, if any.
pub(crate) fn foreground_window() -> Option<Hwnd> {
    // SAFETY: plain query without pointers.
    let fg = unsafe { GetForegroundWindow() };
    if fg.0.is_null() {
        return None;
    }
    // SAFETY: fg is a window handle just returned by the system.
    let root = unsafe { GetAncestor(fg, GA_ROOT) };
    let h = if root.0.is_null() { fg } else { root };
    Some(h.0 as isize)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hk(ctrl: bool, alt: bool, shift: bool, win: bool, vk: u32) -> Hotkey {
        Hotkey {
            ctrl,
            alt,
            shift,
            win,
            vk,
        }
    }

    fn ctrl_alt_p() -> Hotkey {
        hk(true, true, false, false, 0x50)
    }

    #[test]
    fn preset_label() {
        assert_eq!(ctrl_alt_p().label(), "Ctrl+Alt+P");
    }

    #[test]
    fn labels() {
        assert_eq!(hk(false, false, true, true, 0x74).label(), "Shift+Win+F5");
        assert_eq!(hk(true, false, false, false, 0x35).label(), "Ctrl+5");
        assert_eq!(hk(true, false, false, false, 0xBA).label(), "Ctrl+VK 0xBA");
    }

    #[test]
    fn validity() {
        assert!(ctrl_alt_p().is_valid());
        assert!(!hk(false, false, false, false, 0x50).is_valid());
        assert!(!hk(true, false, false, false, 0).is_valid());
    }

    #[test]
    fn shift_alone_is_valid_only_for_function_keys() {
        assert!(!hk(false, false, true, false, 0x50).is_valid());
        assert!(!hk(false, false, true, false, 0x30).is_valid());
        assert!(hk(false, false, true, false, VK_F1).is_valid());
        assert!(hk(false, false, true, false, VK_F24).is_valid());
        assert!(!hk(false, false, true, false, VK_F24 + 1).is_valid());
        assert!(hk(false, false, true, true, 0x50).is_valid());
        assert!(hk(true, false, true, false, 0x50).is_valid());
    }

    #[test]
    fn modifiers_include_norepeat() {
        let m = ctrl_alt_p().modifiers();
        assert_eq!(m, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT);
        assert_eq!(
            hk(false, false, true, true, 1).modifiers(),
            MOD_SHIFT | MOD_WIN | MOD_NOREPEAT
        );
    }
}

/// Win32 registration checks on private windows; run with `cargo test -- --ignored --nocapture`.
#[cfg(test)]
mod real_windows {
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_POPUP,
    };
    use windows::core::w;

    #[test]
    #[ignore]
    fn register_unregister_and_conflict() {
        let _guard = crate::window_test_guard();
        let make = || {
            unsafe {
                CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    w!("STATIC"),
                    w!("pin-it hk"),
                    WS_POPUP,
                    0,
                    0,
                    1,
                    1,
                    None,
                    None,
                    None,
                    None,
                )
            }
            .unwrap()
        };
        let (w1, w2) = (make(), make());
        let (h1, h2) = (w1.0 as isize, w2.0 as isize);
        let hk = Hotkey {
            ctrl: true,
            alt: true,
            shift: true,
            win: false,
            vk: 0x86,
        }; // F23
        let first = register(h1, 1, &hk);
        println!("first register: {first:?}");
        let second = register(h2, 1, &hk);
        println!("same combo, other window: {second:?}");
        unregister(h1, 1);
        let again = register(h2, 1, &hk);
        println!("after unregister, other window: {again:?}");
        unregister(h2, 1);
        println!("foreground root: {:?}", foreground_window());
        unsafe {
            let _ = DestroyWindow(w1);
            let _ = DestroyWindow(w2);
        }
        first.expect("combination is taken by another program");
        assert!(second.is_err());
        assert!(again.is_ok());
    }
}
