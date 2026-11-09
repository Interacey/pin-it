//! The Win32 side of the excluded apps window: one popup with its own window procedure on the
//! main thread and an EDIT child for the input.
//!
//! The window never touches the `App`. A change the user asks for is queued as an `Action` and
//! announced with one message to the notify window; the app runs it through the normal command
//! path and hands the resulting list back with `refresh`, so the window shows what the settings
//! hold.
//!
//! The state lives in a thread-local because there is at most one window. Handlers copy what
//! they need out of it and release it before they call a Win32 function that can send messages
//! to this window (`DestroyWindow`, `SetWindowTextW`, `SetFocus`, ...).

use std::cell::{Cell, RefCell};
use std::time::Instant;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, BitBlt, EndPaint, HDC, PAINTSTRUCT, SRCCOPY};
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, EN_CHANGE, EN_KILLFOCUS, EN_SETFOCUS, KillTimer, PostMessageW, SetTimer,
    WM_ACTIVATE, WM_COMMAND, WM_CTLCOLOREDIT, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCDESTROY, WM_PAINT, WM_SETFOCUS, WM_TIMER,
};
use windows::core::w;

use super::actions::{Action, push};
use super::input::{self, Brush};
use super::layout::Layout;
use super::model::{Hit, ListState};
use super::render::{InputLook, Scene, render_window};
use super::text::{Fonts, TextPass};
use crate::Hwnd;
use crate::popup::Theme;
use crate::popup::draw::to_bgra;
use crate::popup::placement::{bottom_right, monitor_under_cursor};
use crate::popup::window::{
    Activation, PopupClass, close as close_window, create, destroy, invalidate, live, raise,
};
use crate::win32::Dib;
use crate::win32::{BASE_DPI, to_hwnd};

mod keys;
mod pointer;
#[cfg(test)]
pub(crate) mod probe;

pub(in crate::exclusions) use keys::key_pressed;
use pointer::{on_mouse_down, on_mouse_leave, on_mouse_move, on_mouse_up, on_wheel};

const POPUP: PopupClass = PopupClass {
    name: w!("PinItExclusions"),
    title: w!("Excluded apps"),
    proc: Some(exclusions_proc),
};
const ANIMATION_TIMER: usize = 1;
const FRAME_MS: u32 = 16;
/// Longest frame time fed to the flash, so a stalled thread does not make it jump.
const MAX_FRAME_SECS: f32 = 0.05;

thread_local! {
    /// The open window; 0 when there is none.
    static WINDOW: Cell<isize> = const { Cell::new(0) };
    static UI: RefCell<Option<Ui>> = const { RefCell::new(None) };
}

/// Where the message that announces queued actions is posted.
#[derive(Debug, Clone, Copy)]
struct Notify {
    window: Hwnd,
    message: u32,
}

struct Ui {
    list: ListState,
    canvas: Dib,
    fonts: Fonts,
    /// Background of the EDIT control.
    brush: Brush,
    layout: Layout,
    theme: Theme,
    notify: Notify,
    activation: Activation,
    /// The EDIT control; 0 until it is created.
    edit: Hwnd,
    focused: bool,
    /// The last entry was refused and the hint is shown.
    invalid: bool,
    /// What the left button went down on.
    pressed: Option<Hit>,
    /// A `WM_MOUSELEAVE` is on its way, so the hover is cleared when the pointer leaves.
    leave_tracked: bool,
    /// The picture has to be drawn again before the next copy to the screen.
    dirty: bool,
    timer_running: bool,
    last_tick: Instant,
}

impl Ui {
    fn new(notify: Notify, entries: &[String], dpi: u32) -> windows::core::Result<Self> {
        let layout = Layout::new(dpi as f32 / BASE_DPI as f32);
        let theme = Theme::system();
        let (width, height) = layout.size_px();
        Ok(Self {
            list: ListState::new(entries, layout),
            canvas: Dib::new(width, height)?,
            fonts: Fonts::new(layout.scale)?,
            brush: Brush::new(theme.surface)?,
            layout,
            theme,
            notify,
            activation: Activation::new(),
            edit: 0,
            focused: false,
            invalid: false,
            pressed: None,
            leave_tracked: false,
            dirty: true,
            timer_running: false,
            last_tick: Instant::now(),
        })
    }

    fn input_look(&self) -> InputLook {
        if self.invalid {
            InputLook::Invalid
        } else if self.focused {
            InputLook::Focused
        } else {
            InputLook::Idle
        }
    }

    /// Draws the picture and its texts into the canvas.
    fn redraw(&mut self) {
        let scene = Scene {
            theme: self.theme,
            list: &self.list,
            input: self.input_look(),
        };
        let Some(pixmap) = render_window(&self.layout, &scene) else {
            return;
        };
        self.canvas.write(&to_bgra(pixmap));
        TextPass {
            dc: self.canvas.dc,
            fonts: &self.fonts,
            layout: self.layout,
            theme: self.theme,
            list: &self.list,
            hint: self.invalid,
        }
        .draw();
        self.dirty = false;
    }
}

fn with_ui<R>(f: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    UI.with(|ui| {
        ui.try_borrow_mut()
            .ok()
            .and_then(|mut ui| ui.as_mut().map(f))
    })
}

fn current() -> Option<HWND> {
    live(WINDOW.with(Cell::get))
}

/// Opens the window for the programs in `entries`, or brings the open one to the front. Changes
/// the user asks for are queued (`drain_actions`) and announced by posting `message` to `notify`.
pub(crate) fn open(notify: Hwnd, message: u32, entries: &[String]) -> windows::core::Result<Hwnd> {
    if let Some(hwnd) = current() {
        raise(hwnd);
        return Ok(hwnd.0 as Hwnd);
    }
    let (dpi, work) = monitor_under_cursor()?;
    let notify = Notify {
        window: notify,
        message,
    };
    let ui = Ui::new(notify, entries, dpi)?;
    let size = ui.layout.size_px();
    UI.with(|slot| *slot.borrow_mut() = Some(ui));
    let opened = create(&POPUP, bottom_right(&work, size, dpi), size).and_then(|hwnd| {
        WINDOW.with(|w| w.set(hwnd.0 as isize));
        add_input(hwnd)?;
        Ok(hwnd)
    });
    match opened {
        Ok(hwnd) => {
            // The activation by `raise` passes the focus to the input (`WM_SETFOCUS`).
            raise(hwnd);
            Ok(hwnd.0 as Hwnd)
        }
        Err(e) => {
            // `destroy` runs `on_nc_destroy`, which forgets the window and the state; without a
            // window only the state is left.
            if let Some(hwnd) = current() {
                destroy(hwnd);
            }
            UI.with(|slot| slot.borrow_mut().take());
            Err(e)
        }
    }
}

fn add_input(hwnd: HWND) -> windows::core::Result<()> {
    let edit = with_ui(|ui| input::create(hwnd, &ui.layout, &ui.fonts.input))
        .ok_or_else(|| windows::core::Error::from_hresult(windows::Win32::Foundation::E_FAIL))??;
    with_ui(|ui| ui.edit = edit.0 as Hwnd);
    Ok(())
}

fn edit_window() -> Option<HWND> {
    with_ui(|ui| to_hwnd(ui.edit)).filter(|edit| !edit.0.is_null())
}

fn focus_input() {
    if let Some(edit) = edit_window() {
        input::focus(edit);
    }
}

/// Destroys the window if one is open.
pub(crate) fn close() -> windows::core::Result<()> {
    close_window(WINDOW.with(Cell::get))
}

/// Shows `entries` as the list, as the settings hold it now. Does nothing without a window.
pub(crate) fn refresh(entries: &[String]) {
    if let Some(hwnd) = current() {
        with_ui(|ui| ui.list.refresh(entries));
        drive_animation(hwnd);
    }
}

/// Posts the announcement of a queued action; false when the app window is gone, so the window
/// has no purpose.
fn post_change() -> bool {
    let Some(notify) = with_ui(|ui| ui.notify) else {
        return false;
    };
    // SAFETY: posting to a window handle that may be stale is harmless.
    unsafe {
        PostMessageW(
            Some(to_hwnd(notify.window)),
            notify.message,
            WPARAM(0),
            LPARAM(0),
        )
    }
    .is_ok()
}

/// Queues `action` and announces it; a vanished app window closes this one.
fn request(hwnd: HWND, action: Action) {
    push(action);
    if !post_change() {
        destroy(hwnd);
    }
}

/// Starts the frame timer while a row flashes, and draws the next frame.
fn drive_animation(hwnd: HWND) {
    let start = with_ui(|ui| {
        ui.dirty = true;
        let start = ui.list.is_animating() && !ui.timer_running;
        if start {
            ui.last_tick = Instant::now();
        }
        start
    });
    if start == Some(true) {
        // SAFETY: our own window and timer id.
        let timer = unsafe { SetTimer(Some(hwnd), ANIMATION_TIMER, FRAME_MS, None) };
        with_ui(|ui| ui.timer_running = timer != 0);
    }
    invalidate(hwnd);
}

fn on_timer(hwnd: HWND) {
    let still_flashing = with_ui(|ui| {
        let now = Instant::now();
        let dt = now.duration_since(ui.last_tick).as_secs_f32();
        ui.last_tick = now;
        ui.dirty = true;
        ui.list.tick(dt.min(MAX_FRAME_SECS))
    });
    if still_flashing == Some(false) {
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
        with_ui(|ui| {
            if ui.dirty {
                ui.redraw();
            }
            let (w, h) = (ui.canvas.width as i32, ui.canvas.height as i32);
            // A failed copy leaves the previous frame on screen until the next paint.
            let _ = BitBlt(dc, 0, 0, w, h, Some(ui.canvas.dc), 0, 0, SRCCOPY);
        });
        let _ = EndPaint(hwnd, &ps);
    }
}

fn on_activate(hwnd: HWND, wparam: WPARAM) {
    if with_ui(|ui| ui.activation.user_left(wparam)) == Some(true) {
        destroy(hwnd);
    }
}

/// Notifications of the EDIT control: focus changes move the ring, and any change of the text
/// takes the hint and the selection away.
fn on_command(hwnd: HWND, wparam: WPARAM) {
    let code = ((wparam.0 >> 16) & 0xFFFF) as u32;
    let changed = with_ui(|ui| match code {
        EN_SETFOCUS => ui.focused = true,
        EN_KILLFOCUS => ui.focused = false,
        EN_CHANGE => {
            ui.invalid = false;
            ui.list.clear_selection();
        }
        _ => {}
    });
    if changed.is_some() {
        drive_animation(hwnd);
    }
}

/// Colors of the EDIT control; `None` when the state is not available.
fn on_ctl_color_edit(wparam: WPARAM) -> Option<LRESULT> {
    let dc = HDC(wparam.0 as *mut _);
    with_ui(|ui| ui.brush.paint_edit(dc, ui.theme.text, ui.theme.surface))
}

/// The window is gone: forget it and its state.
fn on_nc_destroy() {
    WINDOW.with(|w| w.set(0));
    UI.with(|slot| slot.borrow_mut().take());
}

fn handled(f: impl FnOnce()) -> LRESULT {
    f();
    LRESULT(0)
}

unsafe extern "system" fn exclusions_proc(
    hwnd: HWND,
    id: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match id {
        WM_PAINT => handled(|| on_paint(hwnd)),
        WM_ERASEBKGND => LRESULT(1),
        WM_TIMER if wparam.0 == ANIMATION_TIMER => handled(|| on_timer(hwnd)),
        WM_MOUSEMOVE => handled(|| on_mouse_move(hwnd, lparam)),
        WM_MOUSELEAVE => handled(|| on_mouse_leave(hwnd)),
        WM_LBUTTONDOWN => handled(|| on_mouse_down(hwnd, lparam)),
        WM_LBUTTONUP => handled(|| on_mouse_up(hwnd, lparam)),
        WM_MOUSEWHEEL => handled(|| on_wheel(hwnd, wparam)),
        WM_KEYDOWN => handled(|| {
            key_pressed(wparam.0 as u16);
        }),
        WM_SETFOCUS => handled(focus_input),
        WM_ACTIVATE => handled(|| on_activate(hwnd, wparam)),
        WM_COMMAND => handled(|| on_command(hwnd, wparam)),
        WM_CTLCOLOREDIT => match on_ctl_color_edit(wparam) {
            Some(brush) => brush,
            // SAFETY: default handling.
            None => unsafe { DefWindowProcW(hwnd, id, wparam, lparam) },
        },
        WM_NCDESTROY => {
            on_nc_destroy();
            // SAFETY: default handling.
            unsafe { DefWindowProcW(hwnd, id, wparam, lparam) }
        }
        // SAFETY: default handling.
        _ => unsafe { DefWindowProcW(hwnd, id, wparam, lparam) },
    }
}
