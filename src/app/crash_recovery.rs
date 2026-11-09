//! Crash recovery: `pins.json` lists the pinned windows so the next start can undo the pins of a
//! run that did not shut down.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
use windows::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
};

use super::logging::{info, warn};
use super::winutil::window_alive;
use crate::Hwnd;
use crate::config::guard_write_target;
use crate::win32::{process_id, to_hwnd};
use crate::zorder;

const PINS_FILE_NAME: &str = "pins.json";

/// Where `pins.json` lives for a given settings file.
pub(super) fn pins_path_beside(settings_path: &Path) -> PathBuf {
    settings_path.with_file_name(PINS_FILE_NAME)
}

/// One pinned window as written to `pins.json`. `pid` and `created` (process creation time as a
/// FILETIME value) identify the owning process so a reused window handle is never touched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct SavedPin {
    pub(super) hwnd: Hwnd,
    pub(super) pid: u32,
    pub(super) created: u64,
    pub(super) was_topmost: bool,
    /// The pin was already taken back but the window did not accept it yet; the next start retries.
    #[serde(default)]
    pub(super) pending_release: bool,
}

/// Process id and creation time of the process that owns `h`.
pub(super) fn window_identity(h: Hwnd) -> Option<(u32, u64)> {
    let pid = process_id(to_hwnd(h));
    if pid == 0 {
        return None;
    }
    // SAFETY: the handle is closed below.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let created = process_creation_time(process);
    // SAFETY: handle from OpenProcess above. Nothing useful to do if closing fails.
    let _ = unsafe { CloseHandle(process) };
    Some((pid, created?))
}

fn process_creation_time(process: HANDLE) -> Option<u64> {
    let [mut created, mut exit, mut kernel, mut user] = [FILETIME::default(); 4];
    // SAFETY: all four out pointers are valid for the call.
    unsafe { GetProcessTimes(process, &mut created, &mut exit, &mut kernel, &mut user) }.ok()?;
    Some(u64::from(created.dwHighDateTime) << 32 | u64::from(created.dwLowDateTime))
}

/// Writes the list atomically (temp file, then rename); an empty list deletes the file. Neither the
/// file nor its directory may be a symlink or junction. Failures are logged: a pin must still work
/// when the disk does not.
pub(super) fn write_pins(path: &Path, pins: &[SavedPin]) {
    let result = if pins.is_empty() {
        remove_pins_file(path)
    } else {
        replace_pins_file(path, pins)
    };
    if let Err(e) = result {
        warn(&format!("cannot update {}: {e}", path.display()));
    }
}

fn remove_pins_file(path: &Path) -> io::Result<()> {
    guard_write_target(path)?;
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

fn replace_pins_file(path: &Path, pins: &[SavedPin]) -> io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    guard_write_target(path)?;
    guard_write_target(&tmp)?;
    let json = serde_json::to_string(pins).map_err(io::Error::other)?;
    let written = write_new_file(&tmp, &json).and_then(|()| std::fs::rename(&tmp, path));
    if written.is_err() {
        // The error that matters is the write or rename above.
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

/// Creates the file, failing when anything (a stale temp file, a link) is already there.
fn write_new_file(path: &Path, text: &str) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(text.as_bytes())
}

/// Clears pins left behind by a run that did not shut down: for every listed window that still
/// exists, belongs to the same process (pid and creation time), is topmost and was not topmost
/// before Pin It pinned it, removes the topmost flag. Then deletes the file; entries whose
/// release failed stay in it, and their windows are returned so the running app keeps retrying them.
/// Anything that does not match is left alone.
pub(super) fn recover_pins(path: &Path) -> Vec<Hwnd> {
    if let Err(e) = guard_write_target(path) {
        warn(&format!("not reading {}: {e}", path.display()));
        return Vec::new();
    }
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let leftover = match serde_json::from_str::<Vec<SavedPin>>(&text) {
        Ok(list) => list
            .into_iter()
            .filter(is_stale_pin_of_ours)
            .filter(|pin| !clear_pin(pin))
            .collect(),
        Err(e) => {
            warn(&format!("ignoring unreadable {}: {e}", path.display()));
            Vec::new()
        }
    };
    write_pins(path, &leftover);
    leftover.iter().map(|pin| pin.hwnd).collect()
}

fn is_stale_pin_of_ours(pin: &SavedPin) -> bool {
    window_alive(pin.hwnd)
        && window_identity(pin.hwnd) == Some((pin.pid, pin.created))
        && zorder::is_topmost(pin.hwnd)
        && !pin.was_topmost
}

/// Returns false when the release failed and the entry has to be kept.
fn clear_pin(pin: &SavedPin) -> bool {
    match zorder::unpin(pin.hwnd) {
        Ok(()) | Err(zorder::ApplyError::Gone) => {
            let kind = if pin.pending_release {
                "pending release"
            } else {
                "stale pin"
            };
            info(&format!(
                "cleared {kind} of {:#x} (pid {})",
                pin.hwnd, pin.pid
            ));
            true
        }
        Err(e) => {
            warn(&format!("cannot clear pin of {:#x}: {e:?}", pin.hwnd));
            false
        }
    }
}
