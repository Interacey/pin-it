//! The excluded apps window with its own message loop: messages are posted to the window and its
//! EDIT control only, never sent as input to other applications.

use windows::Win32::Foundation::{HANDLE, LPARAM, WPARAM};
use windows::Win32::Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow};
use windows::Win32::System::ApplicationInstallationAndServicing::{
    ACTCTXW, ActivateActCtx, CreateActCtxW, DeactivateActCtx, ReleaseActCtx,
};
use windows::Win32::UI::Controls::{EM_GETCUEBANNER, EM_GETLIMITTEXT};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    VIRTUAL_KEY, VK_DELETE, VK_DOWN, VK_ESCAPE, VK_RETURN, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    ES_AUTOHSCROLL, ES_MULTILINE, GWL_EXSTYLE, GWL_STYLE, GetWindowLongW, IsWindow, SendMessageW,
    SetWindowTextW, WM_ACTIVATE, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
    WM_MOUSEWHEEL, WM_SETFOCUS, WS_BORDER, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows::core::PCWSTR;

use super::support::{CHANGED, Recorder, eventually, post, pump_ms, received};
use crate::Hwnd;
use crate::exclusions::{self, Action, drain_actions, probe};
use crate::win32::{to_hwnd, wide};

/// Closes the window when a test ends, also when it fails, and empties the action queue.
struct CloseWindow;

impl Drop for CloseWindow {
    fn drop(&mut self) {
        let _ = exclusions::close();
        drain_actions();
        pump_ms(20);
    }
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

/// Opens the window for `list` with an empty action queue.
fn open(rec: &Recorder, list: &[&str]) -> Hwnd {
    drain_actions();
    exclusions::open(rec.0, CHANGED, &names(list)).unwrap()
}

fn is_open(h: Hwnd) -> bool {
    unsafe { IsWindow(Some(to_hwnd(h))).as_bool() }
}

fn lparam(point: (i32, i32)) -> isize {
    ((point.1 as isize & 0xFFFF) << 16) | (point.0 as isize & 0xFFFF)
}

/// Puts `text` into the EDIT control like typing would (the control sends its change event).
fn type_text(text: &str) {
    let edit = to_hwnd(probe::edit().unwrap());
    let wide = wide(text);
    unsafe { SetWindowTextW(edit, PCWSTR(wide.as_ptr())).unwrap() };
    pump_ms(10);
}

fn field_text() -> String {
    let edit = to_hwnd(probe::edit().unwrap());
    let mut buf = [0u16; 400];
    let n = unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowTextW(edit, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// A key as the EDIT control receives it while it has the focus.
fn key_in_field(vk: VIRTUAL_KEY) {
    post(probe::edit().unwrap(), WM_KEYDOWN, vk.0 as usize, 0);
    pump_ms(20);
}

fn click(h: Hwnd, point: (i32, i32)) {
    for m in [WM_MOUSEMOVE, WM_LBUTTONDOWN, WM_LBUTTONUP] {
        post(h, m, 0, lparam(point));
        pump_ms(10);
    }
    pump_ms(20);
}

fn wheel(h: Hwnd, delta: i16) {
    post(h, WM_MOUSEWHEEL, (delta as u16 as usize) << 16, 0);
    pump_ms(20);
}

#[test]
fn opening_twice_gives_one_window() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let _close = CloseWindow;
    let first = open(&rec, &["a.exe"]);
    let second = open(&rec, &["a.exe", "b.exe"]);
    assert_eq!(first, second);
    assert_eq!(
        probe::entries(),
        Some(names(&["a.exe"])),
        "the open window keeps its list"
    );
}

#[test]
fn the_window_is_a_tool_popup_without_a_taskbar_button() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let _close = CloseWindow;
    let h = open(&rec, &[]);
    let style = unsafe { GetWindowLongW(to_hwnd(h), GWL_STYLE) } as u32;
    let ex = unsafe { GetWindowLongW(to_hwnd(h), GWL_EXSTYLE) } as u32;
    assert_ne!(style & WS_POPUP.0, 0);
    assert_ne!(ex & WS_EX_TOOLWINDOW.0, 0);
    assert_eq!(ex & WS_EX_APPWINDOW.0, 0);
}

/// Common controls version 6 as the exe's manifest asks for it. The lib test harness has no
/// manifest of its own, and without it an EDIT control has no cue banner.
struct ManifestContext {
    handle: HANDLE,
    cookie: usize,
}

impl ManifestContext {
    fn activate() -> Self {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/pin_it.manifest");
        let source = wide(path);
        let spec = ACTCTXW {
            cbSize: size_of::<ACTCTXW>() as u32,
            lpSource: PCWSTR(source.as_ptr()),
            ..Default::default()
        };
        let mut cookie = 0usize;
        unsafe {
            let handle = CreateActCtxW(&spec).expect("manifest activation context");
            ActivateActCtx(Some(handle), &mut cookie).expect("activate");
            Self { handle, cookie }
        }
    }
}

impl Drop for ManifestContext {
    fn drop(&mut self) {
        unsafe {
            let _ = DeactivateActCtx(0, self.cookie);
            ReleaseActCtx(self.handle);
        }
    }
}

#[test]
fn the_input_is_a_borderless_single_line_edit_with_the_file_name_limit_and_a_cue() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let _manifest = ManifestContext::activate();
    let _close = CloseWindow;
    open(&rec, &[]);
    let edit = to_hwnd(probe::edit().unwrap());
    let style = unsafe { GetWindowLongW(edit, GWL_STYLE) } as u32;
    assert_ne!(style & ES_AUTOHSCROLL as u32, 0, "scrolls horizontally");
    assert_eq!(style & ES_MULTILINE as u32, 0, "single line");
    assert_eq!(style & WS_BORDER.0, 0, "the window paints the field");
    let limit = unsafe { SendMessageW(edit, EM_GETLIMITTEXT, None, None) };
    assert_eq!(limit.0, 255);
    let mut cue = [0u16; 64];
    let ok = unsafe {
        SendMessageW(
            edit,
            EM_GETCUEBANNER,
            Some(WPARAM(cue.as_mut_ptr() as usize)),
            Some(LPARAM(cue.len() as isize)),
        )
    };
    let cue_len = cue.iter().position(|&c| c == 0).unwrap_or(cue.len());
    assert_ne!(ok.0, 0, "the control reports no cue banner");
    assert_eq!(
        String::from_utf16_lossy(&cue[..cue_len]),
        "Add a program, e.g. chrome.exe"
    );
}

#[test]
fn enter_adds_the_normalized_name_posts_one_message_and_clears_the_input() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let _close = CloseWindow;
    open(&rec, &["a.exe"]);
    type_text("  Chrome ");
    assert!(drain_actions().is_empty(), "typing alone queues nothing");
    key_in_field(VK_RETURN);
    pump_ms(30);
    assert_eq!(drain_actions(), [Action::Add("chrome.exe".into())]);
    assert_eq!(received(), [0]);
    assert_eq!(field_text(), "");
    assert_eq!(probe::hint_shown(), Some(false));
}

#[test]
fn an_invalid_name_shows_the_hint_posts_nothing_and_keeps_the_text_until_typing_goes_on() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let _close = CloseWindow;
    open(&rec, &[]);
    type_text("a/b.exe");
    key_in_field(VK_RETURN);
    assert_eq!(probe::hint_shown(), Some(true));
    assert_eq!(field_text(), "a/b.exe", "the text stays");
    assert!(drain_actions().is_empty());
    assert!(received().is_empty());
    type_text("ab.exe");
    assert_eq!(probe::hint_shown(), Some(false), "typing hides the hint");
    key_in_field(VK_RETURN);
    assert_eq!(drain_actions(), [Action::Add("ab.exe".into())]);
}

#[test]
fn an_empty_input_shows_the_hint_too() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let _close = CloseWindow;
    open(&rec, &[]);
    key_in_field(VK_RETURN);
    assert_eq!(probe::hint_shown(), Some(true));
    assert!(received().is_empty());
}

#[test]
fn a_duplicate_is_ignored_silently_the_input_is_cleared_and_the_row_flashes() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let _close = CloseWindow;
    open(&rec, &["a.exe", "b.exe"]);
    type_text("B");
    key_in_field(VK_RETURN);
    assert!(drain_actions().is_empty());
    assert!(received().is_empty());
    assert_eq!(field_text(), "");
    assert_eq!(probe::hint_shown(), Some(false));
    assert_eq!(probe::flashing(1), Some(true));
    assert_eq!(probe::flashing(0), Some(false));
    eventually("the flash is over", || probe::flashing(1) == Some(false));
}

#[test]
fn clicking_a_trash_button_removes_that_program_at_once_without_a_question() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let _close = CloseWindow;
    let h = open(&rec, &["a.exe", "b.exe", "c.exe"]);
    click(h, probe::trash_point(1).unwrap());
    assert_eq!(drain_actions(), [Action::Remove("b.exe".into())]);
    assert_eq!(received(), [0]);
}

#[test]
fn pressing_on_a_trash_button_and_releasing_elsewhere_removes_nothing() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let _close = CloseWindow;
    let h = open(&rec, &["a.exe", "b.exe"]);
    post(h, WM_LBUTTONDOWN, 0, lparam(probe::trash_point(0).unwrap()));
    pump_ms(10);
    post(h, WM_LBUTTONUP, 0, lparam(probe::name_point(0).unwrap()));
    pump_ms(10);
    assert!(drain_actions().is_empty());
    assert!(received().is_empty());
}

#[test]
fn clicking_a_name_selects_the_row_and_removes_nothing() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let _close = CloseWindow;
    let h = open(&rec, &["a.exe", "b.exe"]);
    click(h, probe::name_point(1).unwrap());
    assert_eq!(probe::selected(), Some(1));
    assert!(drain_actions().is_empty());
}

#[test]
fn up_and_down_select_rows_and_delete_removes_the_selected_one_when_the_field_is_empty() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let _close = CloseWindow;
    open(&rec, &["a.exe", "b.exe", "c.exe"]);
    key_in_field(VK_DOWN);
    key_in_field(VK_DOWN);
    assert_eq!(probe::selected(), Some(1));
    key_in_field(VK_UP);
    assert_eq!(probe::selected(), Some(0));
    key_in_field(VK_DELETE);
    assert_eq!(drain_actions(), [Action::Remove("a.exe".into())]);
}

#[test]
fn delete_belongs_to_the_text_while_the_field_has_text() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let _close = CloseWindow;
    open(&rec, &["a.exe", "b.exe"]);
    key_in_field(VK_DOWN);
    assert_eq!(probe::selected(), Some(0));
    type_text("abc");
    assert_eq!(probe::selected(), None, "typing drops the selection");
    key_in_field(VK_DELETE);
    assert!(drain_actions().is_empty());
    assert_eq!(
        field_text(),
        "bc",
        "the control deleted the character after the caret"
    );
    // A row selected while the field has text is not removed by Delete either.
    key_in_field(VK_DOWN);
    assert_eq!(probe::selected(), Some(0));
    key_in_field(VK_DELETE);
    assert!(drain_actions().is_empty());
    assert_eq!(field_text(), "c");
    // Delete without a selection removes nothing.
    type_text("");
    key_in_field(VK_DELETE);
    assert!(drain_actions().is_empty());
}

#[test]
fn the_wheel_scrolls_a_long_list_and_stops_at_its_ends() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let _close = CloseWindow;
    let many: Vec<String> = (0..20).map(|i| format!("app{i:02}.exe")).collect();
    drain_actions();
    let h = exclusions::open(rec.0, CHANGED, &many).unwrap();
    assert_eq!(probe::scroll(), Some(0.0));
    wheel(h, -120);
    assert_near(probe::scroll(), 36.0 * scale());
    wheel(h, 120);
    wheel(h, 120);
    assert_near(probe::scroll(), 0.0);
    for _ in 0..40 {
        wheel(h, -120);
    }
    assert_near(probe::scroll(), (20.0 * 36.0 - 260.0) * scale());
}

fn assert_near(actual: Option<f32>, expected: f32) {
    let actual = actual.expect("window is open");
    assert!((actual - expected).abs() < 0.01, "{actual} vs {expected}");
}

/// Pixels per dp of the window under test, from its own dpi.
fn scale() -> f32 {
    let dpi = crate::win32::window_dpi(to_hwnd(probe::window()));
    dpi as f32 / 96.0
}

#[test]
fn escape_closes_from_the_field_and_the_window_can_open_again() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let _close = CloseWindow;
    let h = open(&rec, &["a.exe"]);
    key_in_field(VK_ESCAPE);
    eventually("window closed", || !is_open(h));
    assert!(received().is_empty());
    assert_eq!(probe::entries(), None);
    let again = open(&rec, &["b.exe"]);
    assert!(is_open(again));
    assert_eq!(probe::entries(), Some(names(&["b.exe"])));
}

#[test]
fn escape_closes_from_the_window_itself_too() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let _close = CloseWindow;
    let h = open(&rec, &[]);
    post(h, WM_KEYDOWN, VK_ESCAPE.0 as usize, 0);
    eventually("window closed", || !is_open(h));
}

#[test]
fn losing_the_activation_closes_the_window_but_not_before_it_was_active() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let _close = CloseWindow;
    let h = open(&rec, &[]);
    post(h, WM_ACTIVATE, 0, 0); // the system refused the focus at opening
    pump_ms(30);
    assert!(is_open(h), "an inactive start is no reason to close");
    pump_ms(600); // past the opening grace, in which a deactivation is not the user leaving
    post(h, WM_ACTIVATE, 1, 0);
    pump_ms(10);
    post(h, WM_ACTIVATE, 0, 0);
    eventually("window closed", || !is_open(h));
}

#[test]
fn a_window_that_gets_the_focus_passes_it_to_the_input() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let _close = CloseWindow;
    let h = open(&rec, &[]);
    let before = probe::focus_requests();
    post(h, WM_SETFOCUS, 0, 0);
    pump_ms(30);
    assert_eq!(probe::focus_requests(), before + 1);
}

#[test]
fn close_destroys_an_open_window_and_is_fine_without_one() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let h = open(&rec, &[]);
    exclusions::close().unwrap();
    assert!(!is_open(h));
    assert_eq!(probe::entries(), None);
    exclusions::close().unwrap();
}

#[test]
fn a_request_that_cannot_be_posted_closes_the_window() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let _close = CloseWindow;
    let h = open(&rec, &["a.exe"]);
    drop(rec);
    click(h, probe::trash_point(0).unwrap());
    eventually("window closed", || !is_open(h));
}

#[test]
fn refresh_shows_the_list_the_settings_hold_and_is_harmless_without_a_window() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let _close = CloseWindow;
    exclusions::refresh(&names(&["x.exe"]));
    open(&rec, &["a.exe"]);
    exclusions::refresh(&names(&["a.exe", "b.exe"]));
    assert_eq!(probe::entries(), Some(names(&["a.exe", "b.exe"])));
}

/// A new name that arrives with the refresh after the user added it flashes.
#[test]
fn the_row_of_an_added_name_flashes_when_the_list_arrives() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let _close = CloseWindow;
    open(&rec, &["a.exe"]);
    type_text("b");
    key_in_field(VK_RETURN);
    assert_eq!(drain_actions(), [Action::Add("b.exe".into())]);
    exclusions::refresh(&names(&["a.exe", "b.exe"]));
    assert_eq!(probe::flashing(1), Some(true));
}

/// Writes the window as it is drawn when `PIN_IT_EXCLUSIONS_PNG_DIR` is set, for a visual check.
#[test]
fn write_window_pictures_for_review() {
    let Some(dir) = std::env::var_os("PIN_IT_EXCLUSIONS_PNG_DIR") else {
        return;
    };
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let _manifest = ManifestContext::activate();
    let _close = CloseWindow;
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    let many: Vec<String> = (0..20).map(|i| format!("program{i:02}.exe")).collect();
    let few = ["chrome.exe", "code.exe", "notepad.exe"];
    // (name, entries, hover, focused, invalid, scroll notches)
    type Case<'a> = (&'a str, Vec<String>, Option<(usize, bool)>, bool, bool, i16);
    let cases: Vec<Case> = vec![
        ("empty", vec![], None, false, false, 0),
        ("three", names(&few), None, false, false, 0),
        ("twenty-scrolled", many.clone(), None, true, false, -3),
        ("hover-trash", names(&few), Some((1, true)), true, false, 0),
        ("hover-row", names(&few), Some((2, false)), false, false, 0),
        ("error", names(&few), None, true, true, 0),
    ];
    for (theme_name, theme) in [
        ("dark", crate::popup::Theme::DARK),
        ("light", crate::popup::Theme::LIGHT),
    ] {
        for (name, entries, hover, focused, invalid, notches) in &cases {
            drain_actions();
            let h = exclusions::open(rec.0, CHANGED, entries).unwrap();
            probe::set_theme(theme);
            if *notches != 0 {
                for _ in 0..notches.abs() {
                    wheel(h, notches.signum() * 120);
                }
            }
            probe::set_look(*hover, *focused, *invalid);
            let (w, hgt, rgba) = probe::picture().unwrap();
            let size = tiny_skia::IntSize::from_wh(w, hgt).unwrap();
            let pixmap = tiny_skia::Pixmap::from_vec(rgba, size).unwrap();
            pixmap
                .save_png(dir.join(format!("{theme_name}-{name}.png")))
                .unwrap();
            exclusions::close().unwrap();
            assert!(!is_open(h));
        }
        write_input_pictures(&rec, &dir, theme_name, theme);
    }
}

/// The window with its EDIT control as Windows paints it (`PrintWindow`), empty and with text
/// longer than the field.
fn write_input_pictures(
    rec: &Recorder,
    dir: &std::path::Path,
    name: &str,
    theme: crate::popup::Theme,
) {
    let long = "a-very-long-program-name-that-does-not-fit-the-field.exe";
    for (label, text) in [("cue", ""), ("text", "notepad"), ("long", long)] {
        drain_actions();
        let h = exclusions::open(rec.0, CHANGED, &names(&["chrome.exe", "code.exe"])).unwrap();
        probe::set_theme(theme);
        type_text(text);
        pump_ms(50);
        let (w, hgt, _) = probe::picture().unwrap();
        let dib = crate::win32::Dib::new(w, hgt).unwrap();
        let printed = unsafe { PrintWindow(to_hwnd(h), dib.dc, PRINT_WINDOW_FLAGS(2)) };
        assert!(printed.as_bool(), "PrintWindow failed");
        let size = tiny_skia::IntSize::from_wh(w, hgt).unwrap();
        let pixmap = tiny_skia::Pixmap::from_vec(dib.read_rgba(), size).unwrap();
        pixmap
            .save_png(dir.join(format!("{name}-input-{label}.png")))
            .unwrap();
        exclusions::close().unwrap();
    }
}
