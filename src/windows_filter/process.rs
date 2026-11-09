//! Process facts about a window: executable name and whether its process outranks ours.

use std::collections::HashMap;
use std::ffi::c_void;
use std::mem::size_of;
use std::sync::{Mutex, OnceLock};

use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM};
use windows::Win32::Security::{
    GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, TOKEN_MANDATORY_LABEL,
    TOKEN_QUERY, TokenIntegrityLevel,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::WindowsAndMessaging::EnumChildWindows;
use windows::core::{BOOL, PWSTR};

use crate::Hwnd;
use crate::win32::{class_name, process_id, to_hwnd};

const UWP_FRAME_CLASS: &str = "ApplicationFrameWindow";
const UWP_APP_CLASS: &str = "Windows.UI.Core.CoreWindow";
/// UTF-16 units; twice MAX_PATH so long paths still fit.
const IMAGE_NAME_CAPACITY: usize = 520;
/// Token label buffer in u64 words: 8-byte aligned, 256 bytes.
const LABEL_BUFFER_WORDS: usize = 32;
const MAX_ELEVATION_CACHE_ENTRIES: usize = 1024;

type ElevationCache = Mutex<HashMap<(Hwnd, u32), bool>>;

/// A kernel handle that is closed when dropped.
struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: the handle was opened by us and is closed exactly once.
        // A failed close leaves nothing to recover.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

fn open_process(pid: u32) -> Option<OwnedHandle> {
    // SAFETY: plain handle open; the guard closes it.
    unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok() }.map(OwnedHandle)
}

/// Lowercase executable file name of the window's process. For UWP windows (class `ApplicationFrameWindow`) it is the app's
/// own exe, not `applicationframehost.exe`; the host name is the fallback when the app window cannot be found.
pub(crate) fn exe_name(hwnd: Hwnd) -> Option<String> {
    let w = to_hwnd(hwnd);
    if class_name(w) == UWP_FRAME_CLASS
        && let Some(name) = uwp_app_pid(w).and_then(exe_of_pid)
    {
        return Some(name);
    }
    exe_of_pid(process_id(w))
}

/// For a UWP frame window, the process of its `Windows.UI.Core.CoreWindow` child (the real app, not ApplicationFrameHost).
fn uwp_app_pid(frame: HWND) -> Option<u32> {
    struct Search {
        host_pid: u32,
        found: Option<u32>,
    }
    unsafe extern "system" fn visit(child: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: lparam is the &mut Search passed below for the duration of EnumChildWindows.
        let s = unsafe { &mut *(lparam.0 as *mut Search) };
        let pid = process_id(child);
        if pid != 0 && pid != s.host_pid {
            s.found = Some(pid);
            if class_name(child) == UWP_APP_CLASS {
                return false.into();
            }
        }
        true.into()
    }
    let mut s = Search {
        host_pid: process_id(frame),
        found: None,
    };
    // SAFETY: the callback only uses lparam as the Search above.
    // Stopping the walk early (the callback returning false) is reported as an error; the result is in `s`.
    let _ =
        unsafe { EnumChildWindows(Some(frame), Some(visit), LPARAM(&mut s as *mut _ as isize)) };
    s.found
}

fn exe_of_pid(pid: u32) -> Option<String> {
    let process = open_process(pid)?;
    let mut buf = [0u16; IMAGE_NAME_CAPACITY];
    let mut len = buf.len() as u32;
    // SAFETY: buf/len describe a valid buffer.
    unsafe {
        QueryFullProcessImageNameW(
            process.0,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
    }
    .ok()?;
    let path = String::from_utf16_lossy(&buf[..len as usize]);
    path.rsplit('\\').next().map(str::to_lowercase)
}

/// True when the window's process cannot be inspected or runs at higher integrity than ours.
/// The answer is cached per (window, process id), because the sweep asks for every elevated window again.
pub(crate) fn is_elevated_target(hwnd: Hwnd) -> bool {
    static CACHE: OnceLock<ElevationCache> = OnceLock::new();
    let pid = process_id(to_hwnd(hwnd));
    let cache = CACHE.get_or_init(Default::default);
    cached_by_window_and_pid(cache, (hwnd, pid), || outranks_us(pid))
}

fn outranks_us(pid: u32) -> bool {
    let Some(process) = open_process(pid) else {
        return true;
    };
    match (integrity_rid(process.0), our_integrity()) {
        (Some(theirs), Some(ours)) => theirs > ours,
        _ => true,
    }
}

/// Looks the answer up by (window, process id) and computes it only on a miss. A process keeps its integrity level
/// for its whole life, so the entry cannot go stale unless both the handle and the pid come back for another process.
fn cached_by_window_and_pid(
    cache: &ElevationCache,
    key: (Hwnd, u32),
    compute: impl FnOnce() -> bool,
) -> bool {
    if let Some(&v) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        return v;
    }
    let v = compute();
    let mut map = cache.lock().unwrap_or_else(|e| e.into_inner());
    if map.len() >= MAX_ELEVATION_CACHE_ENTRIES {
        map.clear();
    }
    map.insert(key, v);
    v
}

/// Our own integrity level, read once.
fn our_integrity() -> Option<u32> {
    static OURS: OnceLock<Option<u32>> = OnceLock::new();
    // SAFETY: pseudo handle, needs no closing.
    *OURS.get_or_init(|| integrity_rid(unsafe { GetCurrentProcess() }))
}

fn integrity_rid(process: HANDLE) -> Option<u32> {
    let mut token = HANDLE::default();
    // SAFETY: the token is closed by the guard below.
    unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token).ok()? };
    token_integrity_rid(&OwnedHandle(token))
}

/// Last sub-authority of the token's mandatory-label SID: the integrity RID.
fn token_integrity_rid(token: &OwnedHandle) -> Option<u32> {
    let mut buf = [0u64; LABEL_BUFFER_WORDS];
    let mut len = 0u32;
    // SAFETY: buf is a valid buffer of the size we pass; the label SID lives inside it.
    unsafe {
        GetTokenInformation(
            token.0,
            TokenIntegrityLevel,
            Some(buf.as_mut_ptr() as *mut c_void),
            size_of::<[u64; LABEL_BUFFER_WORDS]>() as u32,
            &mut len,
        )
        .ok()?;
        let label = &*(buf.as_ptr() as *const TOKEN_MANDATORY_LABEL);
        let sid = label.Label.Sid;
        let count = *GetSidSubAuthorityCount(sid);
        if count == 0 {
            return None;
        }
        Some(*GetSidSubAuthority(sid, u32::from(count) - 1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elevation_is_computed_once_per_window_and_pid() {
        let cache = Mutex::new(HashMap::new());
        let mut calls = 0;
        let mut ask = |key, answer| {
            cached_by_window_and_pid(&cache, key, || {
                calls += 1;
                answer
            })
        };
        assert!(ask((1, 10), true));
        assert!(ask((1, 10), false)); // cached, the new answer is not computed
        assert!(!ask((1, 11), false)); // same handle, other process: asked again
        assert!(!ask((2, 10), false));
        assert_eq!(calls, 3);
    }
}
