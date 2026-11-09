//! Read access to the open window for the tests in `app::tests`.

use std::cell::Cell;

use super::{Brush, Hit, Hwnd, Theme, WINDOW, with_ui};

/// The popup window.
pub(crate) fn window() -> Hwnd {
    WINDOW.with(Cell::get)
}

/// How often the window passed the focus to the input since the thread began.
pub(crate) fn focus_requests() -> u32 {
    crate::exclusions::input::FOCUS_REQUESTS.with(Cell::get)
}

/// The EDIT control.
pub(crate) fn edit() -> Option<Hwnd> {
    with_ui(|ui| ui.edit)
}

pub(crate) fn entries() -> Option<Vec<String>> {
    with_ui(|ui| ui.list.entries().to_vec())
}

/// The hint about an invalid entry is shown.
pub(crate) fn hint_shown() -> Option<bool> {
    with_ui(|ui| ui.invalid)
}

pub(crate) fn selected() -> Option<usize> {
    with_ui(|ui| ui.list.selected()).flatten()
}

pub(crate) fn scroll() -> Option<f32> {
    with_ui(|ui| ui.list.scroll())
}

/// A row flashes right now.
pub(crate) fn flashing(index: usize) -> Option<bool> {
    with_ui(|ui| ui.list.flash_level(index) > 0.0)
}

/// Client point in the middle of the trash button of row `index` (it must be visible).
pub(crate) fn trash_point(index: usize) -> Option<(i32, i32)> {
    with_ui(|ui| {
        let row = ui.list.row_area(index);
        let b = ui.layout.trash_box(&row);
        ((b.x + b.w / 2.0) as i32, (b.y + b.h / 2.0) as i32)
    })
}

/// Client point on the name of row `index` (it must be visible).
pub(crate) fn name_point(index: usize) -> Option<(i32, i32)> {
    with_ui(|ui| {
        let n = ui.layout.name_area(&ui.list.row_area(index));
        ((n.x + 8.0) as i32, (n.y + n.h / 2.0) as i32)
    })
}

/// The window as it is on screen right now, as RGBA with its size. The EDIT control is a
/// child window and not part of it.
pub(crate) fn picture() -> Option<(u32, u32, Vec<u8>)> {
    with_ui(|ui| {
        ui.redraw();
        (ui.canvas.width, ui.canvas.height, ui.canvas.read_rgba())
    })
}

/// Makes the next redraw show the hover, focus and error states like real use would.
pub(crate) fn set_look(hover: Option<(usize, bool)>, focused: bool, invalid: bool) {
    with_ui(|ui| {
        let hit = hover.map(|(i, trash)| if trash { Hit::Trash(i) } else { Hit::Row(i) });
        ui.list.set_hover(hit);
        ui.focused = focused;
        ui.invalid = invalid;
    });
}

/// Shows the window in `theme` instead of the system's, including the control's colors.
pub(crate) fn set_theme(theme: Theme) {
    with_ui(|ui| {
        ui.theme = theme;
        ui.brush = Brush::new(theme.surface).expect("brush");
        ui.dirty = true;
    });
}
