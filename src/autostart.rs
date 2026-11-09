//! Autostart registration (HKCU Run key).

use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE,
    REG_SAM_FLAGS, REG_SZ, REG_VALUE_TYPE, RegCloseKey, RegCreateKeyExW, RegDeleteValueW,
    RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows::core::PCWSTR;

use crate::win32::wide;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "PinIt";
const UTF16_UNIT_BYTES: usize = 2;
const DOUBLE_QUOTE: u16 = b'"' as u16;

fn check(e: WIN32_ERROR) -> io::Result<()> {
    if e == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(e.0 as i32))
    }
}

/// An open registry key under HKCU, closed on drop.
struct RegKey(HKEY);

impl RegKey {
    /// Opens `subkey`; the Win32 error code says why it failed (e.g. `ERROR_FILE_NOT_FOUND`).
    fn open(subkey: &str, access: REG_SAM_FLAGS) -> Result<RegKey, WIN32_ERROR> {
        let sub = wide(subkey);
        let mut key = HKEY::default();
        // SAFETY: sub is NUL-terminated and outlives the call; key is a valid out pointer.
        let code = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(sub.as_ptr()),
                None,
                access,
                &mut key,
            )
        };
        if code == ERROR_SUCCESS {
            Ok(RegKey(key))
        } else {
            Err(code)
        }
    }

    /// Opens `subkey` for writing, creating it if it does not exist.
    fn create(subkey: &str) -> io::Result<RegKey> {
        let sub = wide(subkey);
        let mut key = HKEY::default();
        // SAFETY: sub is NUL-terminated; key is a valid out pointer.
        check(unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(sub.as_ptr()),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                None,
                &mut key,
                None,
            )
        })?;
        Ok(RegKey(key))
    }
}

impl Drop for RegKey {
    fn drop(&mut self) {
        // SAFETY: the key was opened by `open`/`create` and is closed exactly once.
        // A failed close cannot be acted on.
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

/// Returns true if `value_name` exists under HKCU\`subkey`.
#[cfg(test)]
fn is_enabled_in(subkey: &str, value_name: &str) -> bool {
    let Ok(key) = RegKey::open(subkey, KEY_QUERY_VALUE) else {
        return false;
    };
    let name = wide(value_name);
    // SAFETY: key is open; all-None data arguments only query existence.
    let r = unsafe { RegQueryValueExW(key.0, PCWSTR(name.as_ptr()), None, None, None, None) };
    r == ERROR_SUCCESS
}

/// Adds (`on`) or removes the quoted exe path as a REG_SZ value under HKCU\`subkey`.
fn set_enabled_in(subkey: &str, value_name: &str, exe: &Path, on: bool) -> io::Result<()> {
    if on {
        write_quoted_path(subkey, value_name, exe)
    } else {
        remove_value(subkey, value_name)
    }
}

/// Deleting something that is not there counts as success.
fn remove_value(subkey: &str, value_name: &str) -> io::Result<()> {
    let key = match RegKey::open(subkey, KEY_SET_VALUE) {
        Ok(key) => key,
        Err(ERROR_FILE_NOT_FOUND) => return Ok(()),
        Err(code) => return check(code),
    };
    let name = wide(value_name);
    // SAFETY: key is open and name is NUL-terminated.
    let r = unsafe { RegDeleteValueW(key.0, PCWSTR(name.as_ptr())) };
    if r == ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        check(r)
    }
}

fn write_quoted_path(subkey: &str, value_name: &str, exe: &Path) -> io::Result<()> {
    let key = RegKey::create(subkey)?;
    let name = wide(value_name);
    let quoted: Vec<u16> = std::iter::once(DOUBLE_QUOTE)
        .chain(exe.as_os_str().encode_wide())
        .chain([DOUBLE_QUOTE, 0])
        .collect();
    // SAFETY: the byte view covers exactly the u16 buffer, which outlives the slice.
    let bytes = unsafe {
        std::slice::from_raw_parts(
            quoted.as_ptr().cast::<u8>(),
            quoted.len() * UTF16_UNIT_BYTES,
        )
    };
    // SAFETY: key is open, name is NUL-terminated, bytes is a NUL-terminated UTF-16 string.
    let r = unsafe { RegSetValueExW(key.0, PCWSTR(name.as_ptr()), None, REG_SZ, Some(bytes)) };
    check(r)
}

/// Reads the string value `value_name` under HKCU\`subkey`; `None` if missing or not a string.
fn registered_path_in(subkey: &str, value_name: &str) -> Option<String> {
    let key = RegKey::open(subkey, KEY_QUERY_VALUE).ok()?;
    let name = wide(value_name);
    let byte_len = string_value_byte_len(&key, &name)?;
    read_string_value(&key, &name, byte_len)
}

/// Size in bytes of a REG_SZ value that can hold at least one UTF-16 unit; `None` for other types.
fn string_value_byte_len(key: &RegKey, name: &[u16]) -> Option<u32> {
    let mut ty = REG_VALUE_TYPE(0);
    let mut len = 0u32;
    // SAFETY: key is open; a null data pointer only asks for the size.
    let r = unsafe {
        RegQueryValueExW(
            key.0,
            PCWSTR(name.as_ptr()),
            None,
            Some(&mut ty),
            None,
            Some(&mut len),
        )
    };
    (r == ERROR_SUCCESS && ty == REG_SZ && len as usize >= UTF16_UNIT_BYTES).then_some(len)
}

fn read_string_value(key: &RegKey, name: &[u16], byte_len: u32) -> Option<String> {
    let mut buf = vec![0u16; (byte_len as usize).div_ceil(UTF16_UNIT_BYTES)];
    let mut got = (buf.len() * UTF16_UNIT_BYTES) as u32;
    // SAFETY: buf holds `got` bytes; the call writes at most that many.
    let r = unsafe {
        RegQueryValueExW(
            key.0,
            PCWSTR(name.as_ptr()),
            None,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut got),
        )
    };
    if r != ERROR_SUCCESS {
        return None;
    }
    let units = &buf[..(got as usize / UTF16_UNIT_BYTES).min(buf.len())];
    let end = units.iter().position(|&c| c == 0).unwrap_or(units.len());
    Some(String::from_utf16_lossy(&units[..end]))
}

/// True if the value equals the quoted `exe` path (case-insensitive, as Windows paths are).
fn is_registered_for_in(subkey: &str, value_name: &str, exe: &Path) -> bool {
    let want = format!("\"{}\"", exe.display());
    registered_path_in(subkey, value_name).is_some_and(|v| v.eq_ignore_ascii_case(&want))
}

/// True if the Run value is exactly the quoted path of the running exe.
pub(crate) fn is_registered_for_current_exe() -> bool {
    std::env::current_exe().is_ok_and(|exe| is_registered_for_in(RUN_KEY, VALUE_NAME, &exe))
}

/// Registers or unregisters the current exe in the user's Run key.
pub(crate) fn set_enabled(on: bool) -> io::Result<()> {
    let exe = std::env::current_exe()?;
    set_enabled_in(RUN_KEY, VALUE_NAME, &exe, on)
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Registry::RegDeleteTreeW;

    /// Deletes the test subkey on drop, also when the test fails.
    struct TestKey(String);

    impl TestKey {
        fn new(tag: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos();
            TestKey(format!(
                "Software\\PinItTest-{tag}-{}-{nanos}",
                std::process::id()
            ))
        }
    }

    impl Drop for TestKey {
        fn drop(&mut self) {
            let sub = wide(&self.0);
            // SAFETY: sub is NUL-terminated; only our own test subkey is deleted.
            unsafe {
                let _ = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(sub.as_ptr()));
            }
        }
    }

    fn read_value(subkey: &str, name: &str) -> String {
        let (sub, name) = (wide(subkey), wide(name));
        let mut key = HKEY::default();
        let mut buf = [0u16; 512];
        let mut len = (buf.len() * 2) as u32;
        let mut ty = REG_VALUE_TYPE(0);
        // SAFETY: valid NUL-terminated strings and out pointers; len matches the buffer size.
        unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(sub.as_ptr()),
                None,
                KEY_QUERY_VALUE,
                &mut key,
            )
            .ok()
            .unwrap();
            let r = RegQueryValueExW(
                key,
                PCWSTR(name.as_ptr()),
                None,
                Some(&mut ty),
                Some(buf.as_mut_ptr().cast()),
                Some(&mut len),
            );
            let _ = RegCloseKey(key);
            r.ok().unwrap();
        }
        assert_eq!(ty, REG_SZ);
        String::from_utf16_lossy(&buf[..(len as usize / 2).saturating_sub(1)])
    }

    #[test]
    fn set_read_and_delete() {
        let k = TestKey::new("a");
        let exe = Path::new(r"C:\Program Files\Pin It\pin_it.exe");
        assert!(!is_enabled_in(&k.0, "PinIt"));
        set_enabled_in(&k.0, "PinIt", exe, true).unwrap();
        assert!(is_enabled_in(&k.0, "PinIt"));
        assert_eq!(
            read_value(&k.0, "PinIt"),
            r#""C:\Program Files\Pin It\pin_it.exe""#
        );
        set_enabled_in(&k.0, "PinIt", exe, false).unwrap();
        assert!(!is_enabled_in(&k.0, "PinIt"));
    }

    #[test]
    fn registered_path_and_current_exe_match() {
        let k = TestKey::new("c");
        let exe = Path::new(r"C:\Tools\pin_it.exe");
        assert_eq!(registered_path_in(&k.0, "PinIt"), None);
        assert!(!is_registered_for_in(&k.0, "PinIt", exe));
        set_enabled_in(&k.0, "PinIt", exe, true).unwrap();
        assert_eq!(
            registered_path_in(&k.0, "PinIt").as_deref(),
            Some(r#""C:\Tools\pin_it.exe""#)
        );
        assert!(is_registered_for_in(&k.0, "PinIt", exe));
        assert!(is_registered_for_in(
            &k.0,
            "PinIt",
            Path::new(r"c:\tools\PIN_IT.exe")
        ));
        assert!(!is_registered_for_in(
            &k.0,
            "PinIt",
            Path::new(r"C:\Other\pin_it.exe")
        ));
    }

    #[test]
    fn disable_is_idempotent() {
        let k = TestKey::new("b");
        let x = Path::new("x.exe");
        set_enabled_in(&k.0, "PinIt", x, false).unwrap();
        set_enabled_in(&k.0, "PinIt", x, true).unwrap();
        set_enabled_in(&k.0, "PinIt", x, false).unwrap();
        set_enabled_in(&k.0, "PinIt", x, false).unwrap();
    }
}
