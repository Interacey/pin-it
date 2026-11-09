//! The window registry: which windows have a button, and keeping each button's slot, style and
//! visibility up to date.

use std::time::Instant;

use super::entry::{Entry, HideReason, MINIMIZED_CAUSE, STYLE_OR_SIZE_CAUSE, Slot, Visibility};
use super::host::WM_PIN_CLICK;
use super::logging::warn;
use super::state::App;
use super::winutil::{current_foreground, is_minimized, is_visible, root_window, window_alive};
use crate::Hwnd;
use crate::color::ScanLookup;
use crate::overlay::{ButtonStyle, PinButton};
use crate::placement::{self, Placement, PlacementInput, ScanState};
use crate::win32::{is_cloaked, to_hwnd};
use crate::windows_filter::{default_placement, exe_name, is_elevated_target, is_pinnable};

impl App {
    pub(super) fn eligible(&self, h: Hwnd) -> bool {
        match &self.only {
            Some(list) => list.contains(&h) && window_alive(h) && is_visible(h) && !is_minimized(h),
            None => is_pinnable(h),
        }
    }

    pub(super) fn add_window(&mut self, h: Hwnd) {
        if self.entries.contains_key(&h) || !self.eligible(h) {
            return;
        }
        let exe = exe_name(h);
        if self.is_never_offered(h, exe.as_deref()) {
            return;
        }
        match PinButton::new(h, self.hwnd, WM_PIN_CLICK) {
            Ok(button) => {
                self.entries.insert(h, Entry::new(button, exe));
                self.refresh(h);
                self.watch_foreground_order();
            }
            Err(e) => warn(&format!("cannot create button for {h:#x}: {e}")),
        }
    }

    /// Excluded apps never get a button; neither do elevated windows (outside of test runs).
    fn is_never_offered(&self, h: Hwnd, exe: Option<&str>) -> bool {
        exe.is_some_and(|e| self.settings.is_excluded(e))
            || (self.only.is_none() && is_elevated_target(h))
    }

    pub(super) fn show_or_add(&mut self, h: Hwnd) {
        if self.entries.contains_key(&h) {
            self.refresh(h);
        } else {
            self.add_window(h);
        }
    }

    /// Where the button of `h` goes now: the default slot, scaled by the user's setting and kept
    /// inside the caption, moved to free title-bar space by the latest scan. `None` when the window
    /// has no slot at all (minimized, tiny).
    fn plan_placement(&mut self, h: Hwnd, created: Instant) -> Option<Placement> {
        let default = default_placement(h)?;
        let (left, top, right, bottom) = default.window_rect;
        let input = PlacementInput {
            slot: placement::apply_user_scale(
                default.slot,
                self.settings.button_scale_percent,
                default.dpi,
            ),
            estimated: default.estimated,
            dpi: default.dpi,
            window_origin: (left, top),
            window_size: (right - left, bottom - top),
            caption_hint_px: placement::caption_hint_px(default.slot.size, default.dpi),
            group_right: default.group_right,
            waited: created.elapsed(),
        };
        let now = Instant::now();
        let lookup = self.colors.scan_for(h, now);
        self.rescan_if_resized(h, &lookup, input.window_size, now);
        let placed = placement::decide(&input, scan_state(&lookup));
        if placed == Placement::Wait {
            self.colors.refresh_awaited(h, now);
        }
        Some(placed)
    }

    /// A scan taken at another window size no longer fits; ask for a new one right away.
    fn rescan_if_resized(
        &mut self,
        h: Hwnd,
        lookup: &ScanLookup,
        window_size: (i32, i32),
        now: Instant,
    ) {
        if let ScanLookup::Ready(scan) = lookup
            && scan.window_size != window_size
        {
            self.colors.refresh_resized(h, now);
        }
    }

    /// Updates slot, style and visibility of one button; skips work that would change nothing.
    /// The button is hidden whenever the window is not eligible (hidden, minimized, cloaked, no
    /// caption) or the title bar has no free space for it.
    pub(super) fn refresh(&mut self, h: Hwnd) {
        let Some((exe, created)) = self.entries.get(&h).map(|e| (e.exe.clone(), e.created)) else {
            return;
        };
        let planned = self.plan_placement(h, created);
        let slot = slot_of(planned.unwrap_or(Placement::Hide));
        let wanted = self.visibility_of(h, planned);
        let Some(entry) = self.entries.get_mut(&h) else {
            return;
        };
        let visibility = entry.settle(wanted, Instant::now());
        let style = (visibility == Visibility::Show).then(|| self.style_for(h, exe.as_deref()));
        let Some(entry) = self.entries.get_mut(&h) else {
            return;
        };
        if let Some(planned) = planned {
            entry.note_space(h, planned);
        }
        entry.apply_slot(slot);
        entry.apply_style(style);
        entry.apply_visibility(h, visibility);
    }

    /// Whether the button of `h` is visible, and if not, why.
    fn visibility_of(&self, h: Hwnd, planned: Option<Placement>) -> Visibility {
        if !self.settings.enabled {
            return Visibility::Hide(HideReason::Disabled);
        }
        if !self.eligible(h) {
            return Visibility::Hide(HideReason::NotEligible(ineligible_cause(h)));
        }
        match planned {
            Some(Placement::Show(_)) => Visibility::Show,
            Some(Placement::Hide) => Visibility::Hide(HideReason::NoFreeRun),
            Some(Placement::Wait) => Visibility::Hide(HideReason::AwaitingScan),
            Some(Placement::NoScan) => Visibility::Hide(HideReason::NoScan),
            None => Visibility::Hide(HideReason::NoSlot),
        }
    }

    /// Forgets a destroyed window and logs that its button is gone.
    pub(super) fn drop_destroyed(&mut self, h: Hwnd) {
        if let Some(entry) = self.entries.get_mut(&h) {
            entry.hide(h, HideReason::Destroyed);
        }
        self.drop_entry(h);
    }

    /// Removes a window that stopped being eligible and logs why its button is gone.
    pub(super) fn remove_ineligible(&mut self, h: Hwnd) {
        if let Some(entry) = self.entries.get_mut(&h) {
            entry.hide(h, HideReason::NotEligible(ineligible_cause(h)));
        }
        self.remove_window(h);
    }

    /// Forces a re-render of the button of `h` in `refresh`. A hidden button keeps the look it was last shown
    /// with: `Entry::hold_back_for_animation` reads from it whether fades are wanted, and the next show renders
    /// the look anyway.
    pub(super) fn refresh_style(&mut self, h: Hwnd) {
        if let Some(entry) = self.entries.get_mut(&h).filter(|e| e.shown) {
            entry.style = None;
        }
        self.refresh(h);
    }

    /// Color of the other caption buttons' symbols from the latest scan of `h`.
    fn caption_ink(&mut self, h: Hwnd) -> Option<[u8; 3]> {
        match self.colors.scan_for(h, Instant::now()) {
            ScanLookup::Ready(scan) => scan.caption_ink,
            ScanLookup::Pending | ScanLookup::Unavailable => None,
        }
    }

    pub(super) fn window_color(&mut self, h: Hwnd) -> [u8; 3] {
        self.colors.get(h, Instant::now())
    }

    /// The look of the button: the head takes the window's accent (the strongest color of its title bar, else
    /// the bar color) unless the settings override it; the symbols take the sampled caption ink.
    fn style_for(&mut self, h: Hwnd, exe: Option<&str>) -> ButtonStyle {
        let window_color = self.window_color(h);
        let window_accent = self.colors.accent(h, Instant::now());
        ButtonStyle {
            accent: self.settings.accent_for(exe, window_accent, self.system),
            title_bar: window_color,
            pinned: self.pins.is_pinned(&h),
            ink: self.caption_ink(h),
            animate: self.settings.animate,
        }
    }

    pub(super) fn hide_button(&mut self, h: Hwnd) {
        if let Some(entry) = self.entries.get_mut(&h) {
            entry.hide(h, HideReason::NotEligible(MINIMIZED_CAUSE));
        }
    }

    /// Puts every shown overlay back above its target; cheap when nothing moved.
    pub(super) fn restack_all(&mut self) {
        for entry in self.entries.values_mut().filter(|e| e.shown) {
            entry.button.restack();
        }
    }

    /// Restacks the shown overlays once more shortly after an activation.
    pub(super) fn restack_all_soon(&mut self) {
        for entry in self.entries.values_mut().filter(|e| e.shown) {
            entry.button.restack_soon();
        }
    }

    /// The overlay of the foreground window checks its z-order every few milliseconds, all others rest.
    pub(super) fn watch_foreground_order(&mut self) {
        let foreground = current_foreground().map(root_window);
        for (&h, entry) in self.entries.iter_mut() {
            entry.button.watch_order(Some(h) == foreground);
        }
    }

    pub(super) fn entry_handles(&self) -> Vec<Hwnd> {
        self.entries.keys().copied().collect()
    }

    /// Exe name of the last pinnable foreground window that still exists.
    pub(super) fn last_foreground_exe(&self) -> Option<String> {
        let h = self.last_fg.filter(|&h| window_alive(h))?;
        self.entries
            .get(&h)
            .and_then(|e| e.exe.clone())
            .or_else(|| exe_name(h))
    }
}

/// The first reason, in the order the checks run, why `h` is not eligible for a button.
fn ineligible_cause(h: Hwnd) -> &'static str {
    if !window_alive(h) {
        "window gone"
    } else if is_minimized(h) {
        MINIMIZED_CAUSE
    } else if !is_visible(h) {
        "not visible"
    } else if is_cloaked(to_hwnd(h)) {
        "cloaked"
    } else {
        STYLE_OR_SIZE_CAUSE
    }
}

fn slot_of(placement: Placement) -> Option<Slot> {
    match placement {
        Placement::Show(s) => Some((s.x, s.y, s.size as u32)),
        Placement::Hide | Placement::Wait | Placement::NoScan => None,
    }
}

fn scan_state(lookup: &ScanLookup) -> ScanState<'_> {
    match lookup {
        ScanLookup::Pending => ScanState::Pending,
        ScanLookup::Unavailable => ScanState::Unavailable,
        ScanLookup::Ready(scan) => ScanState::Ready(scan),
    }
}
