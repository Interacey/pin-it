//! Reactions to window events, the foreground window, the hotkey and the periodic sweep.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use super::state::App;
use super::winutil::{
    current_foreground, hotkey_pinnable, is_minimized, root_window, window_alive,
};
use windows::Win32::UI::WindowsAndMessaging::IsZoomed;

use crate::Hwnd;
use crate::color::system_accent;
use crate::events::{EventHooks, WinEvent};
use crate::win32::to_hwnd;
use crate::windows_filter::{enumerate_pinnable, exe_name, is_elevated_target};

/// The title bar of the foreground window is sampled again at least this often (a theme, a tab or a hover
/// state may have changed); the other windows follow the cache's 30 s lifetime.
const FOREGROUND_RESCAN_INTERVAL: Duration = Duration::from_secs(5);

/// Shortest time between two order checks caused by move events.
const MOVE_ORDER_CHECK_INTERVAL: Duration = Duration::from_millis(100);

thread_local! {
    /// The app runs on one thread; tests run each on their own.
    static MOVE_ORDER_CHECK: RefCell<Throttle> = RefCell::new(Throttle::new(MOVE_ORDER_CHECK_INTERVAL));
}

/// Lets one call per interval through.
struct Throttle {
    interval: Duration,
    last: Option<Instant>,
}

impl Throttle {
    fn new(interval: Duration) -> Throttle {
        Throttle {
            interval,
            last: None,
        }
    }

    fn allow(&mut self, now: Instant) -> bool {
        let due = self
            .last
            .is_none_or(|t| now.duration_since(t) >= self.interval);
        if due {
            self.last = Some(now);
        }
        due
    }
}

impl App {
    pub(super) fn handle_event(&mut self, event: WinEvent) {
        match event {
            WinEvent::Shown(h) => self.on_shown(h),
            WinEvent::Restored(h) => {
                self.hold_back_for_animation(h);
                self.on_shown(h);
            }
            WinEvent::Hidden(h) => self.on_hidden(h),
            WinEvent::Destroyed(h) => self.drop_destroyed(h),
            WinEvent::Moved(h) => self.on_moved(h),
            WinEvent::MoveSizeStart(h) => self.on_move_size_start(h),
            WinEvent::MoveSizeEnd(h) => self.on_move_size_end(h),
            WinEvent::Uncloaked(h) => self.on_uncloaked(h),
            WinEvent::Cloaked(h) => {
                self.refresh(h);
                self.restack_all_soon();
            }
            WinEvent::Minimized(h) => self.hide_button(h),
            WinEvent::Foreground(h) => self.on_foreground(h),
        }
    }

    fn on_shown(&mut self, h: Hwnd) {
        self.show_or_add(h);
        self.restack_all();
    }

    /// Windows animates a bitmap of the target for a moment after it is restored or maximized, while the real
    /// window is already at its final place: the button waits invisible and fades in afterwards.
    pub(super) fn hold_back_for_animation(&mut self, h: Hwnd) {
        if let Some(entry) = self.entries.get_mut(&h) {
            entry.hold_back_for_animation();
        }
    }

    /// Starts the hold when the maximized state of `h` changed.
    fn note_zoom_change(&mut self, h: Hwnd) {
        // SAFETY: plain query.
        let zoomed = unsafe { IsZoomed(to_hwnd(h)) }.as_bool();
        if self
            .entries
            .get_mut(&h)
            .is_some_and(|e| e.note_zoomed(zoomed))
        {
            self.hold_back_for_animation(h);
        }
    }

    /// A window that moves (a drag included) may be dragged over a pin, so the pin order is checked
    /// now and then. Without pins, or for a pinned window, nothing can have gone wrong.
    fn on_moved(&mut self, h: Hwnd) {
        self.note_zoom_change(h);
        self.refresh(h);
        self.restack_all();
        let may_cover_a_pin = !self.pins.is_empty() && !self.pins.is_pinned(&h);
        if may_cover_a_pin && MOVE_ORDER_CHECK.with(|t| t.borrow_mut().allow(Instant::now())) {
            self.ensure_order();
        }
    }

    /// A modal move or size loop starts: the window is busy and only its position changes, so no scans of its
    /// own are taken and the button follows with the slot it has.
    fn on_move_size_start(&mut self, h: Hwnd) {
        if self.entries.contains_key(&h) {
            self.colors.suspend(h, Instant::now());
        }
        self.on_moved(h);
    }

    /// The loop ended: the new size gets its scan right away.
    fn on_move_size_end(&mut self, h: Hwnd) {
        self.colors.resume(h, Instant::now());
        self.on_moved(h);
    }

    fn on_uncloaked(&mut self, h: Hwnd) {
        self.show_or_add(h);
        self.ensure_order();
        self.restack_all_soon();
    }

    /// A pinned window keeps its pin while hidden; an unpinned one is forgotten.
    fn on_hidden(&mut self, h: Hwnd) {
        if self.pins.is_pinned(&h) {
            self.refresh(h);
        } else if self.entries.contains_key(&h) {
            self.remove_ineligible(h);
        }
    }

    /// The activation may have raised the window over its button: put the buttons back at once, and again
    /// after 30 ms and 100 ms.
    fn on_foreground(&mut self, h: Hwnd) {
        let previous = self.last_fg;
        self.note_foreground(h);
        self.restack_all();
        // Active and inactive title bars differ in color and ink, so both windows are sampled again.
        let now = Instant::now();
        for window in previous.into_iter().chain([root_window(h)]) {
            self.colors.refresh_after_focus(window, now);
        }
        self.watch_foreground_order();
        self.ensure_order();
        self.restack_all_soon();
    }

    /// The system accent or theme changed: every cached title-bar sample may be out of date. The host calls
    /// this on WM_SETTINGCHANGE (ImmersiveColorSet), WM_DWMCOLORIZATIONCOLORCHANGED and WM_THEMECHANGED; the sweep notices an
    /// accent change by itself.
    pub(super) fn on_system_colors_changed(&mut self) {
        self.system = system_accent();
        self.colors.invalidate_all(Instant::now());
    }

    pub(super) fn handle_hook_events(&mut self) {
        let events = self.hooks.as_ref().map(EventHooks::drain);
        for event in events.into_iter().flatten() {
            self.handle_event(event);
        }
    }

    /// A background sample finished: take the results, then place and re-render every button (a
    /// new scan can move or hide it).
    pub(super) fn on_color_ready(&mut self) {
        self.colors.poll(Instant::now());
        for h in self.entry_handles() {
            self.refresh_style(h);
        }
    }

    /// Remembers `h` as the window the tray's "Exclude current app" and "Pin / unpin current
    /// window" refer to, if it is a real app window (also one that gets no button).
    fn note_foreground(&mut self, h: Hwnd) {
        let h = root_window(h);
        if h != self.hwnd
            && (self.entries.contains_key(&h) || self.eligible(h) || self.pin_target_ok(h))
        {
            self.last_fg = Some(h);
        }
    }

    /// Housekeeping every two seconds: forget dead windows, pick up new ones, re-apply the order,
    /// retry what failed before (pending releases, a missing tray icon).
    pub(super) fn sweep(&mut self) {
        if system_accent() != self.system {
            self.on_system_colors_changed();
        }
        if self.hooks.as_ref().is_some_and(EventHooks::has_pending) {
            self.handle_hook_events();
        }
        if let Some(fg) = current_foreground() {
            self.note_foreground(fg);
            self.colors.refresh_if_older(
                root_window(fg),
                Instant::now(),
                FOREGROUND_RESCAN_INTERVAL,
            );
        }
        self.watch_foreground_order();
        for h in self.entry_handles() {
            self.sweep_entry(h);
        }
        self.drop_dead_pins();
        self.add_candidates();
        self.ensure_order();
        self.retry_pending_releases();
        self.retry_tray_icon();
    }

    fn sweep_entry(&mut self, h: Hwnd) {
        if !window_alive(h) {
            self.drop_destroyed(h);
        } else if !is_minimized(h) && !self.eligible(h) && !self.pins.is_pinned(&h) {
            self.forget_if_hidden_for_good(h);
        } else {
            self.refresh(h);
        }
    }

    /// An ineligible window is forgotten once its button is gone; the refresh hides the button only after the
    /// reason has held for a moment, so a flicker of the window's styles does not destroy the button.
    fn forget_if_hidden_for_good(&mut self, h: Hwnd) {
        self.refresh(h);
        if self.entries.get(&h).is_some_and(|e| !e.shown) {
            self.remove_ineligible(h);
        }
    }

    /// Pins of windows that never got a button (hotkey) can only be checked here.
    fn drop_dead_pins(&mut self) {
        for h in self
            .pinned_order()
            .into_iter()
            .filter(|&h| !window_alive(h))
        {
            self.drop_entry(h);
        }
    }

    fn add_candidates(&mut self) {
        let candidates = match &self.only {
            Some(list) => list.clone(),
            None => enumerate_pinnable(),
        };
        for h in candidates {
            self.add_window(h);
        }
    }

    /// Whether the hotkey or the tray command may pin or unpin `h`: windows we know, and windows
    /// the app would pin even if they get no button; excluded apps and elevated processes never.
    pub(super) fn pin_target_ok(&self, h: Hwnd) -> bool {
        if !window_alive(h) {
            return false;
        }
        if h == self.hwnd {
            return false;
        }
        if self.entries.contains_key(&h) || self.pins.is_pinned(&h) {
            return true;
        }
        let pinnable = match &self.only {
            Some(_) => self.eligible(h),
            None => hotkey_pinnable(h) && !is_elevated_target(h),
        };
        pinnable && !exe_name(h).is_some_and(|e| self.settings.is_excluded(&e))
    }

    pub(super) fn on_hotkey(&mut self) {
        if !self.settings.enabled || self.hotkey.active.is_none() {
            return;
        }
        let Some(fg) = current_foreground() else {
            return;
        };
        if self.pin_target_ok(fg) {
            self.toggle(fg);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn throttle_lets_one_call_per_interval_through() {
        let mut t = Throttle::new(MOVE_ORDER_CHECK_INTERVAL);
        let start = Instant::now();
        assert!(t.allow(start));
        assert!(!t.allow(start + Duration::from_millis(99)));
        assert!(t.allow(start + MOVE_ORDER_CHECK_INTERVAL));
        assert!(!t.allow(start + MOVE_ORDER_CHECK_INTERVAL + Duration::from_millis(1)));
    }
}
