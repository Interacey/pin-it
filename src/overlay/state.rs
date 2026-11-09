//! Per-window state of one overlay: what to draw, the DIB it is drawn into and the animation timer.

use std::time::Instant;

use windows::Win32::Foundation::{COLORREF, HWND, POINT, SIZE};
use windows::Win32::Graphics::Gdi::{AC_SRC_ALPHA, AC_SRC_OVER, BLENDFUNCTION};
use windows::Win32::UI::WindowsAndMessaging::{
    KillTimer, SetTimer, ULW_ALPHA, UpdateLayeredWindow,
};

use super::ButtonStyle;
use super::fade::Fade;
use super::pulse::BUTTON_AFTERGLOW;
use super::pulse::Pulse;
use super::render::render_button;
use crate::Hwnd;
use crate::anim::ButtonAnim;
use crate::win32::Dib;

pub(super) const TIMER_ID: usize = 1;
/// Animation frame interval, about 60 frames per second.
pub(super) const TIMER_MS: u32 = 16;
/// How far the animation is advanced at once when it starts, so the frame drawn inside the mouse
/// handler already shows motion instead of repeating the old one until the first timer tick.
const FIRST_FRAME_SECS: f32 = 0.008;
/// Longest step one timer tick may advance; a stalled message loop must not make the animation jump.
const MAX_FRAME_DT_SECS: f32 = 0.05;

/// Per-window state, shared with the window procedure through GWLP_USERDATA.
pub(super) struct State {
    pub(super) hwnd: HWND,
    pub(super) target: Hwnd,
    pub(super) notify: Hwnd,
    pub(super) notify_msg: u32,
    pub(super) size: u32,
    pub(super) style: ButtonStyle,
    pub(super) anim: ButtonAnim,
    /// Opacity while Windows animates the target (see `fade`).
    pub(super) fade: Fade,
    /// The window is hidden once the running fade-out has finished.
    hide_when_faded: bool,
    dib: Option<Dib>,
    pub(super) hover: bool,
    pub(super) pressed: bool,
    pub(super) timer_on: bool,
    /// The z-order watch timer runs (see `PinButton::watch_order`).
    pub(super) order_watch: bool,
    /// The 1 ms z-order pulse, running while the mouse is busy or an activation burst lasts.
    pub(super) pulse: Option<Pulse>,
    /// Last time the left mouse button was seen down.
    pub(super) button_seen: Instant,
    /// The pulse runs at least until then.
    pub(super) burst_until: Instant,
    last_tick: Instant,
}

impl State {
    pub(super) fn new(hwnd: HWND, target: Hwnd, notify: Hwnd, notify_msg: u32) -> Self {
        Self {
            hwnd,
            target,
            notify,
            notify_msg,
            size: 0,
            style: ButtonStyle::default(),
            anim: ButtonAnim::new(false),
            fade: Fade::new(),
            hide_when_faded: false,
            dib: None,
            hover: false,
            pressed: false,
            timer_on: false,
            order_watch: false,
            pulse: None,
            button_seen: before_any_press(),
            burst_until: Instant::now(),
            last_tick: Instant::now(),
        }
    }

    /// Re-renders after any change of `anim` or `style`, and starts or stops the timer to match.
    pub(super) fn refresh(&mut self) {
        self.redraw();
        self.update_timer();
    }

    /// Draws the first frame of a transition that was just started on `anim`, inside the input
    /// handler, then lets the timer carry on.
    pub(super) fn begin_transition(&mut self) {
        self.anim.step(FIRST_FRAME_SECS);
        self.refresh();
    }

    pub(super) fn redraw(&mut self) {
        if self.size == 0 || self.hwnd.0.is_null() {
            return;
        }
        if self.dib.as_ref().is_none_or(|d| d.width != self.size) {
            self.dib = Dib::new(self.size, self.size).ok();
        }
        let Some(dib) = self.dib.as_mut() else {
            return;
        };
        dib.write(&render_button(self.size, &self.style, &self.anim));
        present(self.hwnd, dib, self.fade.alpha());
    }

    pub(super) fn update_timer(&mut self) {
        let active = self.anim.is_active() || self.fade.is_active();
        if active && !self.timer_on {
            self.last_tick = Instant::now();
            // SAFETY: timer on our own window.
            self.timer_on = unsafe { SetTimer(Some(self.hwnd), TIMER_ID, TIMER_MS, None) } != 0;
        } else if !active && self.timer_on {
            // SAFETY: as above.
            // A failed kill leaves a timer on a window that is being torn down; nothing to retry.
            let _ = unsafe { KillTimer(Some(self.hwnd), TIMER_ID) };
            self.timer_on = false;
        }
    }

    /// Advances the animation by `dt` seconds; returns true while it still runs.
    pub(super) fn tick(&mut self, dt: f32) -> bool {
        self.anim.step(dt);
        self.fade.step(dt);
        self.refresh();
        self.anim.is_active() || self.fade.is_active()
    }

    /// Starts the fade-out that ends with the window hidden.
    pub(super) fn fade_out_then_hide(&mut self) {
        self.fade.fade_out();
        self.hide_when_faded = true;
        self.refresh();
    }

    /// Invisible now, back after the window animation.
    pub(super) fn hold_back(&mut self) {
        self.fade.hold_then_fade_in();
        self.hide_when_faded = false;
        self.refresh();
    }

    /// Called when the window is shown again: an overlay that faded out becomes visible.
    pub(super) fn reveal(&mut self) {
        self.hide_when_faded = false;
        self.fade.ensure_visible();
        self.refresh();
    }

    /// True while the overlay is fully transparent: it must not catch clicks.
    pub(super) fn is_invisible(&self) -> bool {
        self.fade.is_hidden()
    }

    /// Forgets a pressed or hovered state, as when the window is hidden.
    pub(super) fn forget_mouse(&mut self) {
        self.pressed = false;
        self.set_hover(false);
    }

    /// True once a fade-out is over and the window is to be hidden; reported once.
    pub(super) fn take_fade_hide(&mut self) -> bool {
        let due = self.hide_when_faded && self.fade.has_faded_out();
        if due {
            self.hide_when_faded = false;
        }
        due
    }

    /// Timer message: steps by the real time since the last tick.
    pub(super) fn on_timer(&mut self) {
        let now = Instant::now();
        let dt = now
            .duration_since(self.last_tick)
            .as_secs_f32()
            .min(MAX_FRAME_DT_SECS);
        self.last_tick = now;
        self.tick(dt);
    }

    pub(super) fn apply_style(&mut self, style: ButtonStyle) {
        self.style = style;
        self.anim.set_reduced(!style.animate);
        self.anim.set_pinned(style.pinned);
        self.refresh();
    }
}

/// A moment far enough back that no mouse press counts as recent.
fn before_any_press() -> Instant {
    let now = Instant::now();
    now.checked_sub(BUTTON_AFTERGLOW * 2).unwrap_or(now)
}

/// Hands the DIB content to the layered window.
fn present(hwnd: HWND, dib: &Dib, alpha: u8) {
    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: alpha,
        AlphaFormat: AC_SRC_ALPHA as u8,
    };
    let size = SIZE {
        cx: dib.width as i32,
        cy: dib.height as i32,
    };
    // SAFETY: `hwnd` is our live window and `dib.dc` a valid memory DC.
    // A failed update only leaves the previous frame on screen; the next redraw replaces it.
    let _ = unsafe {
        UpdateLayeredWindow(
            hwnd,
            None,
            None,
            Some(&size),
            Some(dib.dc),
            Some(&POINT { x: 0, y: 0 }),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        )
    };
}
