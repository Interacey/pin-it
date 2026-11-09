//! Startup, shutdown and the message loop.

use std::path::PathBuf;
use std::rc::Rc;
use std::time::SystemTime;

use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyWindow, DispatchMessageW, GetMessageW, KillTimer, MSG, SetTimer, TranslateMessage,
};
use windows::core::{PCWSTR, w};

use super::crash_recovery::{pins_path_beside, recover_pins};
use super::host::{
    SWEEP_INTERVAL_MS, SWEEP_TIMER, WM_HOOK_EVENTS, WM_TRAY, allow_tray_messages, clear_app,
    create_main_window, install_app, reset_session_flags, with_app,
};
use super::logging::{info, set_log_path_beside, warn};
use super::panic_registry;
use super::settings_service::log_load_status;
use super::state::App;
use crate::Hwnd;
use crate::color::WM_COLOR_READY;
use crate::config::{self, Settings};
use crate::dial;
use crate::events::EventHooks;
use crate::exclusions;
use crate::tray::{Tray, taskbar_created_message};
use crate::win32::to_hwnd;

const MUTEX_NAME: PCWSTR = w!("Local\\PinIt-single-instance");

/// How to start the app.
pub(crate) struct RunOptions {
    /// Settings file; the log file and `pins.json` live next to it.
    pub(crate) settings_path: PathBuf,
    /// Create the tray icon.
    pub(crate) tray: bool,
    /// If `Some`, only these windows get buttons and the usual pinnable check is skipped for them.
    pub(crate) only: Option<Vec<Hwnd>>,
    /// Exit silently when another instance runs.
    pub(crate) single_instance: bool,
    /// Register the global hotkey.
    pub(crate) hotkey: bool,
    /// Keep the Run-key autostart entry in sync with the settings.
    pub(crate) autostart: bool,
}

impl RunOptions {
    /// Options of the real application.
    pub(crate) fn real() -> Self {
        RunOptions {
            settings_path: config::default_path(),
            tray: true,
            only: None,
            single_instance: true,
            hotkey: true,
            autostart: true,
        }
    }
}

impl App {
    /// Unpins everything, drops buttons, hooks, tray and hotkey. Safe to call twice.
    pub(super) fn shutdown(&mut self) {
        if self.down {
            return;
        }
        self.down = true;
        self.unpin_all();
        self.entries.clear();
        self.hooks = None;
        self.tray = None;
        self.unregister_hotkey();
        if let Err(e) = dial::close() {
            warn(&format!("button size dial could not be closed: {e}"));
        }
        if let Err(e) = exclusions::close() {
            warn(&format!("excluded apps window could not be closed: {e}"));
        }
        // SAFETY: our own window and timer.
        // Failing is expected when the timer never started (reported by `start_sweep_timer`).
        let _ = unsafe { KillTimer(Some(to_hwnd(self.hwnd)), SWEEP_TIMER) };
        info("shut down");
    }

    /// Creates the tray icon and the event hooks; a failure is logged and the app runs without.
    fn attach_integrations(&mut self, tray_wanted: bool) {
        let h = self.hwnd;
        self.colors.set_notify(h, WM_COLOR_READY);
        if tray_wanted {
            match Tray::new(h, WM_TRAY) {
                Ok(tray) => {
                    self.tray = Some(Rc::new(tray));
                    self.add_tray_icon();
                }
                Err(e) => warn(&format!("tray icon failed: {e}")),
            }
        }
        match EventHooks::install(h, WM_HOOK_EVENTS) {
            Ok(hooks) => self.hooks = Some(hooks),
            Err(e) => warn(&format!("event hooks failed: {e}")),
        }
    }

    fn apply_startup_settings(&mut self) {
        self.sync_hotkey();
        if self.hotkey.wanted && self.hotkey.active.is_none() && self.settings.hotkey.is_none() {
            info("hotkey: none");
        }
        self.sync_autostart();
    }
}

/// Reads the settings file and logs what was wrong with it.
fn load_settings(opts: &RunOptions) -> (Settings, Option<SystemTime>) {
    let mtime = config::modified_time(&opts.settings_path);
    let (settings, status) = config::load_report_from(&opts.settings_path);
    info(&format!(
        "start, settings from {}",
        opts.settings_path.display()
    ));
    log_load_status(&status);
    (settings, mtime)
}

/// Starts the 2 s sweep. Without it new windows get no button, pins are not re-applied and dead
/// windows are not cleaned up, so a failure is logged as the serious problem it is; the app
/// keeps running because the tray menu (Exit, Unpin all) still works.
fn start_sweep_timer(hwnd: HWND) {
    // SAFETY: own window and timer id.
    let timer = unsafe { SetTimer(Some(hwnd), SWEEP_TIMER, SWEEP_INTERVAL_MS, None) };
    if timer == 0 {
        warn(&format!(
            "sweep timer could not be started ({}): new windows, re-pinning and cleanup are off",
            windows::core::Error::from_thread()
        ));
    }
}

/// A started app on the current thread. The caller must pump messages (`run_with` does).
///
/// Dropping it shuts everything down: pins are reset, buttons, hooks, tray and hotkey removed.
pub(crate) struct Runtime {
    pub(super) ptr: *mut App,
    pub(super) hwnd: Hwnd,
}

impl Runtime {
    /// Loads settings, creates the owner window and wires everything up.
    pub(crate) fn start(opts: &RunOptions) -> Result<Runtime, String> {
        set_log_path_beside(&opts.settings_path);
        let (settings, mtime) = load_settings(opts);
        let hwnd = create_main_window().map_err(|e| format!("main window: {e}"))?;
        let h = hwnd.0 as Hwnd;
        panic_registry::clear();
        reset_session_flags();
        allow_tray_messages(hwnd);
        let unreleased = recover_pins(&pins_path_beside(&opts.settings_path));
        let mut app = App::new(h, opts, settings, mtime);
        app.pending_release.extend(unreleased);
        app.attach_integrations(opts.tray);
        app.apply_startup_settings();
        let ptr = Box::into_raw(Box::new(app));
        install_app(ptr, taskbar_created_message());
        start_sweep_timer(hwnd);
        with_app(App::sweep);
        Ok(Runtime { ptr, hwnd: h })
    }
}

/// Test access to the running app.
#[cfg(test)]
impl Runtime {
    /// The hidden owner window; post `WM_SWEEP_NOW` or `WM_CLOSE` to it.
    pub(crate) fn hwnd(&self) -> Hwnd {
        self.hwnd
    }

    /// Pinned windows, oldest first.
    pub(crate) fn pinned(&self) -> Vec<Hwnd> {
        // SAFETY: short-lived read on the owning thread, outside any message dispatch.
        unsafe { (*self.ptr).pinned_order() }
    }

    /// Overlay window of the button for `target`, if there is one.
    pub(crate) fn button_hwnd(&self, target: Hwnd) -> Option<Hwnd> {
        // SAFETY: see `pinned`.
        unsafe { (*self.ptr).entries.get(&target).map(|e| e.button.hwnd()) }
    }

    /// Number of windows that have a button.
    pub(crate) fn button_count(&self) -> usize {
        // SAFETY: see `pinned`.
        unsafe { (*self.ptr).entries.len() }
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        with_app(App::shutdown);
        clear_app();
        // SAFETY: ptr came from Box::into_raw in `start` and was cleared from CURRENT just above.
        unsafe {
            drop(Box::from_raw(self.ptr));
            // The window may already be gone (WM_CLOSE in a test); nothing left to clean up then.
            let _ = DestroyWindow(to_hwnd(self.hwnd));
        }
    }
}

/// Holds the named mutex that makes the app single-instance.
struct InstanceGuard(HANDLE);

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        // SAFETY: handle from CreateMutexW. Nothing useful to do if closing fails.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// `Ok(Some(guard))` when this is the only instance, `Ok(None)` when the mutex could not be
/// created (the app runs anyway), `Err(())` when another instance already runs.
fn acquire_single_instance() -> Result<Option<InstanceGuard>, ()> {
    // SAFETY: named mutex; the handle is owned by the returned guard.
    let created = unsafe { CreateMutexW(None, false, MUTEX_NAME) };
    // SAFETY: plain query right after the call above.
    let already_running = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    match created {
        Ok(handle) if already_running => {
            drop(InstanceGuard(handle));
            Err(())
        }
        Ok(handle) => Ok(Some(InstanceGuard(handle))),
        Err(e) => {
            warn(&format!("single-instance mutex failed: {e}"));
            Ok(None)
        }
    }
}

fn pump_messages() {
    let mut msg = MSG::default();
    // SAFETY: standard message loop; GetMessageW returns 0 on WM_QUIT and -1 on error.
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            // Returns whether a character message was produced, not an error.
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Runs the app until it is told to quit.
pub(crate) fn run_with(opts: RunOptions) {
    set_log_path_beside(&opts.settings_path);
    let _instance = if opts.single_instance {
        match acquire_single_instance() {
            Ok(guard) => guard,
            Err(()) => {
                info("another instance is already running, exiting");
                return;
            }
        }
    } else {
        None
    };
    let runtime = match Runtime::start(&opts) {
        Ok(runtime) => runtime,
        Err(e) => {
            warn(&format!("start failed: {e}"));
            return;
        }
    };
    pump_messages();
    drop(runtime);
    info("exit");
}

/// Entry point of the real application.
pub fn run() {
    // SAFETY: first call of the process; a failure means awareness was already set (manifest).
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    // ShellExecuteW and the color dialog need COM on this thread.
    // SAFETY: balanced by CoUninitialize below when it succeeded.
    let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
    let opts = RunOptions::real();
    set_log_path_beside(&opts.settings_path);
    panic_registry::install_panic_hook();
    run_with(opts);
    if com {
        // SAFETY: matches the successful CoInitializeEx above.
        unsafe { CoUninitialize() };
    }
}
