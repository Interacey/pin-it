//! Window class and window procedure of the overlay.

use std::sync::Once;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, GWLP_USERDATA, GetWindowLongPtrW, HTTRANSPARENT, IDC_HAND, IsWindowVisible,
    KillTimer, LoadCursorW, MA_NOACTIVATE, PostMessageW, RegisterClassExW, SW_HIDE,
    SetWindowLongPtrW, ShowWindow, WM_CAPTURECHANGED, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_NCDESTROY, WM_NCHITTEST, WM_TIMER, WNDCLASSEXW,
};
use windows::core::w;

use super::input::Click;
use super::pulse::WM_PULSE;
use super::stacking::{ORDER_WATCH_TIMER_ID, RESTACK_TIMERS, restack_pair};
use super::state::{State, TIMER_ID};

use crate::win32::to_hwnd;

/// WM_MOUSELEAVE, not exported by windows-rs.
const WM_MOUSELEAVE: u32 = 0x02A3;

pub(super) fn register_class() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // SAFETY: registers a class with a static window procedure.
        unsafe {
            let hinst = GetModuleHandleW(None).unwrap_or_default();
            let class = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(wndproc),
                hInstance: hinst.into(),
                hCursor: LoadCursorW(None, IDC_HAND).unwrap_or_default(),
                lpszClassName: w!("PinItOverlay"),
                ..Default::default()
            };
            RegisterClassExW(&class);
        }
    });
}

fn lparam_xy(lp: LPARAM) -> (i32, i32) {
    (i32::from(lp.0 as i16), i32::from((lp.0 >> 16) as i16))
}

/// Runs `f` on the window's state, if it is still attached.
///
/// Win32 calls that can send messages back to this window (SetCapture, ReleaseCapture,
/// ShowWindow, SetWindowPos, DestroyWindow) must be made outside `f`. The window
/// procedure only calls this in the arms that use the state, so no two `&mut State` overlap.
fn with_state<R>(hwnd: HWND, f: impl FnOnce(&mut State) -> R) -> Option<R> {
    // SAFETY: GWLP_USERDATA is either null or the `State` owned by the PinButton;
    // it is cleared in WM_NCDESTROY before the PinButton frees it.
    let st = unsafe { (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State).as_mut() };
    st.map(f)
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => return LRESULT(MA_NOACTIVATE as isize),
        WM_NCHITTEST if with_state(hwnd, |st| st.is_invisible()) == Some(true) => {
            return LRESULT(HTTRANSPARENT as isize);
        }
        WM_MOUSEMOVE => {
            with_state(hwnd, State::mouse_move);
        }
        WM_MOUSELEAVE => {
            with_state(hwnd, |st| st.set_hover(false));
        }
        WM_LBUTTONDOWN => on_button_down(hwnd),
        WM_LBUTTONUP => on_button_up(hwnd, lparam_xy(lp)),
        WM_CAPTURECHANGED => {
            with_state(hwnd, State::capture_lost);
        }
        WM_TIMER if wp.0 == TIMER_ID => on_animation_timer(hwnd),
        WM_TIMER if wp.0 == ORDER_WATCH_TIMER_ID => on_order_watch(hwnd),
        WM_PULSE => on_pulse(hwnd),
        WM_TIMER if RESTACK_TIMERS.iter().any(|&(id, _)| id == wp.0) => {
            on_restack_timer(hwnd, wp.0);
        }
        WM_NCDESTROY => on_destroyed(hwnd),
        _ => {}
    }
    // SAFETY: forwards the unchanged arguments.
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

/// One animation frame; a finished fade-out ends with the window hidden.
fn on_animation_timer(hwnd: HWND) {
    with_state(hwnd, State::on_timer);
    if with_state(hwnd, |st| st.take_fade_hide()) == Some(true) {
        // SAFETY: our own window, no `&mut State` is live (the call re-enters this procedure).
        let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
        with_state(hwnd, State::forget_mouse);
    }
}

fn on_button_down(hwnd: HWND) {
    if with_state(hwnd, State::button_down).is_some() {
        // SAFETY: capture for our own window while handling its message.
        unsafe { SetCapture(hwnd) };
    }
}

fn on_button_up(hwnd: HWND, (x, y): (i32, i32)) {
    let Some(click) = with_state(hwnd, |st| st.button_up(x, y)) else {
        return;
    };
    // SAFETY: releases our own capture; failure when not captured is harmless.
    let _ = unsafe { ReleaseCapture() };
    if let Some(click) = click {
        post_click(click);
    }
}

fn post_click(click: Click) {
    // SAFETY: posting to a window handle is safe even if it no longer exists.
    // A failed post means the notify window is gone, so nobody is left to be told.
    let _ = unsafe {
        PostMessageW(
            Some(to_hwnd(click.notify)),
            click.message,
            WPARAM(click.target as usize),
            LPARAM(0),
        )
    };
}

fn on_destroyed(hwnd: HWND) {
    with_state(hwnd, |st| {
        st.hwnd = HWND::default();
        st.timer_on = false;
    });
    // SAFETY: detaches the state so no later message can reach it.
    unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
}

/// One tick of the z-order watch: the overlay goes back above its target when something buried it,
/// and a held mouse button starts the pulse.
fn on_order_watch(hwnd: HWND) {
    restack_visible(hwnd);
    with_state(hwnd, State::watch_mouse);
}

/// One message of the 1 ms pulse.
fn on_pulse(hwnd: HWND) {
    restack_visible(hwnd);
    with_state(hwnd, State::pulse_handled);
}

/// Puts the overlay above its target when it is shown.
fn restack_visible(hwnd: HWND) {
    let Some(target) = with_state(hwnd, |st| to_hwnd(st.target)) else {
        return;
    };
    // SAFETY: plain query on our own window.
    if unsafe { IsWindowVisible(hwnd) }.as_bool() {
        restack_pair(hwnd, target);
    }
}

/// The one-shot restack after an activation: ends timer `timer_id` and puts the overlay above its target.
fn on_restack_timer(hwnd: HWND, timer_id: usize) {
    // SAFETY: our own timer; a failed kill only means it fires again and restacks once more.
    let _ = unsafe { KillTimer(Some(hwnd), timer_id) };
    restack_visible(hwnd);
}
