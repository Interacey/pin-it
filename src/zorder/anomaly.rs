//! A window that is not pinned but lies above the lowest pinned window: an app's own
//! always-on-top window, a Task Manager, a drag proxy. The relative order of the pins alone
//! does not show it, so the order check asks for it separately.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use windows::Win32::UI::WindowsAndMessaging::{
    GA_ROOT, GWL_EXSTYLE, GetAncestor, GetWindowLongW, GetWindowRect, IsIconic, IsWindowVisible,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
};

use super::primitives::is_topmost;
use crate::Hwnd;
use crate::win32::{class_name, is_cloaked, to_hwnd};
use crate::windows_filter::{exe_name, is_shell_class};

/// Class of menus; they are topmost by design and must stay above the pins.
const MENU_CLASS: &str = "#32768";
/// At most one log line per this interval, however often the anomaly comes back.
const LOG_INTERVAL: Duration = Duration::from_secs(5);
/// Windows from the top of the snapshot that the log line lists.
const EXCERPT_LEN: usize = 6;

/// An unpinned window above the lowest pinned one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Intrusion {
    pub(super) intruder: Hwnd,
    pub(super) lowest_pinned: Hwnd,
}

/// The topmost window of `snapshot_top_to_bottom` that `competes` and is neither pinned nor below
/// the lowest pinned window. Only the windows above the lowest pinned one are asked.
pub(super) fn find_intrusion(
    snapshot_top_to_bottom: &[Hwnd],
    pinned: &[Hwnd],
    competes: impl Fn(Hwnd) -> bool,
) -> Option<Intrusion> {
    let lowest = snapshot_top_to_bottom
        .iter()
        .rposition(|h| pinned.contains(h))?;
    let intruder = snapshot_top_to_bottom[..lowest]
        .iter()
        .copied()
        .find(|h| !pinned.contains(h) && competes(*h))?;
    Some(Intrusion {
        intruder,
        lowest_pinned: snapshot_top_to_bottom[lowest],
    })
}

/// What to do about an intrusion found while the pins themselves are in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Verdict {
    pub(super) reassert: bool,
    pub(super) log: bool,
}

/// Remembers the intruder the pins were last re-asserted for. A window that stays above the pins
/// even then (a UI-access window, say) is left alone, so it cannot cause a re-pin on every sweep.
#[derive(Debug, Default)]
pub(super) struct Episode {
    handled: Option<Hwnd>,
    last_log: Option<Instant>,
}

impl Episode {
    pub(super) fn judge(&mut self, intrusion: Option<Intrusion>, now: Instant) -> Verdict {
        let Some(found) = intrusion else {
            self.handled = None;
            return Verdict {
                reassert: false,
                log: false,
            };
        };
        if self.handled == Some(found.intruder) {
            return Verdict {
                reassert: false,
                log: false,
            };
        }
        self.handled = Some(found.intruder);
        let log = self
            .last_log
            .is_none_or(|t| now.duration_since(t) >= LOG_INTERVAL);
        if log {
            self.last_log = Some(now);
        }
        Verdict {
            reassert: true,
            log,
        }
    }
}

thread_local! {
    /// The app runs on one thread; tests run each on their own.
    static EPISODE: RefCell<Episode> = RefCell::new(Episode::default());
}

pub(super) fn judge_now(intrusion: Option<Intrusion>) -> Verdict {
    EPISODE.with(|e| e.borrow_mut().judge(intrusion, Instant::now()))
}

/// A visible, restored, non-cloaked top-level window that is not shell, menu, tool window,
/// no-activate or click-through: one the user sees as being in front of a pin.
pub(super) fn is_competing_window(h: Hwnd) -> bool {
    let w = to_hwnd(h);
    // SAFETY: plain queries on a handle that may be stale; they fail harmlessly.
    let (shown, ex) = unsafe {
        (
            IsWindowVisible(w).as_bool() && !IsIconic(w).as_bool() && GetAncestor(w, GA_ROOT) == w,
            GetWindowLongW(w, GWL_EXSTYLE) as u32,
        )
    };
    let ignored_styles = WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0 | WS_EX_TRANSPARENT.0;
    if !shown || ex & ignored_styles != 0 || !has_area(h) {
        return false;
    }
    let class = class_name(w);
    !is_shell_class(&class) && class != MENU_CLASS && !is_cloaked(w)
}

fn has_area(h: Hwnd) -> bool {
    let mut r = windows::Win32::Foundation::RECT::default();
    // SAFETY: r is a valid out param.
    let queried = unsafe { GetWindowRect(to_hwnd(h), &mut r) }.is_ok();
    queried && r.right > r.left && r.bottom > r.top
}

/// The log line for an intrusion, with the top of the z-order so the next occurrence can be read.
pub(super) fn describe(found: &Intrusion, snapshot_top_to_bottom: &[Hwnd]) -> String {
    let top: Vec<String> = snapshot_top_to_bottom
        .iter()
        .take(EXCERPT_LEN)
        .map(|&h| format!("{h:#x}{}", if is_topmost(h) { "(T)" } else { "" }))
        .collect();
    format!(
        "z-order anomaly: window {:#x} ({}, {}) is above pinned {:#x}; re-asserting pins; top of z-order: {}",
        found.intruder,
        class_name(to_hwnd(found.intruder)),
        exe_name(found.intruder).unwrap_or_else(|| "?".into()),
        found.lowest_pinned,
        top.join(" ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const SNAP: [Hwnd; 5] = [50, 40, 30, 20, 10];

    fn all(_: Hwnd) -> bool {
        true
    }

    fn found(intruder: Hwnd) -> Option<Intrusion> {
        Some(Intrusion {
            intruder,
            lowest_pinned: 20,
        })
    }

    #[test]
    fn unpinned_window_above_the_lowest_pin_is_an_intrusion() {
        assert_eq!(find_intrusion(&SNAP, &[30, 20], all), found(50));
        assert_eq!(find_intrusion(&SNAP, &[40, 20], |h| h == 30), found(30));
    }

    #[test]
    fn pins_alone_and_windows_below_the_lowest_pin_are_no_intrusion() {
        assert_eq!(find_intrusion(&SNAP, &[50, 40], all), None);
        assert_eq!(find_intrusion(&SNAP, &[30, 20], |h| h == 10), None);
        assert_eq!(find_intrusion(&SNAP, &[], all), None);
        assert_eq!(find_intrusion(&SNAP, &[99], all), None);
    }

    #[test]
    fn windows_that_do_not_compete_are_ignored() {
        // 50 is cloaked, a shell window or one of our overlays; 40 is the real intruder.
        assert_eq!(find_intrusion(&SNAP, &[20], |h| h != 50), found(40));
        assert_eq!(find_intrusion(&SNAP, &[20], |_| false), None);
    }

    #[test]
    fn only_windows_above_the_lowest_pin_are_asked() {
        let asked = RefCell::new(Vec::new());
        find_intrusion(&SNAP, &[30], |h| {
            asked.borrow_mut().push(h);
            false
        });
        assert_eq!(*asked.borrow(), vec![50, 40]);
    }

    #[test]
    fn episode_acts_once_per_intruder_and_forgets_when_it_is_gone() {
        let mut e = Episode::default();
        let t = Instant::now();
        assert_eq!(
            e.judge(found(50), t),
            Verdict {
                reassert: true,
                log: true
            }
        );
        // Still above after the re-assert: left alone, no log.
        assert_eq!(
            e.judge(found(50), t),
            Verdict {
                reassert: false,
                log: false
            }
        );
        assert_eq!(
            e.judge(None, t),
            Verdict {
                reassert: false,
                log: false
            }
        );
        assert!(e.judge(found(50), t).reassert, "a new episode");
    }

    #[test]
    fn log_is_rate_limited_but_the_pins_are_still_re_asserted() {
        let mut e = Episode::default();
        let t = Instant::now();
        assert!(e.judge(found(50), t).log);
        assert_eq!(
            e.judge(found(40), t),
            Verdict {
                reassert: true,
                log: false
            }
        );
        assert!(e.judge(found(30), t + LOG_INTERVAL).log);
    }
}
