//! The Win32 side of the dial: one popup window with its own window procedure on the main thread.
//!
//! The window never touches the `App`. It only posts `message` with the chosen percent to the
//! notify window, so the change runs through the normal command path once the current message
//! is done (see the re-entrancy notes in `app::host`). The state is in `ui`.

mod ui;

#[cfg(test)]
pub(crate) mod probe;

#[cfg(test)]
mod review;

use std::cell::Cell;
use std::time::Instant;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, VK_DOWN, VK_END, VK_ESCAPE, VK_HOME, VK_LEFT, VK_RETURN, VK_RIGHT,
    VK_SPACE, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, KillTimer, PostMessageW, SetTimer, WM_ACTIVATE, WM_CAPTURECHANGED,
    WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL,
    WM_NCDESTROY, WM_PAINT, WM_TIMER,
};
use windows::core::w;

use self::ui::{UI, Ui, WINDOW, with_ui};
use super::model::Step;
use super::state::{Key, Reaction};
use crate::Hwnd;
use crate::popup::placement::{bottom_right, monitor_under_cursor};
use crate::popup::window::{
    PopupClass, close as close_window, create, destroy, invalidate, live, point_of, raise,
};
use crate::win32::to_hwnd;

const POPUP: PopupClass = PopupClass {
    name: w!("PinItSizeDial"),
    title: w!("Button size"),
    proc: Some(dial_proc),
};
const ANIMATION_TIMER: usize = 1;
const FRAME_MS: u32 = 16;
/// Longest frame time fed to the spring, so a stalled thread does not make it jump.
const MAX_FRAME_SECS: f32 = 0.05;

/// What opens the dial.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Request {
    /// The window the chosen percent is posted to ...
    pub(crate) notify: Hwnd,
    /// ... as this message, with the percent in wparam.
    pub(crate) message: u32,
    /// The size the dial starts at.
    pub(crate) percent: u32,
    /// Play a click on every detent (`Settings.dial_click_sound`).
    pub(crate) click_sound: bool,
}

/// Opens the dial, or brings the open one to the front.
pub(crate) fn open(request: Request) -> windows::core::Result<Hwnd> {
    if let Some(hwnd) = live(WINDOW.with(Cell::get)) {
        raise(hwnd);
        return Ok(hwnd.0 as Hwnd);
    }
    let (dpi, work) = monitor_under_cursor()?;
    let ui = Ui::new(request, dpi)?;
    let size = ui.layout.size_px();
    UI.with(|slot| *slot.borrow_mut() = Some(ui));
    match create(&POPUP, bottom_right(&work, size, dpi), size) {
        Ok(hwnd) => {
            WINDOW.with(|w| w.set(hwnd.0 as isize));
            raise(hwnd);
            Ok(hwnd.0 as Hwnd)
        }
        Err(e) => {
            UI.with(|slot| slot.borrow_mut().take());
            Err(e)
        }
    }
}

/// Destroys the dial window if one is open.
pub(crate) fn close() -> windows::core::Result<()> {
    with_ui(|ui| ui.clicker.close());
    close_window(WINDOW.with(Cell::get))
}

/// Destroys the window from inside; from here on no click is played.
fn close_from_inside(hwnd: HWND) {
    with_ui(|ui| ui.clicker.close());
    destroy(hwnd);
}

/// Posts the chosen percent; false when the app window is gone, so the dial has no purpose.
fn post_change(percent: u32) -> bool {
    let Some(notify) = with_ui(|ui| ui.notify) else {
        return false;
    };
    // SAFETY: posting to a window handle that may be stale is harmless.
    unsafe {
        PostMessageW(
            Some(to_hwnd(notify.window)),
            notify.message,
            WPARAM(percent as usize),
            LPARAM(0),
        )
    }
    .is_ok()
}

/// Starts the frame timer when something still moves; without a timer the motion jumps to its
/// end so the window never shows a half-finished move.
fn drive_animation(hwnd: HWND) {
    let start = with_ui(|ui| {
        ui.dirty = true;
        let start = ui.dial.is_animating() && !ui.timer_running;
        if start {
            ui.last_tick = Instant::now();
        }
        start
    });
    if start == Some(true) {
        // SAFETY: our own window and timer id.
        let timer = unsafe { SetTimer(Some(hwnd), ANIMATION_TIMER, FRAME_MS, None) };
        with_ui(|ui| {
            ui.timer_running = timer != 0;
            if timer == 0 {
                ui.dial.settle_now();
            }
        });
    }
    invalidate(hwnd);
}

fn react(hwnd: HWND, reaction: Reaction) {
    if let Some(percent) = reaction.committed {
        with_ui(|ui| ui.committed(percent, reaction.clicks));
        if !post_change(percent) {
            close_from_inside(hwnd);
            return;
        }
    }
    if reaction.close {
        close_from_inside(hwnd);
        return;
    }
    drive_animation(hwnd);
}

fn on_timer(hwnd: HWND) {
    let still_moving = with_ui(|ui| {
        let now = Instant::now();
        let dt = now.duration_since(ui.last_tick).as_secs_f32();
        ui.last_tick = now;
        ui.dial.tick(dt.min(MAX_FRAME_SECS));
        ui.dirty = true;
        ui.dial.is_animating()
    });
    if still_moving == Some(false) {
        // SAFETY: our own window and timer id; an already stopped timer is not an error here.
        unsafe {
            let _ = KillTimer(Some(hwnd), ANIMATION_TIMER);
        }
        with_ui(|ui| ui.timer_running = false);
    }
    invalidate(hwnd);
}

fn on_paint(hwnd: HWND) {
    let mut ps = PAINTSTRUCT::default();
    // SAFETY: standard paint cycle on our own window.
    unsafe {
        let dc = BeginPaint(hwnd, &mut ps);
        with_ui(|ui| ui.paint_to(dc));
        let _ = EndPaint(hwnd, &ps);
    }
}

fn on_mouse_down(hwnd: HWND, lparam: LPARAM) {
    let (x, y) = point_of(lparam);
    let grabbed = with_ui(|ui| {
        let (dx, dy) = ui.offset_from_center(x, y);
        let grabbed = dx.hypot(dy) <= ui.layout.grab_radius();
        if grabbed {
            ui.dial.press(dx, dy);
        }
        grabbed
    });
    if grabbed == Some(true) {
        // SAFETY: our own window; capture keeps the drag going outside it.
        unsafe { SetCapture(hwnd) };
    }
}

fn on_mouse_move(hwnd: HWND, lparam: LPARAM) {
    let (x, y) = point_of(lparam);
    let reaction = with_ui(|ui| {
        let (dx, dy) = ui.offset_from_center(x, y);
        ui.dial.drag_to(dx, dy)
    });
    if let Some(reaction) = reaction.filter(|r| r.committed.is_some()) {
        react(hwnd, reaction);
    } else if with_ui(|ui| ui.dial.is_dragging()) == Some(true) {
        drive_animation(hwnd);
    }
}

fn on_mouse_up(hwnd: HWND) {
    // SAFETY: only releases a capture this thread holds; none is not an error worth acting on.
    unsafe {
        let _ = ReleaseCapture();
    }
    on_capture_lost(hwnd);
}

/// The drag ends, whether by mouse up or by losing the capture.
fn on_capture_lost(hwnd: HWND) {
    if let Some(reaction) = with_ui(|ui| ui.dial.release()) {
        react(hwnd, reaction);
    }
}

fn key_of(vk: u16) -> Option<Key> {
    Some(match vk {
        v if v == VK_LEFT.0 || v == VK_DOWN.0 => Key::Move(Step::Previous),
        v if v == VK_RIGHT.0 || v == VK_UP.0 => Key::Move(Step::Next),
        v if v == VK_HOME.0 => Key::Move(Step::First),
        v if v == VK_END.0 => Key::Move(Step::Last),
        v if v == VK_RETURN.0 || v == VK_SPACE.0 => Key::Confirm,
        v if v == VK_ESCAPE.0 => Key::Cancel,
        _ => return None,
    })
}

fn on_activate(hwnd: HWND, wparam: WPARAM) {
    if with_ui(|ui| ui.activation.user_left(wparam)) == Some(true) {
        close_from_inside(hwnd);
    }
}

fn handled(f: impl FnOnce()) -> LRESULT {
    f();
    LRESULT(0)
}

fn on_key(hwnd: HWND, wparam: WPARAM) {
    let Some(key) = key_of(wparam.0 as u16) else {
        return;
    };
    if let Some(reaction) = with_ui(|ui| ui.dial.key(key)) {
        react(hwnd, reaction);
    }
}

fn on_wheel(hwnd: HWND, wparam: WPARAM) {
    let delta = i32::from((wparam.0 >> 16) as u16 as i16);
    if let Some(reaction) = with_ui(|ui| ui.dial.wheel(delta)) {
        react(hwnd, reaction);
    }
}

/// The window is gone: forget it and its state.
fn on_nc_destroy() {
    WINDOW.with(|w| w.set(0));
    UI.with(|slot| slot.borrow_mut().take());
}

unsafe extern "system" fn dial_proc(
    hwnd: HWND,
    id: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match id {
        WM_PAINT => handled(|| on_paint(hwnd)),
        WM_ERASEBKGND => LRESULT(1),
        WM_TIMER if wparam.0 == ANIMATION_TIMER => handled(|| on_timer(hwnd)),
        WM_LBUTTONDOWN => handled(|| on_mouse_down(hwnd, lparam)),
        WM_MOUSEMOVE => handled(|| on_mouse_move(hwnd, lparam)),
        WM_LBUTTONUP => handled(|| on_mouse_up(hwnd)),
        WM_CAPTURECHANGED => handled(|| on_capture_lost(hwnd)),
        WM_KEYDOWN => handled(|| on_key(hwnd, wparam)),
        WM_MOUSEWHEEL => handled(|| on_wheel(hwnd, wparam)),
        WM_ACTIVATE => handled(|| on_activate(hwnd, wparam)),
        WM_NCDESTROY => {
            on_nc_destroy();
            // SAFETY: default handling.
            unsafe { DefWindowProcW(hwnd, id, wparam, lparam) }
        }
        // SAFETY: default handling.
        _ => unsafe { DefWindowProcW(hwnd, id, wparam, lparam) },
    }
}
