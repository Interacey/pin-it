//! WinEvent hook plumbing: turns system window events into a coalesced queue.
//!
//! The thread that calls `EventHooks::install` must run a Win32 message loop; out-of-context
//! hook callbacks are only delivered while that thread pumps messages. The callback posts
//! `notify_msg` to `notify_hwnd` at most once until `drain` is called. Only one `EventHooks`
//! can exist at a time (the queue is process-wide); a second `install` returns an error.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard};

use windows::Win32::Foundation::{E_FAIL, HWND, LPARAM, WPARAM};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    CHILDID_SELF, EVENT_OBJECT_CLOAKED, EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE,
    EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_SHOW, EVENT_OBJECT_UNCLOAKED,
    EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_MINIMIZEEND, EVENT_SYSTEM_MINIMIZESTART,
    EVENT_SYSTEM_MOVESIZEEND, EVENT_SYSTEM_MOVESIZESTART, OBJID_WINDOW, PostMessageW,
    WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
};
use windows::core::{Error, Result};

use crate::Hwnd;
use crate::win32::to_hwnd;

/// Events kept while the app does not drain; the oldest are dropped first.
const QUEUE_CAP: usize = 4096;

/// Event id ranges to hook, as `(min, max)`.
const HOOKED_RANGES: [(u32, u32); 6] = [
    (EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND),
    (EVENT_SYSTEM_MOVESIZESTART, EVENT_SYSTEM_MOVESIZEEND),
    (EVENT_SYSTEM_MINIMIZESTART, EVENT_SYSTEM_MINIMIZEEND),
    // DESTROY, SHOW and HIDE are consecutive; everything else in the object range is noise.
    (EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE),
    (EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_LOCATIONCHANGE),
    // CLOAKED and UNCLOAKED are consecutive: a window leaves or enters view without a show or hide.
    (EVENT_OBJECT_CLOAKED, EVENT_OBJECT_UNCLOAKED),
];

static QUEUE: Mutex<VecDeque<WinEvent>> = Mutex::new(VecDeque::new());
static PENDING: AtomicBool = AtomicBool::new(false);
static NOTIFY_HWND: AtomicIsize = AtomicIsize::new(0);
static NOTIFY_MSG: AtomicU32 = AtomicU32::new(0);
/// Set while an `EventHooks` owns the process-wide queue.
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// A window event the app cares about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WinEvent {
    /// Window became visible.
    Shown(Hwnd),
    /// Window was hidden.
    Hidden(Hwnd),
    /// Window was destroyed.
    Destroyed(Hwnd),
    /// Window moved or resized (also during an interactive move or size loop).
    Moved(Hwnd),
    /// The user pressed the title bar or a border: a modal move or size loop starts.
    MoveSizeStart(Hwnd),
    /// The modal move or size loop ended.
    MoveSizeEnd(Hwnd),
    /// Window became the foreground window.
    Foreground(Hwnd),
    /// Window was minimized.
    Minimized(Hwnd),
    /// Window was restored from minimized.
    Restored(Hwnd),
    /// The shell or DWM hid the window without a hide event (other virtual desktop, suspended app).
    Cloaked(Hwnd),
    /// The window is visible again after being cloaked.
    Uncloaked(Hwnd),
}

/// Maps a raw WinEvent to a `WinEvent`; only whole-window events qualify.
fn classify(event: u32, id_object: i32, id_child: i32, hwnd: Hwnd) -> Option<WinEvent> {
    if id_object != OBJID_WINDOW.0 || id_child != CHILDID_SELF as i32 || hwnd == 0 {
        return None;
    }
    Some(match event {
        EVENT_OBJECT_SHOW => WinEvent::Shown(hwnd),
        EVENT_OBJECT_HIDE => WinEvent::Hidden(hwnd),
        EVENT_OBJECT_DESTROY => WinEvent::Destroyed(hwnd),
        EVENT_OBJECT_LOCATIONCHANGE => WinEvent::Moved(hwnd),
        EVENT_SYSTEM_MOVESIZESTART => WinEvent::MoveSizeStart(hwnd),
        EVENT_SYSTEM_MOVESIZEEND => WinEvent::MoveSizeEnd(hwnd),
        EVENT_SYSTEM_FOREGROUND => WinEvent::Foreground(hwnd),
        EVENT_SYSTEM_MINIMIZESTART => WinEvent::Minimized(hwnd),
        EVENT_SYSTEM_MINIMIZEEND => WinEvent::Restored(hwnd),
        EVENT_OBJECT_CLOAKED => WinEvent::Cloaked(hwnd),
        EVENT_OBJECT_UNCLOAKED => WinEvent::Uncloaked(hwnd),
        _ => return None,
    })
}

/// The queue stays usable after a panic in another holder: its content is plain data.
fn lock_queue() -> MutexGuard<'static, VecDeque<WinEvent>> {
    QUEUE.lock().unwrap_or_else(|e| e.into_inner())
}

fn enqueue(event: WinEvent) {
    let mut queue = lock_queue();
    if queue.len() >= QUEUE_CAP {
        queue.pop_front();
    }
    queue.push_back(event);
}

/// Posts the notify message unless one is already pending.
fn notify_once() {
    if PENDING.swap(true, Ordering::AcqRel) {
        return;
    }
    let target = NOTIFY_HWND.load(Ordering::Acquire);
    let msg = NOTIFY_MSG.load(Ordering::Acquire);
    // SAFETY: posting to a possibly stale handle fails harmlessly.
    let posted = unsafe { PostMessageW(Some(to_hwnd(target)), msg, WPARAM(0), LPARAM(0)) };
    if posted.is_err() {
        // Re-arm so the next event retries; `has_pending` lets a periodic sweep drain meanwhile.
        PENDING.store(false, Ordering::Release);
    }
}

unsafe extern "system" fn callback(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    id_object: i32,
    id_child: i32,
    _thread: u32,
    _time: u32,
) {
    let Some(ev) = classify(event, id_object, id_child, hwnd.0 as isize) else {
        return;
    };
    enqueue(ev);
    notify_once();
}

/// Drops a `Moved` that repeats the previous event for the same window.
fn merge_repeated_moves(events: Vec<WinEvent>) -> Vec<WinEvent> {
    let mut merged: Vec<WinEvent> = Vec::with_capacity(events.len());
    for ev in events {
        if matches!(ev, WinEvent::Moved(_)) && merged.last() == Some(&ev) {
            continue;
        }
        merged.push(ev);
    }
    merged
}

/// Installed WinEvent hooks; unhooked on drop.
pub(crate) struct EventHooks {
    hooks: Vec<HWINEVENTHOOK>,
    /// True when this value holds the `INSTALLED` guard.
    owns_guard: bool,
}

impl EventHooks {
    /// Installs the hooks, ignoring events from our own process.
    pub(crate) fn install(notify_hwnd: Hwnd, notify_msg: u32) -> Result<EventHooks> {
        Self::install_with(
            notify_hwnd,
            notify_msg,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        )
    }

    fn install_with(notify_hwnd: Hwnd, notify_msg: u32, flags: u32) -> Result<EventHooks> {
        if INSTALLED
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(Error::new(E_FAIL, "EventHooks already installed"));
        }
        // From here on `me` releases the guard when dropped, also on the error paths.
        let mut me = EventHooks {
            hooks: Vec::new(),
            owns_guard: true,
        };
        NOTIFY_HWND.store(notify_hwnd, Ordering::Release);
        NOTIFY_MSG.store(notify_msg, Ordering::Release);
        PENDING.store(false, Ordering::Release);
        lock_queue().clear();
        for (min, max) in HOOKED_RANGES {
            // SAFETY: callback is a valid WINEVENTPROC; out-of-context needs no module handle.
            let hook = unsafe { SetWinEventHook(min, max, None, Some(callback), 0, 0, flags) };
            if hook.is_invalid() {
                return Err(Error::from_thread()); // `me` drops and unhooks the earlier ones
            }
            me.hooks.push(hook);
        }
        Ok(me)
    }

    /// True when events are queued, e.g. after a failed notify post; lets a periodic sweep call `drain`.
    pub(crate) fn has_pending(&self) -> bool {
        !lock_queue().is_empty()
    }

    /// Takes the queued events, merging consecutive `Moved` for the same window, and re-arms notification.
    pub(crate) fn drain(&self) -> Vec<WinEvent> {
        // Clear before taking the queue: an event pushed in between posts a fresh notification.
        PENDING.store(false, Ordering::Release);
        let queued: Vec<WinEvent> = lock_queue().drain(..).collect();
        merge_repeated_moves(queued)
    }
}

impl Drop for EventHooks {
    fn drop(&mut self) {
        for hook in self.hooks.drain(..) {
            // SAFETY: hook came from SetWinEventHook on this thread.
            // A failed unhook cannot be retried or reported from a destructor.
            let _ = unsafe { UnhookWinEvent(hook) };
        }
        if self.owns_guard {
            INSTALLED.store(false, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    use windows::Win32::Foundation::LRESULT;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, EVENT_OBJECT_NAMECHANGE,
        GetSystemMetrics, HWND_MESSAGE, MSG, PM_REMOVE, PeekMessageW, RegisterClassW, SM_CXSCREEN,
        SM_CYSCREEN, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER,
        SetWindowPos, ShowWindow, TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WNDCLASSW,
        WS_OVERLAPPEDWINDOW,
    };
    use windows::core::{PCWSTR, w};

    const NOTIFY: u32 = 0x8000 + 77; // WM_APP + 77

    #[test]
    fn classify_maps_window_events() {
        let c = |e| classify(e, OBJID_WINDOW.0, 0, 5);
        assert_eq!(c(EVENT_OBJECT_SHOW), Some(WinEvent::Shown(5)));
        assert_eq!(c(EVENT_OBJECT_HIDE), Some(WinEvent::Hidden(5)));
        assert_eq!(c(EVENT_OBJECT_DESTROY), Some(WinEvent::Destroyed(5)));
        assert_eq!(c(EVENT_OBJECT_LOCATIONCHANGE), Some(WinEvent::Moved(5)));
        assert_eq!(
            c(EVENT_SYSTEM_MOVESIZESTART),
            Some(WinEvent::MoveSizeStart(5))
        );
        assert_eq!(c(EVENT_SYSTEM_MOVESIZEEND), Some(WinEvent::MoveSizeEnd(5)));
        assert_eq!(c(EVENT_SYSTEM_FOREGROUND), Some(WinEvent::Foreground(5)));
        assert_eq!(c(EVENT_SYSTEM_MINIMIZESTART), Some(WinEvent::Minimized(5)));
        assert_eq!(c(EVENT_SYSTEM_MINIMIZEEND), Some(WinEvent::Restored(5)));
        assert_eq!(c(EVENT_OBJECT_CLOAKED), Some(WinEvent::Cloaked(5)));
        assert_eq!(c(EVENT_OBJECT_UNCLOAKED), Some(WinEvent::Uncloaked(5)));
        assert_eq!(c(0x0001), None);
    }

    #[test]
    fn hooked_ranges_do_not_include_name_changes() {
        // Title changes are never used; hooking them would only wake the app for noise.
        let hooked = |event: u32| {
            HOOKED_RANGES
                .iter()
                .any(|&(lo, hi)| (lo..=hi).contains(&event))
        };
        assert!(!hooked(EVENT_OBJECT_NAMECHANGE));
        assert_eq!(
            classify(EVENT_OBJECT_NAMECHANGE, OBJID_WINDOW.0, 0, 5),
            None
        );
        assert!(
            hooked(EVENT_OBJECT_SHOW) && hooked(EVENT_OBJECT_HIDE) && hooked(EVENT_OBJECT_DESTROY)
        );
        assert!(hooked(EVENT_OBJECT_CLOAKED) && hooked(EVENT_OBJECT_UNCLOAKED));
        assert_eq!(
            (EVENT_OBJECT_CLOAKED, EVENT_OBJECT_UNCLOAKED),
            (0x8017, 0x8018)
        );
    }

    #[test]
    fn classify_rejects_non_window_objects() {
        assert_eq!(classify(EVENT_OBJECT_SHOW, -4, 0, 5), None); // OBJID_CARET
        assert_eq!(classify(EVENT_OBJECT_SHOW, OBJID_WINDOW.0, 1, 5), None);
        assert_eq!(classify(EVENT_OBJECT_SHOW, OBJID_WINDOW.0, 0, 0), None);
    }

    unsafe extern "system" fn test_proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        // SAFETY: forwards unchanged arguments.
        unsafe { DefWindowProcW(h, m, w, l) }
    }

    /// Pumps messages for `ms`; returns how many `NOTIFY` messages were seen.
    fn pump(ms: u64) -> u32 {
        let end = Instant::now() + Duration::from_millis(ms);
        let mut notified = 0;
        while Instant::now() < end {
            let mut msg = MSG::default();
            // SAFETY: standard message pump on this thread.
            unsafe {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    if msg.message == NOTIFY {
                        notified += 1;
                    }
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        notified
    }

    fn make_window(class: PCWSTR, style: u32, parent: Option<HWND>, pos: (i32, i32)) -> HWND {
        // SAFETY: plain window creation on this thread.
        unsafe {
            let inst = GetModuleHandleW(None).unwrap();
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class,
                w!("pinit-test"),
                WINDOW_STYLE(style),
                pos.0,
                pos.1,
                50,
                50,
                parent,
                None,
                Some(inst.into()),
                None,
            )
            .unwrap()
        }
    }

    /// Registers a window class whose procedure is DefWindowProc.
    fn register_test_class() -> PCWSTR {
        let class = w!("PinItEventsTest");
        // SAFETY: registering a class whose procedure is DefWindowProc.
        unsafe {
            let inst = GetModuleHandleW(None).unwrap();
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(test_proc),
                hInstance: inst.into(),
                lpszClassName: class,
                ..Default::default()
            });
        }
        class
    }

    fn screen_size() -> (i32, i32) {
        // SAFETY: plain metric queries.
        unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) }
    }

    /// Phase 1: the public install skips events from our own process.
    fn assert_own_process_events_are_skipped(notify: HWND, target: HWND) {
        let t = target.0 as isize;
        let hooks = EventHooks::install(notify.0 as isize, NOTIFY).unwrap();
        // SAFETY: operations on our own test window.
        unsafe {
            let _ = ShowWindow(target, SW_SHOWNOACTIVATE);
        }
        pump(400);
        let own = hooks.drain();
        drop(hooks);
        assert!(
            !own.iter()
                .any(|e| matches!(e, WinEvent::Shown(x) if *x == t)),
            "{own:?}"
        );
    }

    /// Phase 2: without SKIPOWNPROCESS the same kinds of events arrive.
    fn assert_own_process_events_arrive_without_skip_flag(notify: HWND, target: HWND) {
        let t = target.0 as isize;
        let (sx, sy) = screen_size();
        let hooks =
            EventHooks::install_with(notify.0 as isize, NOTIFY, WINEVENT_OUTOFCONTEXT).unwrap();
        // SAFETY: operations on our own test window.
        unsafe {
            let _ = ShowWindow(target, SW_HIDE);
            let _ = ShowWindow(target, SW_SHOWNOACTIVATE);
            let _ = SetWindowPos(
                target,
                None,
                sx - 70,
                sy - 70,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
        let notified = pump(800);
        // SAFETY: destroying our own test window.
        unsafe { DestroyWindow(target).ok() };
        pump(800);
        let got = hooks.drain();
        drop(hooks);
        // SAFETY: destroying our own notify window.
        unsafe { DestroyWindow(notify).ok() };

        assert!(notified >= 1, "no notify message posted");
        for want in [
            WinEvent::Hidden(t),
            WinEvent::Shown(t),
            WinEvent::Moved(t),
            WinEvent::Destroyed(t),
        ] {
            assert!(got.contains(&want), "missing {want:?} in {got:?}");
        }
    }

    #[test]
    fn hooks_deliver_events_for_own_test_window() {
        let _serial = crate::window_test_guard();
        let class = register_test_class();
        let notify = make_window(class, 0, Some(HWND_MESSAGE), (0, 0));
        let (sx, sy) = screen_size();
        let target = make_window(class, WS_OVERLAPPEDWINDOW.0, None, (sx - 60, sy - 60));

        assert_own_process_events_are_skipped(notify, target);
        assert_own_process_events_arrive_without_skip_flag(notify, target);
    }

    #[test]
    fn drain_coalesces_consecutive_moved() {
        let _serial = crate::window_test_guard();
        let hooks = EventHooks {
            hooks: Vec::new(),
            owns_guard: false,
        };
        {
            let mut q = QUEUE.lock().unwrap();
            q.clear();
            for ev in [
                WinEvent::Moved(1),
                WinEvent::Moved(1),
                WinEvent::Moved(2),
                WinEvent::Moved(1),
                WinEvent::Shown(1),
                WinEvent::Shown(1),
            ] {
                q.push_back(ev);
            }
        }
        assert_eq!(
            hooks.drain(),
            vec![
                WinEvent::Moved(1),
                WinEvent::Moved(2),
                WinEvent::Moved(1),
                WinEvent::Shown(1),
                WinEvent::Shown(1)
            ]
        );
    }

    #[test]
    fn second_install_fails_until_first_is_dropped() {
        let _serial = crate::window_test_guard();
        let first = EventHooks::install(0, NOTIFY).unwrap();
        assert!(EventHooks::install(0, NOTIFY).is_err());
        drop(first);
        drop(EventHooks::install(0, NOTIFY).unwrap());
    }

    #[test]
    fn has_pending_reflects_queue() {
        let _serial = crate::window_test_guard();
        let hooks = EventHooks {
            hooks: Vec::new(),
            owns_guard: false,
        };
        QUEUE.lock().unwrap().clear();
        assert!(!hooks.has_pending());
        QUEUE.lock().unwrap().push_back(WinEvent::Shown(1));
        assert!(hooks.has_pending());
        hooks.drain();
        assert!(!hooks.has_pending());
    }
}
