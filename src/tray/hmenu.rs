//! Turns the menu model into a Win32 popup menu and maps the chosen id back to a command.

use super::command::TrayCommand;
use super::model::{MenuItem, command_ids};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetMenuItemCount, HMENU, MENU_ITEM_FLAGS,
    MF_BYPOSITION, MF_CHECKED, MF_DISABLED, MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING,
    SetMenuDefaultItem,
};
use windows::core::PCWSTR;

use crate::win32::wide;

/// Id of a disabled entry; `TrackPopupMenu` never returns it and 0 means "nothing chosen".
const HEADING_ID: usize = 0;

/// Doubles `&` so menu labels do not turn exe names into mnemonics.
fn escape_mnemonic(s: &str) -> String {
    s.replace('&', "&&")
}

/// The command behind a menu id as returned by `TrackPopupMenu`; `None` for 0 ("nothing chosen").
pub(super) fn command_for_id(model: &[MenuItem], id: i32) -> Option<TrayCommand> {
    usize::try_from(id)
        .ok()
        .and_then(|i| i.checked_sub(1))
        .and_then(|i| command_ids(model).into_iter().nth(i))
}

/// Turns the model into a popup menu; ids follow `command_ids`. The caller destroys the menu.
pub(super) fn build_hmenu(model: &[MenuItem]) -> windows::core::Result<HMENU> {
    let mut next_id = 1usize;
    build_level(model, &mut next_id)
}

fn build_level(model: &[MenuItem], next_id: &mut usize) -> windows::core::Result<HMENU> {
    // SAFETY: plain creation call without pointers.
    let menu = unsafe { CreatePopupMenu()? };
    for item in model {
        if let Err(e) = append_item(menu, item, next_id) {
            destroy_menu(menu);
            return Err(e);
        }
    }
    Ok(menu)
}

pub(super) fn destroy_menu(menu: HMENU) {
    // SAFETY: callers pass a menu they created and that no other menu owns.
    // Nothing can be done about a failed destroy, the handle just leaks.
    unsafe {
        let _ = DestroyMenu(menu);
    }
}

fn append_item(menu: HMENU, item: &MenuItem, next_id: &mut usize) -> windows::core::Result<()> {
    if item.separator {
        // SAFETY: separators take no id and no text.
        return unsafe { AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null()) };
    }
    let text = wide(&escape_mnemonic(&item.label));
    if item.heading {
        return append_heading(menu, &text);
    }
    let flags = item_flags(item);
    if item.submenu.is_empty() {
        let id = take_command_id(item, next_id);
        return append_entry(menu, flags, id, &text);
    }
    append_submenu(menu, item, flags | MF_POPUP, &text, next_id)
}

/// Bold text that cannot be chosen. `MF_DISABLED` without `MF_GRAYED` keeps the normal text color, so
/// the title reads in light and dark menus; the bold comes from making it the default item.
fn append_heading(menu: HMENU, text: &[u16]) -> windows::core::Result<()> {
    append_entry(menu, MF_STRING | MF_DISABLED, HEADING_ID, text)?;
    // SAFETY: `menu` is valid and the position is the entry appended just above.
    let position = unsafe { GetMenuItemCount(Some(menu)) } - 1;
    // Only the bold look is lost when this fails, so the heading stays.
    let _ = unsafe { SetMenuDefaultItem(menu, position as u32, MF_BYPOSITION.0) };
    Ok(())
}

fn item_flags(item: &MenuItem) -> MENU_ITEM_FLAGS {
    let mut flags = MF_STRING;
    if item.checked {
        flags |= MF_CHECKED;
    }
    if !item.enabled {
        flags |= MF_GRAYED;
    }
    flags
}

/// Ids are handed out only to entries that carry a command, in `command_ids` order.
fn take_command_id(item: &MenuItem, next_id: &mut usize) -> usize {
    let id = *next_id;
    if item.command.is_some() {
        *next_id += 1;
    }
    id
}

fn append_submenu(
    menu: HMENU,
    item: &MenuItem,
    flags: MENU_ITEM_FLAGS,
    text: &[u16],
    next_id: &mut usize,
) -> windows::core::Result<()> {
    let sub = build_level(&item.submenu, next_id)?;
    let res = append_entry(menu, flags, sub.0 as usize, text);
    if res.is_err() {
        // The append failed, so `menu` does not own the submenu and nobody else does.
        destroy_menu(sub);
    }
    res
}

fn append_entry(
    menu: HMENU,
    flags: MENU_ITEM_FLAGS,
    id: usize,
    text: &[u16],
) -> windows::core::Result<()> {
    // SAFETY: text is NUL-terminated and outlives the call; a popup handle is owned by `menu` afterwards.
    unsafe { AppendMenuW(menu, flags, id, PCWSTR(text.as_ptr())) }
}

#[cfg(test)]
mod tests {
    use super::super::model::{Foreground, menu_model};
    use super::*;
    use crate::config::Settings;

    /// Builds the real HMENU (no display) and checks every leaf id against `command_ids`.
    #[test]
    fn hmenu_ids_match_model() {
        use windows::Win32::UI::WindowsAndMessaging::{
            GetMenuItemCount, GetMenuItemID, GetSubMenu,
        };
        fn walk(menu: HMENU, out: &mut Vec<u32>) {
            // SAFETY: menu is a valid menu built by build_hmenu.
            unsafe {
                for pos in 0..GetMenuItemCount(Some(menu)) {
                    let sub = GetSubMenu(menu, pos);
                    if sub.0.is_null() {
                        let id = GetMenuItemID(menu, pos);
                        if id != 0 && id != u32::MAX {
                            out.push(id);
                        }
                    } else {
                        walk(sub, out);
                    }
                }
            }
        }
        let model = menu_model(&Settings::default(), &Foreground::app("a&b.exe"));
        let menu = build_hmenu(&model).unwrap();
        let mut ids = Vec::new();
        walk(menu, &mut ids);
        // SAFETY: menu was created above.
        unsafe {
            let _ = DestroyMenu(menu);
        }
        let expected: Vec<u32> = (1..=command_ids(&model).len() as u32).collect();
        assert_eq!(ids, expected);
    }

    /// The first entry is the heading: text "Pin It", disabled, bold (default item), no id.
    #[test]
    fn heading_is_a_bold_disabled_entry_without_id() {
        use windows::Win32::UI::WindowsAndMessaging::{
            GMDI_USEDISABLED, GetMenuDefaultItem, GetMenuItemID, GetMenuState, GetMenuStringW,
        };
        let model = menu_model(&Settings::default(), &Foreground::default());
        let menu = build_hmenu(&model).unwrap();
        // SAFETY: menu was built above and is destroyed below.
        let (state, id, default, text) = unsafe {
            let mut buf = [0u16; 16];
            let len = GetMenuStringW(menu, 0, Some(&mut buf), MF_BYPOSITION) as usize;
            (
                GetMenuState(menu, 0, MF_BYPOSITION),
                GetMenuItemID(menu, 0),
                GetMenuDefaultItem(menu, 1, GMDI_USEDISABLED),
                String::from_utf16_lossy(&buf[..len]),
            )
        };
        destroy_menu(menu);
        assert_eq!(text, "Pin It");
        assert_ne!(state & MF_DISABLED.0, 0, "cannot be chosen");
        assert_eq!(state & MF_GRAYED.0, 0, "not greyed out: stays legible");
        assert_eq!(id, 0);
        assert_eq!(default, 0, "bold default item");
    }

    #[test]
    fn mnemonic_escape() {
        assert_eq!(escape_mnemonic("a&b"), "a&&b");
    }
}
