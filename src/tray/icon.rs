//! The tray icon: the application icon from the exe resources, with a stock icon as fallback.

use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForSystem, GetSystemMetricsForDpi};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, HICON, IDI_APPLICATION, IMAGE_ICON, LR_DEFAULTCOLOR, LoadIconW, LoadImageW,
    SM_CXSMICON, SM_CYSMICON,
};
use windows::core::PCWSTR;

/// Resource id of the icon in `assets/pin_it.rc` (`1 ICON "pin_it.ico"`).
const APP_ICON_RESOURCE_ID: usize = 1;

/// An `HICON` for the notification area. A private copy loaded with `LoadImageW` is destroyed on
/// drop; the shared stock icon of the fallback must not be.
pub(super) struct TrayIcon {
    handle: HICON,
    owned: bool,
}

impl TrayIcon {
    /// Loads the exe's application icon at the small-icon size of the system DPI. Without the
    /// resource (test binaries do not embed it) it falls back to the stock application icon.
    pub(super) fn load() -> windows::core::Result<TrayIcon> {
        let (width, height) = small_icon_size();
        match load_app_icon(width, height) {
            Ok(handle) => Ok(TrayIcon {
                handle,
                owned: true,
            }),
            Err(_) => load_stock_icon(),
        }
    }

    pub(super) fn handle(&self) -> HICON {
        self.handle
    }
}

impl Drop for TrayIcon {
    fn drop(&mut self) {
        if self.owned {
            // SAFETY: the handle came from LoadImageW without LR_SHARED and nothing uses it
            // afterwards. A failed destroy only leaks it.
            let _ = unsafe { DestroyIcon(self.handle) };
        }
    }
}

fn small_icon_size() -> (i32, i32) {
    // SAFETY: plain metric queries.
    unsafe {
        let dpi = GetDpiForSystem();
        (
            GetSystemMetricsForDpi(SM_CXSMICON, dpi),
            GetSystemMetricsForDpi(SM_CYSMICON, dpi),
        )
    }
}

fn load_app_icon(width: i32, height: i32) -> windows::core::Result<HICON> {
    // SAFETY: a MAKEINTRESOURCE name (integer in the low word) is what LoadImageW expects for
    // numeric resource ids.
    let image = unsafe {
        let module = GetModuleHandleW(None)?;
        LoadImageW(
            Some(module.into()),
            PCWSTR(APP_ICON_RESOURCE_ID as *const u16),
            IMAGE_ICON,
            width,
            height,
            LR_DEFAULTCOLOR,
        )?
    };
    Ok(HICON(image.0))
}

fn load_stock_icon() -> windows::core::Result<TrayIcon> {
    // SAFETY: IDI_APPLICATION is a system icon; the system owns it.
    let handle = unsafe { LoadIconW(None, IDI_APPLICATION)? };
    Ok(TrayIcon {
        handle,
        owned: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Threading::{
        GR_GDIOBJECTS, GR_USEROBJECTS, GetCurrentProcess, GetGuiResources,
    };
    use windows::Win32::UI::WindowsAndMessaging::LR_LOADFROMFILE;
    use windows::core::HSTRING;

    const LOADS: usize = 50;

    fn gui_objects() -> (u32, u32) {
        // SAFETY: queries the current process pseudo handle.
        unsafe {
            let process = GetCurrentProcess();
            (
                GetGuiResources(process, GR_GDIOBJECTS),
                GetGuiResources(process, GR_USEROBJECTS),
            )
        }
    }

    /// Loads the real artwork from the assets folder, so the owned path runs in test binaries
    /// that carry no icon resource.
    fn load_from_asset_file() -> TrayIcon {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/pin_it.ico");
        // SAFETY: the name is a valid NUL-terminated wide string for the duration of the call.
        let image = unsafe {
            LoadImageW(
                None,
                &HSTRING::from(path),
                IMAGE_ICON,
                16,
                16,
                LR_LOADFROMFILE,
            )
        }
        .expect("assets/pin_it.ico loads");
        TrayIcon {
            handle: HICON(image.0),
            owned: true,
        }
    }

    #[test]
    fn load_returns_a_usable_handle() {
        let icon = TrayIcon::load().expect("icon or fallback");
        assert!(!icon.handle().is_invalid());
    }

    /// Growth of the process-wide GUI object counts over `LOADS` calls of `load`. The counts include
    /// objects that other tests of this process create and free at the same time, so the window test
    /// lock is held and the result is a growth, not an exact difference: noise from tests that do not
    /// take the lock is a few objects, while a leak adds at least one object per load.
    fn gui_growth_over_loads(load: impl Fn()) -> (u32, u32) {
        let _guard = crate::window_test_guard();
        load();
        let before = gui_objects();
        for _ in 0..LOADS {
            load();
        }
        let after = gui_objects();
        (
            after.0.saturating_sub(before.0),
            after.1.saturating_sub(before.1),
        )
    }

    /// A leak shows up in the USER count too (one icon object per load), so either count crossing
    /// `LOADS` is a leak and anything below is noise.
    fn assert_no_leak((gdi, user): (u32, u32)) {
        let limit = u32::try_from(LOADS).expect("LOADS fits u32");
        assert!(
            gdi < limit && user < limit,
            "GDI grew by {gdi}, USER by {user} over {LOADS} loads"
        );
    }

    #[test]
    fn owned_icons_do_not_leak_gui_objects() {
        assert_no_leak(gui_growth_over_loads(|| {
            let icon = load_from_asset_file();
            assert!(!icon.handle().is_invalid());
        }));
    }

    #[test]
    fn fallback_path_does_not_leak_gui_objects() {
        assert_no_leak(gui_growth_over_loads(|| {
            drop(load_stock_icon().expect("stock icon"));
        }));
    }

    #[test]
    #[should_panic(expected = "GDI grew by")]
    fn the_leak_check_catches_a_leaking_loader() {
        assert_no_leak(gui_growth_over_loads(|| {
            std::mem::forget(load_from_asset_file());
        }));
    }
}
