//! Hung-target behavior through `pin`, `unpin` and `apply_order`.

use super::guard::is_busy;
use super::*;

#[test]
fn hung_check_on_missing_and_responsive_windows() {
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_POPUP,
    };
    use windows::core::w;
    let _guard = crate::window_test_guard();
    assert!(!is_hung(0x7FFF_FFF0));
    assert!(!is_busy(0x7FFF_FFF0));
    let win = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("STATIC"),
            w!(""),
            WS_POPUP,
            0,
            0,
            10,
            10,
            None,
            None,
            None,
            None,
        )
        .unwrap()
    };
    let h = win.0 as isize;
    assert!(!is_hung(h) && !is_busy(h));
    unsafe { DestroyWindow(win).unwrap() };
}

/// Own PowerShell WinForms window whose UI thread sleeps; pin must return Hung instead of blocking.
/// Run with `cargo test hung_target -- --ignored --nocapture`.
#[test]
#[ignore]
fn hung_target_returns_hung_without_blocking() {
    use std::time::{Duration, Instant};
    use windows::Win32::UI::WindowsAndMessaging::FindWindowW;
    use windows::core::w;
    const SCRIPT: &str = r#"
Add-Type -ReferencedAssemblies System.Windows.Forms,System.Drawing -TypeDefinition @"
using System; using System.Windows.Forms; using System.Drawing; using System.Threading;
public class NoActFormT : Form {
  protected override bool ShowWithoutActivation { get { return true; } }
  protected override CreateParams CreateParams { get { var p = base.CreateParams; p.ExStyle |= 0x08000000; return p; } }
  System.Windows.Forms.Timer T;
  public NoActFormT() {
Text = "PinIt-HangTest"; StartPosition = FormStartPosition.Manual; ShowInTaskbar = false;
var wa = Screen.PrimaryScreen.WorkingArea; Size = new Size(200,100);
Location = new Point(wa.Right - 210, wa.Bottom - 110);
T = new System.Windows.Forms.Timer(); T.Interval = 2000;
T.Tick += (s,e)=>{ T.Stop(); Thread.Sleep(14000); Close(); }; T.Start();
  }
}
"@
[System.Windows.Forms.Application]::Run((New-Object NoActFormT))
"#;
    let script = std::env::temp_dir().join("pin_it_hang_helper.ps1");
    std::fs::write(&script, SCRIPT).unwrap();
    let mut child = std::process::Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&script)
        .spawn()
        .unwrap();
    let result = std::panic::catch_unwind(|| {
        let start = Instant::now();
        let h = loop {
            let h = unsafe { FindWindowW(None, w!("PinIt-HangTest")) }.unwrap_or_default();
            if !h.is_invalid() {
                break h.0 as isize;
            }
            assert!(
                start.elapsed() < Duration::from_secs(30),
                "helper window missing"
            );
            std::thread::sleep(Duration::from_millis(100));
        };
        std::thread::sleep(Duration::from_millis(2600)); // the form is now in its sleep
        let t = Instant::now();
        let r = pin(h);
        let busy_call = t.elapsed();
        println!("pin while busy (not yet hung): {r:?} after {busy_call:?}");
        assert_eq!(r, Err(ApplyError::Hung));
        assert!(busy_call < Duration::from_millis(1500));
        let hung_since = Instant::now();
        while !is_hung(h) {
            assert!(hung_since.elapsed() < Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(100));
        }
        println!(
            "IsHungAppWindow true {:?} after the busy probe",
            hung_since.elapsed()
        );
        let t = Instant::now();
        assert_eq!(pin(h), Err(ApplyError::Hung));
        assert_eq!(
            unpin(h),
            Ok(()),
            "unpin is queued, it does not wait for the target"
        );
        assert_eq!(apply_order(&[h]).failed, vec![(h, ApplyError::Hung)]);
        let hung_calls = t.elapsed();
        println!("3 calls while hung took {hung_calls:?}");
        assert!(hung_calls < Duration::from_millis(500));
        assert!(!is_topmost(h));
    });
    let _ = child.kill();
    let _ = child.wait();
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}

/// Own PowerShell WinForms window that is topmost, then busy for 10 s and then answers again:
/// `unpin` must return at once, also while the window counts as hung, and the window loses the
/// topmost flag after it resumes. Run with `cargo test busy_target_unpin -- --ignored --nocapture`.
#[test]
#[ignore]
fn busy_target_unpin_is_queued_and_applied_after_resume() {
    use std::time::{Duration, Instant};
    use windows::Win32::UI::WindowsAndMessaging::FindWindowW;
    use windows::core::w;
    const SCRIPT: &str = r#"
Add-Type -ReferencedAssemblies System.Windows.Forms,System.Drawing -TypeDefinition @"
using System; using System.Windows.Forms; using System.Drawing; using System.Threading;
public class NoActFormU : Form {
  protected override bool ShowWithoutActivation { get { return true; } }
  protected override CreateParams CreateParams { get { var p = base.CreateParams; p.ExStyle |= 0x08000000; return p; } }
  System.Windows.Forms.Timer T;
  public NoActFormU() {
Text = "PinIt-BusyUnpinTest"; StartPosition = FormStartPosition.Manual; ShowInTaskbar = false;
var wa = Screen.PrimaryScreen.WorkingArea; Size = new Size(200,100);
Location = new Point(wa.Right - 210, wa.Bottom - 110);
T = new System.Windows.Forms.Timer(); T.Interval = 2000;
T.Tick += (s,e)=>{ T.Stop(); Thread.Sleep(10000); }; T.Start();
  }
}
"@
[System.Windows.Forms.Application]::Run((New-Object NoActFormU))
"#;
    let script = std::env::temp_dir().join("pin_it_busy_unpin_helper.ps1");
    std::fs::write(&script, SCRIPT).unwrap();
    let mut child = std::process::Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&script)
        .spawn()
        .unwrap();
    println!("helper pid {}", child.id());
    let result = std::panic::catch_unwind(|| {
        let start = Instant::now();
        let h = loop {
            let h = unsafe { FindWindowW(None, w!("PinIt-BusyUnpinTest")) }.unwrap_or_default();
            if !h.is_invalid() {
                break h.0 as isize;
            }
            assert!(
                start.elapsed() < Duration::from_secs(30),
                "helper window missing"
            );
            std::thread::sleep(Duration::from_millis(100));
        };
        assert_eq!(pin(h), Ok(()));
        assert!(is_topmost(h), "helper not topmost before it got busy");
        std::thread::sleep(Duration::from_millis(2600)); // the form is now in its sleep
        let hung_since = Instant::now();
        while !is_hung(h) {
            assert!(hung_since.elapsed() < Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(100));
        }
        let t = Instant::now();
        let r = unpin(h);
        let call = t.elapsed();
        println!(
            "unpin while hung: {r:?} after {call:?}; topmost right after: {}",
            is_topmost(h)
        );
        assert_eq!(r, Ok(()));
        assert!(call < Duration::from_millis(100));
        assert!(is_topmost(h), "flag changed before the target resumed");
        let queued = Instant::now();
        while is_topmost(h) {
            assert!(
                queued.elapsed() < Duration::from_secs(15),
                "flag never removed"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        println!(
            "topmost flag gone {:?} after the unpin call",
            queued.elapsed()
        );
    });
    let _ = child.kill();
    let _ = child.wait();
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}

#[test]
fn activate_answers_with_a_bool_for_own_and_missing_windows() {
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
        WS_POPUP,
    };
    use windows::core::w;
    let _guard = crate::window_test_guard();
    let _real = activate_hooks::RealActivation::enable();
    let before = activate_hooks::calls();
    assert!(
        !activate(0x7FFF_FFF0),
        "a missing window cannot be activated"
    );
    let win = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0),
            w!("STATIC"),
            w!(""),
            WS_POPUP,
            0,
            0,
            10,
            10,
            None,
            None,
            None,
            None,
        )
        .unwrap()
    };
    let answer: bool = activate(win.0 as isize);
    println!("activate on an own no-activate window: {answer}");
    assert_eq!(activate_hooks::calls(), before + 2);
    unsafe { DestroyWindow(win).unwrap() };
}
