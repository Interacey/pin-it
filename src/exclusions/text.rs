//! GDI text of the excluded apps window: the header, the program names, the empty state and the
//! hint about an invalid entry.

use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Gdi::{
    DT_CENTER, DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, FW_NORMAL,
    FW_SEMIBOLD, HDC, IntersectClipRect, SelectClipRgn,
};

use super::layout::{Area, Layout};
use super::model::ListState;
use crate::popup::Theme;
use crate::popup::text::{Font, draw_in_rect, set_color};

pub(super) const HEADER_TEXT: &str = "Excluded apps";
pub(super) const EMPTY_TEXT: &str = "No excluded apps";
pub(super) const HINT_TEXT: &str = "Not a valid program name";
pub(super) const CUE_TEXT: &str = "Add a program, e.g. chrome.exe";

/// Font heights in dp.
const HEADER_FONT_DP: f32 = 13.0;
const BODY_FONT_DP: f32 = 13.0;
const HINT_FONT_DP: f32 = 12.0;
const INPUT_FONT_DP: f32 = 14.0;

/// The fonts of the window; the input font is also handed to the EDIT control.
pub(super) struct Fonts {
    header: Font,
    body: Font,
    hint: Font,
    pub(super) input: Font,
}

impl Fonts {
    pub(super) fn new(scale: f32) -> windows::core::Result<Self> {
        Ok(Self {
            header: Font::new(HEADER_FONT_DP, scale, FW_SEMIBOLD.0 as i32)?,
            body: Font::new(BODY_FONT_DP, scale, FW_NORMAL.0 as i32)?,
            hint: Font::new(HINT_FONT_DP, scale, FW_NORMAL.0 as i32)?,
            input: Font::new(INPUT_FONT_DP, scale, FW_NORMAL.0 as i32)?,
        })
    }
}

fn rect_of(area: &Area) -> RECT {
    RECT {
        left: area.x.round() as i32,
        top: area.y.round() as i32,
        right: area.right().round() as i32,
        bottom: area.bottom().round() as i32,
    }
}

/// Everything needed to draw the texts of one frame onto `dc`.
pub(super) struct TextPass<'a> {
    pub(super) dc: HDC,
    pub(super) fonts: &'a Fonts,
    pub(super) layout: Layout,
    pub(super) theme: Theme,
    pub(super) list: &'a ListState,
    /// The hint about an invalid entry is shown.
    pub(super) hint: bool,
}

impl TextPass<'_> {
    pub(super) fn draw(&self) {
        let single_line = DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX;
        set_color(self.dc, self.theme.text);
        let header = rect_of(&self.layout.header());
        draw_in_rect(
            self.dc,
            &self.fonts.header,
            header,
            HEADER_TEXT,
            single_line,
        );
        if self.list.entries().is_empty() {
            self.empty_state();
        } else {
            self.names();
        }
        if self.hint {
            set_color(self.dc, self.theme.error);
            let hint = rect_of(&self.layout.hint());
            draw_in_rect(self.dc, &self.fonts.hint, hint, HINT_TEXT, single_line);
        }
    }

    fn empty_state(&self) {
        set_color(self.dc, self.theme.muted);
        let format = DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX;
        let area = rect_of(&self.layout.list());
        draw_in_rect(self.dc, &self.fonts.body, area, EMPTY_TEXT, format);
    }

    /// The names, cut off at the edges of the list and shortened with an ellipsis at the trash
    /// button.
    fn names(&self) {
        let list = rect_of(&self.layout.list());
        let format = DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS;
        set_color(self.dc, self.theme.text);
        // SAFETY: `dc` is a live memory DC; the clip is removed again below.
        unsafe { IntersectClipRect(self.dc, list.left, list.top, list.right, list.bottom) };
        for index in self.list.visible() {
            let name = rect_of(&self.layout.name_area(&self.list.row_area(index)));
            draw_in_rect(
                self.dc,
                &self.fonts.body,
                name,
                &self.list.entries()[index],
                format,
            );
        }
        // SAFETY: see above.
        unsafe { SelectClipRgn(self.dc, None) };
    }
}
