//! List of pinned windows that a panic hook can release without access to the `App`.

use std::sync::Mutex;
use std::sync::mpsc;
use std::time::Duration;

use super::logging::log;
use crate::Hwnd;
use crate::zorder;

/// How long the panic hook waits for the worker that talks to other processes' windows.
const RELEASE_TIMEOUT: Duration = Duration::from_secs(1);

/// Windows that Pin It made topmost (`true` = it was topmost before).
static PINNED: Mutex<Vec<(Hwnd, bool)>> = Mutex::new(Vec::new());

/// Replaces the list; called whenever the pin list changes.
pub(super) fn replace(list: Vec<(Hwnd, bool)>) {
    *PINNED.lock().unwrap_or_else(|e| e.into_inner()) = list;
}

#[cfg(test)]
pub(super) fn current() -> Vec<(Hwnd, bool)> {
    PINNED.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

pub(super) fn clear() {
    replace(Vec::new());
}

/// Best effort after a panic: unpins the pinned windows.
///
/// Never blocks the panicking thread for long: it only `try_lock`s the list and waits at most one
/// second for the worker that talks to the other processes' windows.
pub(crate) fn release_pinned_after_panic() {
    let list = match PINNED.try_lock() {
        Ok(guard) => guard.clone(),
        Err(std::sync::TryLockError::Poisoned(e)) => e.into_inner().clone(),
        Err(std::sync::TryLockError::WouldBlock) => return,
    };
    if list.is_empty() {
        return;
    }
    let (done, finished) = mpsc::channel();
    std::thread::spawn(move || {
        release_all(&list);
        // The receiver is gone when the hook timed out; nobody is left to tell.
        let _ = done.send(());
    });
    // On timeout the process is going down anyway; the worker is abandoned.
    let _ = finished.recv_timeout(RELEASE_TIMEOUT);
}

/// Best effort while the process is going down: a failure is written to the log, nothing more can be done.
fn release_all(list: &[(Hwnd, bool)]) {
    for &(h, _) in list.iter().filter(|(_, was_topmost)| !was_topmost) {
        if let Err(e) = zorder::unpin(h) {
            log("PANIC", &format!("cannot unpin {h:#x}: {e:?}"));
        }
    }
}

pub(super) fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        // Release builds abort after the hook returns, so the line must be written here.
        log("PANIC", &info.to_string());
        release_pinned_after_panic();
    }));
}
