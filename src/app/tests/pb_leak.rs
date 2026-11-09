//! Opening and closing many decorated windows must not grow the process: handles, GDI and USER objects,
//! private bytes and the per-window state of the app. Own windows only.

use std::time::Duration;

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Threading::{
    GR_GDIOBJECTS, GR_USEROBJECTS, GetCurrentProcess, GetGuiResources, GetProcessHandleCount,
};

use super::pb_common::NativeWin;
use super::support::{options, pump_ms, temp_dir};
use crate::app::Runtime;
use crate::app::host::with_app;
use crate::config::Settings;
use crate::events::WinEvent;

/// How long a window lives before it is closed: long enough for the overlay and the first scan.
const WINDOW_LIFE_MS: u64 = 60;
/// Growth allowed per batch after the first (the first one fills the caches once).
const MAX_HANDLE_GROWTH: i64 = 10;
const MAX_GDI_GROWTH: i64 = 10;
const MAX_USER_GROWTH: i64 = 10;

/// How much to open and close, and how much growth of private bytes in the second batch is allocator noise.
struct Stress {
    windows_per_batch: usize,
    /// Pumping after a batch, so that the worker, the timers and the allocator can settle.
    settle_ms: u64,
    max_private_growth: i64,
}

/// The default run: about 15 s. The leak this guards against (the worker kept every scan) added 3.2 MB per 100 windows.
const QUICK: Stress = Stress {
    windows_per_batch: 60,
    settle_ms: 1000,
    max_private_growth: 600_000,
};
/// The measurement of the report: 200 windows per batch, 3 s of settling.
const LONG: Stress = Stress {
    windows_per_batch: 200,
    settle_ms: 3000,
    max_private_growth: 1_000_000,
};

#[repr(C)]
#[derive(Default)]
struct ProcessMemory {
    cb: u32,
    page_fault_count: u32,
    peak_working_set: usize,
    working_set: usize,
    quota_peak_paged: usize,
    quota_paged: usize,
    quota_peak_non_paged: usize,
    quota_non_paged: usize,
    pagefile: usize,
    peak_pagefile: usize,
    private_usage: usize,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn K32GetProcessMemoryInfo(process: HANDLE, counters: *mut ProcessMemory, cb: u32) -> i32;
}

#[derive(Debug, Clone, Copy)]
struct Usage {
    handles: i64,
    gdi: i64,
    user: i64,
    private_bytes: i64,
}

fn usage() -> Usage {
    // SAFETY: plain queries on the current process; the pseudo handle needs no close.
    unsafe {
        let process = GetCurrentProcess();
        let mut handles = 0u32;
        GetProcessHandleCount(process, &mut handles).unwrap();
        let mut memory = ProcessMemory {
            cb: size_of::<ProcessMemory>() as u32,
            ..Default::default()
        };
        assert_ne!(K32GetProcessMemoryInfo(process, &mut memory, memory.cb), 0);
        Usage {
            handles: i64::from(handles),
            gdi: i64::from(GetGuiResources(process, GR_GDIOBJECTS)),
            user: i64::from(GetGuiResources(process, GR_USEROBJECTS)),
            private_bytes: memory.private_usage as i64,
        }
    }
}

/// Opens, decorates and closes `count` windows the way the hooks would report them.
fn churn(count: usize, settle_ms: u64) {
    for i in 0..count {
        let w = NativeWin::new((i % 5) as i32, None);
        with_app(|a| a.only.get_or_insert_with(Vec::new).push(w.0));
        let h = w.0;
        with_app(|a| a.handle_event(WinEvent::Shown(h)));
        pump_ms(WINDOW_LIFE_MS);
        drop(w);
        with_app(|a| a.handle_event(WinEvent::Destroyed(h)));
        with_app(|a| a.only.get_or_insert_with(Vec::new).retain(|&x| x != h));
    }
    pump_ms(settle_ms);
}

/// What the app still holds per window.
fn tracked_windows() -> usize {
    with_app(|a| a.entries.len() + a.colors.tracked_windows()).unwrap()
}

fn growth(before: Usage, after: Usage) -> Usage {
    Usage {
        handles: after.handles - before.handles,
        gdi: after.gdi - before.gdi,
        user: after.user - before.user,
        private_bytes: after.private_bytes - before.private_bytes,
    }
}

/// The counters are process-wide, so another thread (a test that does not take the window guard, the allocator
/// returning pages) can add to one batch. A leak grows in every batch, a one-off only in one: the smaller
/// growth per counter of two batches is what a leak would at least have produced.
fn steadier(a: Usage, b: Usage) -> Usage {
    Usage {
        handles: a.handles.min(b.handles),
        gdi: a.gdi.min(b.gdi),
        user: a.user.min(b.user),
        private_bytes: a.private_bytes.min(b.private_bytes),
    }
}

fn assert_no_growth(plan: Stress) {
    let _g = crate::window_test_guard();
    let dir = temp_dir("pb-leak");
    let rt = Runtime::start(&options(&dir, Vec::new(), &Settings::default())).unwrap();
    let start = usage();
    churn(plan.windows_per_batch, plan.settle_ms);
    let first = usage();
    churn(plan.windows_per_batch, plan.settle_ms);
    let second = usage();
    churn(plan.windows_per_batch, plan.settle_ms);
    let third = usage();
    let per_100 = |g: Usage| {
        let f = |v: i64| v * 100 / plan.windows_per_batch as i64;
        (f(g.handles), f(g.gdi), f(g.user), f(g.private_bytes))
    };
    eprintln!("start {start:?}");
    eprintln!(
        "batch 1 {first:?} per 100 windows (handles, gdi, user, bytes): {:?}",
        per_100(growth(start, first))
    );
    eprintln!(
        "batch 2 {second:?} per 100 windows (handles, gdi, user, bytes): {:?}",
        per_100(growth(first, second))
    );
    eprintln!(
        "batch 3 {third:?} per 100 windows (handles, gdi, user, bytes): {:?}",
        per_100(growth(second, third))
    );
    let tracked = tracked_windows();
    drop(rt);
    let _ = std::fs::remove_dir_all(dir);
    std::thread::sleep(Duration::from_millis(50));

    let grown = steadier(growth(first, second), growth(second, third));
    assert_eq!(
        tracked, 0,
        "per-window state left after every window closed"
    );
    assert!(grown.handles < MAX_HANDLE_GROWTH, "handles {grown:?}");
    assert!(grown.gdi < MAX_GDI_GROWTH, "gdi objects {grown:?}");
    assert!(grown.user < MAX_USER_GROWTH, "user objects {grown:?}");
    assert!(
        grown.private_bytes < plan.max_private_growth,
        "private bytes {grown:?}"
    );
}

#[test]
fn opening_and_closing_windows_does_not_grow_the_process() {
    assert_no_growth(QUICK);
}

#[test]
#[ignore = "takes a minute: 400 windows; run with --ignored --nocapture for the numbers"]
fn opening_and_closing_many_windows_does_not_grow_the_process() {
    assert_no_growth(LONG);
}
