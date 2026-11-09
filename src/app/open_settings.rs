//! "Open settings file": the shell call runs from a posted message, never inside an `App` method.
//!
//! `ShellExecuteW` can pump messages while it works. `with_app` re-posts every message while an
//! `App` method runs, so a pumping call made inside one could make the app spin on its own
//! messages. The tray command only posts `WM_OPEN_SETTINGS`; the window procedure reads the path
//! and calls `shell_open` after the borrow of the `App` has ended.
//!
//! A machine can have no usable app for `.json` (`FindExecutableW` answers "no association"); then
//! `ShellExecuteW` fails or only shows the "Open with" chooser. The file is opened in Notepad
//! instead, which every Windows has.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows::Win32::Foundation::{CloseHandle, LPARAM, WPARAM};
use windows::Win32::System::Threading::{
    CreateProcessW, PROCESS_CREATION_FLAGS, PROCESS_INFORMATION, STARTUPINFOW,
};
use windows::Win32::UI::Shell::{FindExecutableW, ShellExecuteW};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, SW_SHOWNORMAL};
use windows::core::{PCWSTR, PWSTR, w};

use super::host::WM_OPEN_SETTINGS;
use super::logging::{info, warn};
use super::state::App;
use crate::win32::{to_hwnd, wide};

/// `ShellExecuteW` and `FindExecutableW` report success with a value above 32.
const SHELL_MAX_ERROR_CODE: isize = 32;
/// `MAX_PATH`, the buffer size `FindExecutableW` asks for.
const FOUND_PATH_CAPACITY: usize = 260;
const NOTEPAD: &str = "notepad.exe";

impl App {
    /// Makes sure the file exists (a first run has none yet) and asks the window procedure to
    /// open it.
    pub(super) fn request_open_settings_file(&mut self) -> Result<(), String> {
        if !self.settings_file.path.exists() {
            self.save_settings();
        }
        if !self.settings_file.path.exists() {
            return Err("the settings file does not exist and could not be written".into());
        }
        // SAFETY: posts to our own window.
        unsafe {
            PostMessageW(
                Some(to_hwnd(self.hwnd)),
                WM_OPEN_SETTINGS,
                WPARAM(0),
                LPARAM(0),
            )
        }
        .map_err(|e| format!("cannot open settings file: {e}"))
    }
}

/// Opens `path` with its default app, or in Notepad when that does not work. Failures are logged
/// with the shell's code, so the log tells why nothing opened.
pub(super) fn shell_open(path: &Path) {
    let Err(failure) = open_with_default_app(path) else {
        return;
    };
    // Many machines have no app for .json: the fallback is the normal path there, not a problem.
    info(&format!(
        "opening settings file with Notepad (no default app for .json: {failure})"
    ));
    if let Err(failure) = open_in_notepad(path) {
        warn(&format!("cannot open settings file in Notepad: {failure}"));
    }
}

fn open_with_default_app(path: &Path) -> Result<(), String> {
    #[cfg(test)]
    if let Some(result) = recorder::record(recorder::Opener::DefaultApp, path) {
        return result;
    }
    let wide_path = wide_path(path);
    check_association(&wide_path)?;
    // SAFETY: `wide_path` is NUL-terminated and outlives the call.
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(wide_path.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        )
    };
    shell_result("ShellExecuteW", result.0 as isize)
}

/// Whether the shell knows an app for the file; without one `ShellExecuteW` would show the
/// "Open with" chooser (or fail), and the user sees nothing useful.
fn check_association(wide_path: &[u16]) -> Result<(), String> {
    let mut found = [0u16; FOUND_PATH_CAPACITY];
    // SAFETY: `wide_path` is NUL-terminated; `found` is the buffer the API wants.
    let result = unsafe { FindExecutableW(PCWSTR(wide_path.as_ptr()), PCWSTR::null(), &mut found) };
    shell_result("FindExecutableW", result.0 as isize)
}

fn shell_result(call: &str, code: isize) -> Result<(), String> {
    if code > SHELL_MAX_ERROR_CODE {
        Ok(())
    } else {
        Err(format!("{call} returned {code}"))
    }
}

fn open_in_notepad(path: &Path) -> Result<(), String> {
    let command_line = notepad_command_line(path);
    #[cfg(test)]
    if let Some(result) = recorder::record(recorder::Opener::Notepad(command_line.clone()), path) {
        return result;
    }
    let program = wide(&notepad_program().to_string_lossy());
    let mut buffer = wide(&command_line);
    let startup = STARTUPINFOW {
        cb: size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    let mut process = PROCESS_INFORMATION::default();
    // SAFETY: `program` and `buffer` are NUL-terminated and `buffer` is writable; the structs outlive the call.
    unsafe {
        CreateProcessW(
            PCWSTR(program.as_ptr()),
            Some(PWSTR(buffer.as_mut_ptr())),
            None,
            None,
            false,
            PROCESS_CREATION_FLAGS(0),
            None,
            PCWSTR::null(),
            &startup,
            &mut process,
        )
    }
    .map_err(|e| e.to_string())?;
    // SAFETY: the handles came from the successful call above; closing does not stop Notepad.
    unsafe {
        let _ = CloseHandle(process.hProcess);
        let _ = CloseHandle(process.hThread);
    }
    Ok(())
}

/// Notepad in the Windows directory, so a `notepad.exe` in the current directory is never started.
fn notepad_program() -> std::path::PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:Windows".into());
    Path::new(&root).join(NOTEPAD)
}

/// `notepad.exe "<path>"`: a path cannot contain a quote, so quoting it is enough.
fn notepad_command_line(path: &Path) -> String {
    format!("{NOTEPAD} \"{}\"", path.display())
}

fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

#[cfg(test)]
pub(super) mod recorder {
    use std::cell::{Cell, RefCell};
    use std::path::{Path, PathBuf};

    use crate::app::host::is_busy;

    /// Which way of opening the file was asked for.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) enum Opener {
        DefaultApp,
        /// The command line the Notepad fallback would start.
        Notepad(String),
    }

    /// One recorded call: the way, the path and whether an `App` method was running.
    pub(crate) struct OpenCall {
        pub(crate) opener: Opener,
        pub(crate) path: PathBuf,
        pub(crate) app_busy: bool,
    }

    thread_local! {
        static RECORDER: RefCell<Option<Vec<OpenCall>>> = const { RefCell::new(None) };
        static DEFAULT_APP_FAILS: Cell<bool> = const { Cell::new(false) };
    }

    /// Starts recording instead of opening a real editor.
    pub(crate) fn start_recording() {
        DEFAULT_APP_FAILS.with(|f| f.set(false));
        RECORDER.with(|r| *r.borrow_mut() = Some(Vec::new()));
    }

    /// Like `start_recording`, and the default app reports a failure, so the fallback is tried.
    pub(crate) fn start_recording_with_failing_default_app() {
        start_recording();
        DEFAULT_APP_FAILS.with(|f| f.set(true));
    }

    /// Stops recording and returns the calls.
    pub(crate) fn take_recorded() -> Vec<OpenCall> {
        DEFAULT_APP_FAILS.with(|f| f.set(false));
        RECORDER.with(|r| r.borrow_mut().take().unwrap_or_default())
    }

    /// Records the call when a test asked for it and returns the outcome to use instead of the
    /// real one; `None` when nobody records.
    pub(super) fn record(opener: Opener, path: &Path) -> Option<Result<(), String>> {
        let fails = opener == Opener::DefaultApp && DEFAULT_APP_FAILS.with(Cell::get);
        RECORDER.with(|r| {
            r.borrow_mut().as_mut().map(|calls| {
                calls.push(OpenCall {
                    opener,
                    path: path.to_path_buf(),
                    app_busy: is_busy(),
                });
                if fails {
                    Err("test: no app for the file".to_string())
                } else {
                    Ok(())
                }
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_codes_up_to_32_are_errors() {
        assert!(shell_result("x", 33).is_ok());
        assert_eq!(
            shell_result("FindExecutableW", 31).unwrap_err(),
            "FindExecutableW returned 31"
        );
        assert!(shell_result("x", 0).is_err());
        assert!(shell_result("x", 32).is_err());
    }

    #[test]
    fn notepad_starts_from_the_windows_directory() {
        let program = notepad_program();
        assert!(program.is_absolute(), "{program:?}");
        assert!(program.ends_with(NOTEPAD));
        assert!(program.exists(), "{program:?}");
    }

    #[test]
    fn notepad_gets_the_quoted_path() {
        let line = notepad_command_line(Path::new(r"C:\Users\a b\settings.json"));
        assert_eq!(line, "notepad.exe \"C:\\Users\\a b\\settings.json\"");
    }
}
