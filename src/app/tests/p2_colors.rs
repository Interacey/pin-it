//! A color setting changed in the tray reaches every button at once, pinned or not.

use super::support::{Wins, click, eventually_swept, options, pump_ms, temp_dir};
use crate::Hwnd;
use crate::anim::ButtonAnim;
use crate::app::Runtime;
use crate::app::host::with_app;
use crate::config::Settings;
use crate::overlay::{ButtonStyle, render_button};
use crate::tray::TrayCommand;
use crate::zorder::is_topmost;

/// The style the app holds for the button of `h`.
fn style_of(h: Hwnd) -> Option<ButtonStyle> {
    with_app(|a| a.entries.get(&h).and_then(|e| e.style)).flatten()
}

/// The pixels the button of that style is drawn with once the pin animation has settled.
fn rendered(style: &ButtonStyle) -> Vec<u8> {
    let mut anim = ButtonAnim::new(true);
    anim.set_pinned(style.pinned);
    render_button(32, style, &anim)
}

#[test]
fn a_fixed_accent_chosen_in_the_tray_restyles_pinned_and_unpinned_buttons_at_once() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(2);
    let w = wins.0.clone();
    let dir = temp_dir("p2-colors");
    let settings = Settings {
        animate: false,
        ..Settings::default()
    };
    let rt = Runtime::start(&options(&dir, w.clone(), &settings)).unwrap();
    eventually_swept(&rt, "two buttons", || rt.button_count() == 2);
    click(rt.button_hwnd(w[0]).unwrap());
    eventually_swept(&rt, "pinned", || is_topmost(w[0]));
    pump_ms(100);

    let before = style_of(w[0]).expect("style of the pinned window");
    let before_other = style_of(w[1]).expect("style of the unpinned window");
    assert!(before.pinned && !before_other.pinned);
    let before_pixels = rendered(&before);

    let red = [200, 30, 30];
    assert_ne!(before.accent, red, "setup: the window color is not red");
    rt.run_tray_command(&TrayCommand::SetFixedColor(red));
    let pinned = style_of(w[0]).unwrap();
    assert_eq!(pinned.accent, red, "pinned button keeps the old accent");
    assert_ne!(
        rendered(&pinned),
        before_pixels,
        "pinned button looks the same"
    );
    assert_eq!(
        style_of(w[1]).unwrap().accent,
        red,
        "unpinned entry keeps the old accent"
    );

    rt.run_tray_command(&TrayCommand::ToggleWindowColor);
    let back = style_of(w[0]).unwrap();
    assert_eq!(back.accent, before.accent, "window color is used again");
    assert_eq!(rendered(&back), before_pixels);
    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}

fn invalidations() -> u32 {
    with_app(|a| a.colors.invalidation_count()).expect("app is idle")
}

#[test]
fn system_color_messages_invalidate_the_color_cache_even_when_the_app_is_busy() {
    use super::support::post;
    use crate::app::system_change::{WM_DWM_COLORIZATION_CHANGED, WM_THEME_CHANGED};
    use windows::Win32::UI::WindowsAndMessaging::WM_SETTINGCHANGE;

    let _g = crate::window_test_guard();
    let wins = Wins::new(1);
    let dir = temp_dir("p2-syscolors");
    let rt = Runtime::start(&options(&dir, wins.0.clone(), &Settings::default())).unwrap();
    pump_ms(100);

    let start = invalidations();
    post(rt.hwnd(), WM_DWM_COLORIZATION_CHANGED, 0, 0);
    pump_ms(100);
    assert_eq!(invalidations(), start + 1, "WM_DWMCOLORIZATIONCOLORCHANGED");

    post(rt.hwnd(), WM_THEME_CHANGED, 0, 0);
    pump_ms(100);
    assert_eq!(invalidations(), start + 2, "WM_THEMECHANGED");

    let name: Vec<u16> = "ImmersiveColorSet\0".encode_utf16().collect();
    let other: Vec<u16> = "Policy\0".encode_utf16().collect();
    // Sent, not posted: the string is only valid during the call, as with the real broadcast.
    send(rt.hwnd(), WM_SETTINGCHANGE, name.as_ptr() as isize);
    assert_eq!(
        invalidations(),
        start + 3,
        "WM_SETTINGCHANGE ImmersiveColorSet"
    );
    send(rt.hwnd(), WM_SETTINGCHANGE, other.as_ptr() as isize);
    send(rt.hwnd(), WM_SETTINGCHANGE, 0);
    assert_eq!(invalidations(), start + 3, "other settings must not count");

    // While an App method runs, the message is posted again and handled afterwards, without a panic.
    rt.while_busy(|| {
        send(rt.hwnd(), WM_SETTINGCHANGE, name.as_ptr() as isize);
        post(rt.hwnd(), WM_DWM_COLORIZATION_CHANGED, 0, 0);
    });
    pump_ms(150);
    assert_eq!(
        invalidations(),
        start + 5,
        "messages that arrived while busy"
    );
    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}

fn send(hwnd: isize, msg: u32, lparam: isize) {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::SendMessageW;
    // SAFETY: own hidden window; `lparam` is null or a live string of the caller.
    unsafe {
        SendMessageW(
            crate::win32::to_hwnd(hwnd),
            msg,
            Some(WPARAM(0)),
            Some(LPARAM(lparam)),
        )
    };
}
