//! Real-window checks on private test windows; run with `cargo test -- --ignored --nocapture`.

use super::primitives::NO_FOCUS_FLAGS;
use super::*;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    GetSystemMetrics, HWND_TOP, PM_REMOVE, PeekMessageW, RegisterClassW, SM_CXSCREEN, SM_CYSCREEN,
    SW_SHOWNA, SetWindowPos, ShowWindow, TranslateMessage, UnregisterClassW, WNDCLASSW,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_OVERLAPPEDWINDOW,
};
use windows::core::w;

use crate::Hwnd;
use crate::win32::to_hwnd;

unsafe extern "system" fn proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(h, m, w, l) }
}

struct Win(Hwnd);
impl Drop for Win {
    fn drop(&mut self) {
        let _ = unsafe { DestroyWindow(to_hwnd(self.0)) };
    }
}

fn make(x: i32, y: i32) -> Win {
    let inst = unsafe { GetModuleHandleW(None) }.unwrap();
    let h = unsafe {
        CreateWindowExW(
            WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            w!("PinItZorderTest"),
            w!("pin-it test"),
            WS_OVERLAPPEDWINDOW,
            x,
            y,
            160,
            120,
            None,
            None,
            Some(inst.into()),
            None,
        )
    }
    .unwrap();
    let _ = unsafe { ShowWindow(h, SW_SHOWNA) };
    Win(h.0 as isize)
}

fn pump(ms: u64) {
    let end = std::time::Instant::now() + std::time::Duration::from_millis(ms);
    while std::time::Instant::now() < end {
        let mut msg = Default::default();
        while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// Names of the test windows in snapshot order, top first.
fn show(names: &[(&str, Hwnd)]) -> String {
    let snap = z_order_snapshot();
    let mut v: Vec<(usize, &str)> = names
        .iter()
        .filter_map(|(n, h)| snap.iter().position(|s| s == h).map(|p| (p, *n)))
        .collect();
    v.sort();
    v.iter().map(|(_, n)| *n).collect::<Vec<_>>().join(" > ")
}

#[test]
#[ignore]
fn z_order() {
    let _guard = crate::window_test_guard();
    let cls = WNDCLASSW {
        lpfnWndProc: Some(proc),
        lpszClassName: w!("PinItZorderTest"),
        hInstance: unsafe { GetModuleHandleW(None) }.unwrap().into(),
        ..Default::default()
    };
    let _ = unsafe {
        windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        )
    };
    assert_ne!(unsafe { RegisterClassW(&cls) }, 0);
    let (sw, sh) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    let _ = CW_USEDEFAULT;
    let base_x = sw - 260;
    let base_y = sh - 200;
    let (a, b, c, d) = (
        make(base_x, base_y),
        make(base_x + 20, base_y + 10),
        make(base_x + 40, base_y + 20),
        make(base_x + 60, base_y + 30),
    );
    let names = [("A", a.0), ("B", b.0), ("C", c.0), ("D", d.0)];
    pump(200);
    println!("created A,B,C,D        : {}", show(&names));

    pin(a.0).unwrap();
    pin(b.0).unwrap();
    pin(c.0).unwrap();
    println!("pinned A,B,C           : {}", show(&names));
    assert_eq!(show(&names), "C > B > A > D");
    println!(
        "topmost A,B,C,D        : {} {} {} {}",
        is_topmost(a.0),
        is_topmost(b.0),
        is_topmost(c.0),
        is_topmost(d.0)
    );
    assert!(is_topmost(a.0) && is_topmost(b.0) && is_topmost(c.0) && !is_topmost(d.0));

    let order = [a.0, b.0, c.0];
    assert!(is_in_order(&z_order_snapshot(), &order));
    assert_eq!(apply_order(&order), super::order::OrderOutcome::default());

    unsafe { SetWindowPos(to_hwnd(a.0), Some(HWND_TOP), 0, 0, 0, 0, NO_FOCUS_FLAGS) }.unwrap();
    println!("raised A (HWND_TOP)    : {}", show(&names));
    let in_order = is_in_order(&z_order_snapshot(), &order);
    println!("is_in_order            : {in_order}");
    assert!(!in_order);

    let fails = apply_order(&order);
    println!("apply_order failures   : {fails:?}");
    println!("after apply_order      : {}", show(&names));
    assert!(fails.failed.is_empty());
    assert_eq!(show(&names), "C > B > A > D");
    println!(
        "is_in_order            : {}",
        is_in_order(&z_order_snapshot(), &order)
    );

    unpin(b.0).unwrap();
    println!("unpinned B             : {}", show(&names));
    println!(
        "topmost A,B,C,D        : {} {} {} {}",
        is_topmost(a.0),
        is_topmost(b.0),
        is_topmost(c.0),
        is_topmost(d.0)
    );
    let s = show(&names);
    assert!(s.find('C') < s.find('A') && s.find('A') < s.find('D'));
    assert!(!is_topmost(b.0));

    drop((a, b, c, d));
    let _ = unsafe { UnregisterClassW(w!("PinItZorderTest"), None) };
}

/// A window created hidden and shown with SW_SHOWNA sits at the bottom of the z-order; the first
/// HWND_TOPMOST for it is then ignored. Reports how often `pin` copes with it.
#[test]
#[ignore]
fn pin_of_window_shown_without_activation() {
    let _guard = crate::window_test_guard();
    let cls = WNDCLASSW {
        lpfnWndProc: Some(proc),
        lpszClassName: w!("PinItZorderHidden"),
        hInstance: unsafe { GetModuleHandleW(None) }.unwrap().into(),
        ..Default::default()
    };
    assert_ne!(unsafe { RegisterClassW(&cls) }, 0);
    let (sw, sh) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    let (mut pinned, mut refused) = (0, 0);
    for _ in 0..TRIALS {
        let inst = unsafe { GetModuleHandleW(None) }.unwrap();
        let h = unsafe {
            CreateWindowExW(
                Default::default(),
                w!("PinItZorderHidden"),
                w!("pin-it hidden"),
                WS_OVERLAPPEDWINDOW,
                sw - 200,
                sh - 140,
                160,
                100,
                None,
                None,
                Some(inst.into()),
                None,
            )
        }
        .unwrap();
        let win = Win(h.0 as isize);
        let _ = unsafe { ShowWindow(h, SW_SHOWNA) };
        pump(30);
        match pin(win.0) {
            Ok(()) if is_topmost(win.0) => pinned += 1,
            _ => refused += 1,
        }
        let _ = unpin(win.0);
    }
    println!("pin of hidden-then-shown window: pinned {pinned}, refused {refused} of {TRIALS}");
    let _ = unsafe { UnregisterClassW(w!("PinItZorderHidden"), None) };
    assert_eq!(refused, 0);
}

const TRIALS: usize = 20;

/// The same with targets in another process, as in real use: WinForms windows that are created
/// hidden and shown without activation (10 windows in each of 2 helper processes).
#[test]
#[ignore]
fn pin_of_foreign_windows_shown_without_activation() {
    use windows::Win32::UI::WindowsAndMessaging::FindWindowW;
    const SCRIPT: &str = r#"
Add-Type -ReferencedAssemblies System.Windows.Forms,System.Drawing -TypeDefinition @"
using System; using System.Windows.Forms; using System.Drawing;
public class NoActForm : Form {
  protected override bool ShowWithoutActivation { get { return true; } }
}
"@
$wa = [System.Windows.Forms.Screen]::PrimaryScreen.WorkingArea
$forms = @()
for ($i = 0; $i -lt 10; $i++) {
  $f = New-Object NoActForm
  $f.Text = "PinIt-Fgn-$($args[0])-$i"; $f.StartPosition = 'Manual'; $f.ShowInTaskbar = $false
  $f.Size = New-Object System.Drawing.Size(160,80)
  $f.Location = New-Object System.Drawing.Point(($wa.Right - 170), ($wa.Bottom - 90))
  $f.Show(); $forms += $f
}
$end = (Get-Date).AddSeconds(25)
while ((Get-Date) -lt $end) { [System.Windows.Forms.Application]::DoEvents(); Start-Sleep -Milliseconds 20 }
"#;
    let _guard = crate::window_test_guard();
    let script = std::env::temp_dir().join("pin_it_foreign_helper.ps1");
    std::fs::write(&script, SCRIPT).unwrap();
    let mut children: Vec<_> = (0..2)
        .map(|p| {
            std::process::Command::new("powershell")
                .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
                .arg(&script)
                .arg(p.to_string())
                .spawn()
                .unwrap()
        })
        .collect();
    let find = |p: usize, i: usize| {
        let title = crate::win32::wide(&format!("PinIt-Fgn-{p}-{i}"));
        let h = unsafe { FindWindowW(None, windows::core::PCWSTR(title.as_ptr())) };
        h.map(|h| h.0 as isize).unwrap_or(0)
    };
    let start = std::time::Instant::now();
    while (0..2).any(|p| find(p, 9) == 0) && start.elapsed().as_secs() < 30 {
        pump(100);
    }
    let (mut pinned, mut refused) = (0, 0);
    for p in 0..2 {
        for i in 0..10 {
            let h = find(p, i);
            assert_ne!(h, 0, "helper window {p}-{i} missing");
            match pin(h) {
                Ok(()) if is_topmost(h) => pinned += 1,
                _ => refused += 1,
            }
            let _ = unpin(h);
        }
    }
    println!("pin of foreign hidden-then-shown windows: pinned {pinned}, refused {refused} of 20");
    for c in &mut children {
        let _ = c.kill();
    }
    assert_eq!(refused, 0);
}
