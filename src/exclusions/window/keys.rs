//! Keys of the excluded apps window: Enter adds, Esc closes, Up and Down move the selection and
//! Delete removes the selected row. They arrive from the EDIT control (through its subclass) or
//! from the window itself.

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    VK_DELETE, VK_DOWN, VK_ESCAPE, VK_RETURN, VK_UP,
};

use super::{current, drive_animation, edit_window, request, with_ui};
use crate::exclusions::actions::Action;
use crate::exclusions::input;
use crate::exclusions::model::normalize_entry;
use crate::popup::window::destroy;

/// The user pressed Enter: a valid name is queued (once the app has it, the list shows it), a
/// name already in the list only flashes, and an invalid one shows the hint and stays in the field.
fn submit(hwnd: HWND) {
    let Some(edit) = edit_window() else {
        return;
    };
    let Ok(name) = normalize_entry(&input::text(edit)) else {
        with_ui(|ui| ui.invalid = true);
        drive_animation(hwnd);
        return;
    };
    let known = with_ui(|ui| {
        ui.list.highlight(&name);
        ui.list.contains(&name)
    });
    if known == Some(false) {
        request(hwnd, Action::Add(name));
    }
    // The change event of the control hides the hint and clears the selection. A failed clear
    // leaves the text in the field, where the user sees it.
    let _ = input::set_text(edit, "");
    drive_animation(hwnd);
}

/// Asks the app to remove the program in row `index`.
pub(super) fn remove_row(hwnd: HWND, index: usize) {
    let name = with_ui(|ui| ui.list.entries().get(index).cloned()).flatten();
    if let Some(name) = name {
        request(hwnd, Action::Remove(name));
    }
}

/// Delete removes the selected row, but only while the field is empty: otherwise the key belongs
/// to the text.
fn delete_selected(hwnd: HWND) -> bool {
    let field_empty = edit_window().is_none_or(input::is_empty);
    let selected = with_ui(|ui| ui.list.selected()).flatten();
    match selected {
        Some(index) if field_empty => {
            remove_row(hwnd, index);
            true
        }
        _ => false,
    }
}

/// A key from the window or from the EDIT control. True when the window took it; false leaves
/// it to the control (typing, Delete in text, ...).
pub(in crate::exclusions) fn key_pressed(vk: u16) -> bool {
    let Some(hwnd) = current() else {
        return false;
    };
    match vk {
        v if v == VK_ESCAPE.0 => destroy(hwnd),
        v if v == VK_RETURN.0 => submit(hwnd),
        v if v == VK_UP.0 => move_selection(hwnd, -1),
        v if v == VK_DOWN.0 => move_selection(hwnd, 1),
        v if v == VK_DELETE.0 => return delete_selected(hwnd),
        _ => return false,
    }
    true
}

fn move_selection(hwnd: HWND, delta: i32) {
    with_ui(|ui| ui.list.select_step(delta));
    drive_animation(hwnd);
}
