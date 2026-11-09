//! One window that has a pin button, and the cached state of that button.

use std::time::{Duration, Instant};

use super::logging::info;
use crate::Hwnd;
use crate::overlay::{ButtonStyle, PinButton};
use crate::placement::Placement;

/// A transient reason to hide (a flicker of the window's styles or size) must hold this long before the
/// button goes; a button that vanishes and returns looks broken.
pub(super) const HIDE_DEBOUNCE: Duration = Duration::from_millis(150);

/// The text of `ineligible_cause` for a window that is neither gone, minimized, hidden nor cloaked: its
/// styles or size changed, which can be a flicker.
pub(super) const STYLE_OR_SIZE_CAUSE: &str = "no title bar, tool window or no area";

/// The text of `ineligible_cause` for a minimized window.
pub(super) const MINIMIZED_CAUSE: &str = "minimized";

/// The text of `HideReason::NoScan`, also logged for a button that was never shown.
const NO_SCAN_TEXT: &str = "no title bar scan";

/// Button position and size: x, y, size in pixels.
pub(super) type Slot = (i32, i32, u32);

pub(super) struct Entry {
    pub(super) button: PinButton,
    pub(super) exe: Option<String>,
    pub(super) slot: Option<Slot>,
    pub(super) style: Option<ButtonStyle>,
    pub(super) shown: bool,
    /// Whether the target was maximized at the last check; a change starts a Windows animation.
    zoomed: bool,
    /// When the button was created; custom title bars wait for their first scan from then on.
    pub(super) created: Instant,
    /// The "no free space" line was logged for this window.
    no_space_logged: bool,
    /// The "no title bar scan" line was logged for this window.
    no_scan_logged: bool,
    /// Since when a transient reason to hide has held; `None` while the button has no such reason.
    hide_pending_since: Option<Instant>,
}

impl Entry {
    pub(super) fn new(button: PinButton, exe: Option<String>) -> Entry {
        Entry {
            button,
            exe,
            slot: None,
            style: None,
            shown: false,
            zoomed: false,
            created: Instant::now(),
            no_space_logged: false,
            no_scan_logged: false,
            hide_pending_since: None,
        }
    }

    /// The visibility to apply: a shown button stays through a transient reason to hide until it has held for
    /// `HIDE_DEBOUNCE`, and through a pending first scan; every other reason is applied at once.
    pub(super) fn settle(&mut self, wanted: Visibility, now: Instant) -> Visibility {
        let Visibility::Hide(reason) = wanted else {
            self.hide_pending_since = None;
            return wanted;
        };
        if !self.shown || reason.is_immediate() {
            self.hide_pending_since = None;
            return wanted;
        }
        if reason == HideReason::AwaitingScan {
            return Visibility::Show;
        }
        let since = *self.hide_pending_since.get_or_insert(now);
        if now.saturating_duration_since(since) >= HIDE_DEBOUNCE {
            wanted
        } else {
            Visibility::Show
        }
    }

    /// Whether fades are wanted: the look applied last says so.
    fn animates(&self) -> bool {
        self.style.is_some_and(|s| s.animate)
    }

    /// Records the maximized state of the target; true when it changed (Windows animates the change).
    pub(super) fn note_zoomed(&mut self, zoomed: bool) -> bool {
        std::mem::replace(&mut self.zoomed, zoomed) != zoomed
    }

    /// The target changes state with a Windows animation: the button waits invisible until it is over.
    pub(super) fn hold_back_for_animation(&mut self) {
        self.button.hold_back(self.animates());
    }

    /// Logs once per episode that the title bar has no room or no scan; re-arms when a slot exists again.
    /// The window stays pinned while its button is hidden.
    pub(super) fn note_space(&mut self, target: Hwnd, placement: Placement) {
        match placement {
            Placement::Hide if !self.no_space_logged => {
                self.no_space_logged = true;
                info(&format!(
                    "no free space in the title bar: button hidden; use Tray > Pin current window ({target:#x})"
                ));
            }
            Placement::NoScan if !self.no_scan_logged => {
                self.no_scan_logged = true;
                info(&format!("button hidden: {NO_SCAN_TEXT} ({target:#x})"));
            }
            Placement::Show(_) => {
                self.no_space_logged = false;
                self.no_scan_logged = false;
            }
            Placement::Hide | Placement::Wait | Placement::NoScan => {}
        }
    }

    pub(super) fn apply_slot(&mut self, slot: Option<Slot>) {
        if let Some((x, y, size)) = slot
            && self.slot != slot
        {
            self.button.set_slot(x, y, size);
            self.slot = slot;
        }
    }

    pub(super) fn apply_style(&mut self, style: Option<ButtonStyle>) {
        if let Some(style) = style
            && self.style != Some(style)
        {
            self.button.set_style(style);
            self.style = Some(style);
        }
    }

    /// Shows or hides the button and logs each change of visibility once, with the reason.
    pub(super) fn apply_visibility(&mut self, target: Hwnd, visibility: Visibility) {
        match visibility {
            Visibility::Show if !self.shown => {
                self.button.show();
                self.button.restack();
                self.shown = true;
                if let Some((x, y, size)) = self.slot {
                    info(&format!(
                        "button shown at {x},{y} size {size} ({target:#x})"
                    ));
                }
            }
            Visibility::Hide(reason) if self.shown => self.hide(target, reason),
            Visibility::Show | Visibility::Hide(_) => {}
        }
    }

    pub(super) fn hide(&mut self, target: Hwnd, reason: HideReason) {
        if self.shown {
            info(&format!("button hidden: {} ({target:#x})", reason.text()));
        }
        if self.shown && reason.is_minimize() {
            self.button.fade_out_then_hide(self.animates());
        } else {
            self.button.hide();
        }
        self.shown = false;
    }
}

/// Whether the button of a window is to be visible after a refresh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Visibility {
    Show,
    Hide(HideReason),
}

/// Why a button is hidden; the text goes to the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HideReason {
    /// Pin It is switched off in the settings.
    Disabled,
    /// The window cannot get a button right now; the text says why (cloaked, minimized, ...).
    NotEligible(&'static str),
    /// No caption slot at all: minimized or too small.
    NoSlot,
    /// The title bar scan found no free run wide enough.
    NoFreeRun,
    /// A custom title bar waits for a scan that shows its caption buttons.
    AwaitingScan,
    /// A custom title bar got no scan within the wait.
    NoScan,
    /// The window was destroyed.
    Destroyed,
}

impl HideReason {
    /// Whether the reason is final the moment it shows up (minimized, hidden, cloaked, destroyed, switched
    /// off, no free run in a scan of the current size). The others can be a flicker.
    fn is_immediate(self) -> bool {
        match self {
            Self::Disabled | Self::NoFreeRun | Self::NoScan | Self::Destroyed => true,
            Self::NotEligible(why) => why != STYLE_OR_SIZE_CAUSE,
            Self::NoSlot | Self::AwaitingScan => false,
        }
    }

    /// The target was minimized, which Windows animates.
    fn is_minimize(self) -> bool {
        self == Self::NotEligible(MINIMIZED_CAUSE)
    }

    fn text(self) -> String {
        match self {
            Self::Disabled => "disabled in settings".into(),
            Self::NotEligible(why) => format!("not eligible ({why})"),
            Self::NoSlot => "no caption slot (minimized or too small)".into(),
            Self::NoFreeRun => "no free run in the title bar".into(),
            Self::AwaitingScan => "waiting for the first title bar scan".into(),
            Self::NoScan => NO_SCAN_TEXT.into(),
            Self::Destroyed => "window destroyed".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown_entry() -> Entry {
        let button = PinButton::new(1, 0, 0).unwrap();
        Entry {
            shown: true,
            ..Entry::new(button, None)
        }
    }

    const SOON: Duration = Duration::from_millis(50);
    const LATE: Duration = Duration::from_millis(200);
    const FLICKER: Visibility = Visibility::Hide(HideReason::NotEligible(STYLE_OR_SIZE_CAUSE));

    #[test]
    fn a_flicker_of_a_transient_reason_keeps_the_button() {
        let _g = crate::window_test_guard();
        let mut e = shown_entry();
        let t0 = Instant::now();
        assert_eq!(e.settle(FLICKER, t0), Visibility::Show);
        assert_eq!(e.settle(FLICKER, t0 + SOON), Visibility::Show);
        assert_eq!(e.settle(Visibility::Show, t0 + SOON), Visibility::Show);
        // The reason is gone, so the clock starts again.
        assert_eq!(e.settle(FLICKER, t0 + LATE), Visibility::Show);
    }

    #[test]
    fn a_transient_reason_that_holds_hides_the_button() {
        let _g = crate::window_test_guard();
        let mut e = shown_entry();
        let t0 = Instant::now();
        e.settle(FLICKER, t0);
        assert_eq!(e.settle(FLICKER, t0 + LATE), FLICKER);
    }

    #[test]
    fn real_reasons_hide_at_once() {
        let _g = crate::window_test_guard();
        let now = Instant::now();
        for reason in [
            HideReason::Disabled,
            HideReason::NoFreeRun,
            HideReason::NoScan,
            HideReason::Destroyed,
            HideReason::NotEligible("minimized"),
            HideReason::NotEligible("not visible"),
            HideReason::NotEligible("cloaked"),
        ] {
            let mut e = shown_entry();
            assert_eq!(
                e.settle(Visibility::Hide(reason), now),
                Visibility::Hide(reason)
            );
        }
    }

    #[test]
    fn a_pending_first_scan_never_hides_a_shown_button() {
        let _g = crate::window_test_guard();
        let mut e = shown_entry();
        let t0 = Instant::now();
        let wait = Visibility::Hide(HideReason::AwaitingScan);
        assert_eq!(e.settle(wait, t0), Visibility::Show);
        assert_eq!(e.settle(wait, t0 + LATE), Visibility::Show);
    }

    #[test]
    fn a_hidden_button_stays_hidden_without_delay() {
        let _g = crate::window_test_guard();
        let mut e = shown_entry();
        e.shown = false;
        assert_eq!(e.settle(FLICKER, Instant::now()), FLICKER);
    }
}
