//! The state behind the dial window: the thread-local `Ui` with the dial, the pictures and the
//! click sound, and the drawing of one frame into the canvas.
//!
//! The state lives in a thread-local because there is at most one dial. Handlers copy what they
//! need out of it and release it before they call a Win32 function that can send messages to
//! this window (`DestroyWindow`, `SetCapture`, `InvalidateRect`, ...).

use std::cell::{Cell, RefCell};
use std::time::Instant;

use windows::Win32::Graphics::Gdi::{BitBlt, HDC, SRCCOPY};

use super::Request;
use crate::Hwnd;
use crate::dial::render::{Layout, Renderer, Scene};
use crate::dial::size::{effective_size, readout};
use crate::dial::sound::{Clicker, default_player};
use crate::dial::state::{DEAD_ZONE_DP, Dial};
use crate::dial::text::{Fonts, TextPass};
use crate::popup::Theme;
use crate::popup::draw::to_bgra;
use crate::popup::window::Activation;
use crate::win32::{BASE_DPI, Dib};

thread_local! {
    /// The open dial window; 0 when there is none.
    pub(super) static WINDOW: Cell<isize> = const { Cell::new(0) };
    pub(super) static UI: RefCell<Option<Ui>> = const { RefCell::new(None) };
}

/// Where the chosen percent is posted.
#[derive(Debug, Clone, Copy)]
pub(super) struct Notify {
    pub(super) window: Hwnd,
    pub(super) message: u32,
}

pub(super) struct Ui {
    pub(super) dial: Dial,
    pub(super) canvas: Dib,
    fonts: Fonts,
    renderer: Renderer,
    pub(super) layout: Layout,
    theme: Theme,
    dpi: u32,
    pub(super) notify: Notify,
    pub(super) activation: Activation,
    pub(super) clicker: Clicker,
    /// The picture has to be drawn again before the next copy to the screen.
    pub(super) dirty: bool,
    pub(super) timer_running: bool,
    pub(super) last_tick: Instant,
}

impl Ui {
    pub(super) fn new(request: Request, dpi: u32) -> windows::core::Result<Self> {
        Self::with_theme(request, dpi, Theme::system())
    }

    pub(super) fn with_theme(
        request: Request,
        dpi: u32,
        theme: Theme,
    ) -> windows::core::Result<Self> {
        let layout = Layout::new(dpi as f32 / BASE_DPI as f32);
        let (width, height) = layout.size_px();
        let renderer = Renderer::new(layout, theme)
            .ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_FAIL))?;
        Ok(Self {
            dial: Dial::new(request.percent, DEAD_ZONE_DP * layout.scale),
            canvas: Dib::new(width, height)?,
            fonts: Fonts::new(layout.scale)?,
            renderer,
            layout,
            theme,
            dpi,
            notify: Notify {
                window: request.notify,
                message: request.message,
            },
            activation: Activation::new(),
            clicker: Clicker::new(request.click_sound, request.percent, default_player()),
            dirty: true,
            timer_running: false,
            last_tick: Instant::now(),
        })
    }

    /// Draws the picture and its texts into the canvas.
    pub(super) fn redraw(&mut self) {
        let percent = self.dial.percent();
        let scene = Scene {
            angle: self.dial.angle(),
            flash: self.dial.flash(),
            glyph_px: effective_size(percent, self.dpi).px,
        };
        self.canvas.write(&to_bgra(self.renderer.frame(&scene)));
        let text = TextPass {
            dc: self.canvas.dc,
            fonts: &self.fonts,
            layout: self.layout,
            theme: self.theme,
        };
        text.scale_labels();
        text.readout(&readout(percent), self.dial.pulse_scale());
        self.dirty = false;
    }

    /// Copies the canvas to `dc`, drawing it first when it is out of date.
    pub(super) fn paint_to(&mut self, dc: HDC) {
        if self.dirty {
            self.redraw();
        }
        let (w, h) = (self.canvas.width as i32, self.canvas.height as i32);
        // SAFETY: `dc` comes from the paint cycle of our window and the canvas DC is valid.
        // A failed copy leaves the previous frame on screen until the next paint.
        let _ = unsafe { BitBlt(dc, 0, 0, w, h, Some(self.canvas.dc), 0, 0, SRCCOPY) };
    }

    /// Pointer position (client pixels) relative to the dial center.
    pub(super) fn offset_from_center(&self, x: i32, y: i32) -> (f32, f32) {
        let (cx, cy) = self.layout.center();
        (x as f32 - cx, y as f32 - cy)
    }

    /// The chosen value changed: the easter egg may start, and one click per detent crossed
    /// plays, as far as the setting, the rate limit and a running jingle allow.
    pub(super) fn committed(&mut self, percent: u32, crossings: u32) {
        let now = Instant::now();
        self.clicker.committed(percent, now);
        for _ in 0..crossings {
            self.clicker.click(now);
        }
    }
}

pub(super) fn with_ui<R>(f: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    UI.with(|ui| {
        ui.try_borrow_mut()
            .ok()
            .and_then(|mut ui| ui.as_mut().map(f))
    })
}
