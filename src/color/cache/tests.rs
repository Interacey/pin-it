//! Cache and worker behavior, with injected samplers.

use super::*;
use crate::color::WM_COLOR_READY;
use std::sync::atomic::{AtomicUsize, Ordering};

static CALLS: AtomicUsize = AtomicUsize::new(0);
fn counting_ok(_: Hwnd) -> Option<[u8; 3]> {
    CALLS.fetch_add(1, Ordering::SeqCst);
    Some([1, 2, 3])
}
fn failing(_: Hwnd) -> Option<[u8; 3]> {
    None
}
fn other(_: Hwnd) -> Option<[u8; 3]> {
    Some([9, 9, 9])
}

#[test]
fn cache_ttl_and_forget() {
    let mut c = ColorCache::new();
    let t0 = Instant::now();
    CALLS.store(0, Ordering::SeqCst);
    assert_eq!(c.get_with(counting_ok, 1, t0), [1, 2, 3]);
    let t = t0 + Duration::from_secs(29);
    assert_eq!(c.get_with(counting_ok, 1, t), [1, 2, 3]);
    assert_eq!(CALLS.load(Ordering::SeqCst), 1);
    c.get_with(counting_ok, 1, t0 + Duration::from_secs(31));
    assert_eq!(CALLS.load(Ordering::SeqCst), 2);
    c.forget(1);
    c.get_with(counting_ok, 1, t0 + Duration::from_secs(32));
    assert_eq!(CALLS.load(Ordering::SeqCst), 3);
    c.set_ttl(Duration::from_secs(1));
    c.forget(1);
    c.get_with(counting_ok, 1, t0);
    c.get_with(counting_ok, 1, t0 + Duration::from_secs(2));
    assert_eq!(CALLS.load(Ordering::SeqCst), 5);
}

#[test]
fn cache_keeps_last_good_on_failure() {
    let mut c = ColorCache::new();
    let t0 = Instant::now();
    c.get_with(other, 7, t0);
    let t1 = t0 + Duration::from_secs(40);
    assert_eq!(c.get_with(failing, 7, t1), [9, 9, 9]);
}

#[test]
fn cache_failure_without_value_gives_accent() {
    let mut c = ColorCache::new();
    assert_eq!(c.get_with(failing, 8, Instant::now()), system_accent());
}

#[test]
fn cache_keeps_live_entries_over_the_cap() {
    static CAP_CALLS: AtomicUsize = AtomicUsize::new(0);
    fn sampler(_: Hwnd) -> Option<[u8; 3]> {
        CAP_CALLS.fetch_add(1, Ordering::SeqCst);
        Some([5, 5, 5])
    }
    let ms = Duration::from_millis;
    let mut c = ColorCache::new();
    let t0 = Instant::now();
    for i in 0..300 {
        c.get_with(sampler, i, t0 + ms(i as u64));
    }
    // every entry was asked for within 5 s: nothing is evicted
    assert_eq!(c.map.len(), 300);
    for i in 0..300 {
        c.get_with(sampler, i, t0 + ms(1000 + i as u64));
    }
    assert_eq!(
        CAP_CALLS.load(Ordering::SeqCst),
        300,
        "second sweep resampled"
    );

    // 20 s later only entry 0 was asked for recently; a new window evicts oldest expiry first
    c.get_with(sampler, 0, t0 + Duration::from_secs(19));
    c.get_with(sampler, 1000, t0 + Duration::from_secs(20));
    assert_eq!(c.map.len(), 256);
    assert!(c.map.contains_key(&0), "recently used entry was evicted");
    assert!(!c.map.contains_key(&1));
    assert!(!c.map.contains_key(&45));
    assert!(c.map.contains_key(&46));
    assert!(c.map.contains_key(&299) && c.map.contains_key(&1000));
}

#[test]
fn cache_forget_removes_entry() {
    let mut c = ColorCache::new();
    c.get_with(other, 299, Instant::now());
    c.forget(299);
    assert!(!c.map.contains_key(&299));
}

/// Blocks a sampler until opened, so a test can show that `get` does not wait for it.
struct Gate(std::sync::Mutex<bool>, std::sync::Condvar);
impl Gate {
    const fn new() -> Self {
        Gate(std::sync::Mutex::new(false), std::sync::Condvar::new())
    }
    fn wait(&self) {
        let mut open = self.0.lock().unwrap();
        while !*open {
            open = self.1.wait(open).unwrap();
        }
    }
    fn open(&self) {
        *self.0.lock().unwrap() = true;
        self.1.notify_all();
    }
}

fn scan_of(width: i32) -> Arc<TitleBarScan> {
    Arc::new(TitleBarScan {
        window_size: (width, 40),
        caption_px: 40,
        background: [9, 9, 9],
        caption_ink: None,
        accent: None,
        rows: vec![vec![[9, 9, 9]; width as usize]; 3],
    })
}

fn scan_sampler(h: Hwnd) -> Option<TitleSample> {
    Some(TitleSample {
        color: None,
        scan: Some(scan_of(h as i32)),
    })
}

#[test]
fn scan_for_is_pending_then_ready_and_cached() {
    let mut c = ColorCache::with_title_sampler(scan_sampler);
    let t0 = Instant::now();
    assert_eq!(c.scan_for(500, t0), ScanLookup::Pending);
    eventually("scan arrives", || {
        std::thread::sleep(Duration::from_millis(10));
        matches!(c.scan_for(500, t0), ScanLookup::Ready(_))
    });
    let ScanLookup::Ready(scan) = c.scan_for(500, t0) else {
        panic!("no scan");
    };
    assert_eq!(scan.window_size, (500, 40));
}

#[test]
fn failed_capture_without_scan_is_unavailable_and_keeps_an_old_scan() {
    fn fails(_: Hwnd) -> Option<TitleSample> {
        None
    }
    let mut c = ColorCache::with_title_sampler(fails);
    let t0 = Instant::now();
    c.scan_for(1, t0);
    eventually("failure stored", || {
        std::thread::sleep(Duration::from_millis(10));
        c.scan_for(1, t0) == ScanLookup::Unavailable
    });
    // a good scan, then a failed refresh: the good scan stays
    c.store(2, scan_sampler(300), t0);
    c.store(2, None, t0);
    assert!(matches!(c.scan_for(2, t0), ScanLookup::Ready(_)));
}

#[test]
fn refresh_is_limited_to_one_per_two_seconds() {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    fn counting(h: Hwnd) -> Option<TitleSample> {
        CALLS.fetch_add(1, Ordering::SeqCst);
        scan_sampler(h)
    }
    let mut c = ColorCache::with_title_sampler(counting);
    let t0 = Instant::now();
    c.scan_for(400, t0);
    eventually("first sample", || {
        std::thread::sleep(Duration::from_millis(10));
        matches!(c.scan_for(400, t0), ScanLookup::Ready(_))
    });
    assert_eq!(CALLS.load(Ordering::SeqCst), 1);
    c.refresh_if_older(400, t0 + Duration::from_millis(500), Duration::from_secs(2));
    std::thread::sleep(Duration::from_millis(100));
    c.poll(t0 + Duration::from_millis(500));
    assert_eq!(CALLS.load(Ordering::SeqCst), 1, "refresh within 2 s");
    c.refresh_if_older(400, t0 + Duration::from_secs(3), Duration::from_secs(2));
    eventually("second sample", || {
        std::thread::sleep(Duration::from_millis(10));
        CALLS.load(Ordering::SeqCst) == 2
    });
}

#[test]
fn a_changed_scan_posts_but_an_identical_one_does_not() {
    let a = TitleSample {
        color: Some([1, 1, 1]),
        scan: Some(scan_of(100)),
    };
    let same = a.clone();
    let other_scan = TitleSample {
        color: Some([1, 1, 1]),
        scan: Some(scan_of(101)),
    };
    assert_eq!(a, same);
    assert_ne!(a, other_scan);
}

#[test]
fn the_caption_ink_refreshes_with_the_scan() {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    fn dimming(h: Hwnd) -> Option<TitleSample> {
        let ink = if CALLS.fetch_add(1, Ordering::SeqCst) == 0 {
            [0x55; 3]
        } else {
            [0x33; 3]
        };
        let scan = TitleBarScan {
            caption_ink: Some(ink),
            accent: None,
            ..(*scan_of(h as i32)).clone()
        };
        Some(TitleSample {
            color: None,
            scan: Some(Arc::new(scan)),
        })
    }
    let ink_of = |c: &mut ColorCache, t: Instant| match c.scan_for(600, t) {
        ScanLookup::Ready(scan) => scan.caption_ink,
        _ => None,
    };
    let mut c = ColorCache::with_title_sampler(dimming);
    let t0 = Instant::now();
    c.scan_for(600, t0);
    eventually("first ink", || ink_of(&mut c, t0) == Some([0x55; 3]));
    c.refresh_if_older(600, t0 + Duration::from_secs(3), Duration::from_secs(2));
    eventually("refreshed ink", || {
        ink_of(&mut c, t0 + Duration::from_secs(3)) == Some([0x33; 3])
    });
}

#[test]
fn a_changed_ink_alone_counts_as_a_changed_sample() {
    let a = TitleSample {
        color: None,
        scan: Some(scan_of(100)),
    };
    let dimmer = TitleBarScan {
        caption_ink: Some([0x33; 3]),
        accent: None,
        ..(*scan_of(100)).clone()
    };
    let b = TitleSample {
        color: None,
        scan: Some(Arc::new(dimmer)),
    };
    assert_ne!(a, b);
}

fn eventually(what: &str, mut f: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(5);
    while !f() {
        assert!(Instant::now() < end, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn get_does_not_block_on_a_slow_sampler() {
    static GATE: Gate = Gate::new();
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    fn slow(_: Hwnd) -> Option<[u8; 3]> {
        CALLS.fetch_add(1, Ordering::SeqCst);
        GATE.wait();
        Some([4, 5, 6])
    }
    let mut c = ColorCache::with_sampler(slow);
    let t0 = Instant::now();
    let started = Instant::now();
    assert_eq!(c.get(1, t0), system_accent());
    for _ in 0..5 {
        assert_eq!(c.get(1, t0), system_accent());
    }
    assert!(started.elapsed() < Duration::from_millis(500));
    GATE.open();
    eventually("result applied", || c.get(1, t0) == [4, 5, 6]);
    assert_eq!(
        CALLS.load(Ordering::SeqCst),
        1,
        "requests for one window not deduped"
    );
}

#[test]
fn queue_is_capped_at_64() {
    static GATE: Gate = Gate::new();
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    fn slow(_: Hwnd) -> Option<[u8; 3]> {
        CALLS.fetch_add(1, Ordering::SeqCst);
        GATE.wait();
        Some([7, 7, 7])
    }
    let mut c = ColorCache::with_sampler(slow);
    let t0 = Instant::now();
    for i in 0..100 {
        c.get(i, t0);
    }
    assert_eq!(c.in_flight.len(), 64);
    GATE.open();
    // the other 36 are requested by later gets
    eventually("all cached", || (0..100).all(|i| c.get(i, t0) == [7, 7, 7]));
    assert_eq!(CALLS.load(Ordering::SeqCst), 100);
}

#[test]
fn failed_sample_keeps_last_good_color() {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    fn flaky(_: Hwnd) -> Option<[u8; 3]> {
        (CALLS.fetch_add(1, Ordering::SeqCst) == 0).then_some([8, 8, 8])
    }
    let mut c = ColorCache::with_sampler(flaky);
    let t0 = Instant::now();
    c.get(1, t0);
    eventually("first sample", || c.get(1, t0) == [8, 8, 8]);
    let t1 = t0 + Duration::from_secs(31);
    assert_eq!(c.get(1, t1), [8, 8, 8]); // expired: old value while the failing resample runs
    eventually("failed resample done", || {
        c.get(1, t1);
        CALLS.load(Ordering::SeqCst) >= 2 && c.in_flight.is_empty()
    });
    assert_eq!(c.get(1, t1), [8, 8, 8]);
}

fn quick(h: Hwnd) -> Option<[u8; 3]> {
    Some([h as u8, 1, 2])
}

#[test]
fn worker_exits_when_cache_is_dropped() {
    let mut c = ColorCache::with_sampler(quick);
    c.get(1, Instant::now());
    let shared = c.notify.clone();
    eventually("worker running", || Arc::strong_count(&shared) == 3);
    drop(c);
    eventually("worker gone", || Arc::strong_count(&shared) == 1);
}

#[test]
fn notify_posts_once_until_polled() {
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, PM_REMOVE, PeekMessageW, WINDOW_EX_STYLE, WS_POPUP,
    };
    use windows::core::w;
    let _guard = crate::window_test_guard();
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
    let count = || {
        let mut n = 0;
        let mut msg = Default::default();
        while unsafe {
            PeekMessageW(
                &mut msg,
                Some(win),
                WM_COLOR_READY,
                WM_COLOR_READY,
                PM_REMOVE,
            )
        }
        .as_bool()
        {
            n += 1;
        }
        n
    };
    let mut c = ColorCache::with_sampler(quick);
    c.set_notify(win.0 as isize, WM_COLOR_READY);
    let t0 = Instant::now();
    for i in 1..=5 {
        c.request(i);
    }
    eventually("first post", || c.notify.is_pending());
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(count(), 1, "more than one post waiting");
    c.poll(t0);
    assert_eq!(c.get(3, t0), [3, 1, 2]);
    // a new window changes a color after the poll cleared the flag: one more post
    c.request(6);
    eventually("second post", || {
        std::thread::sleep(Duration::from_millis(20));
        c.notify.is_pending()
    });
    assert_eq!(count(), 1);
    // an unchanged color does not post
    c.poll(t0);
    c.request(6);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(count(), 0, "unchanged color posted");
    // a forgotten window starts over: the worker no longer remembers its last sample
    c.poll(t0);
    c.forget(6);
    c.request(6);
    eventually("post after forget", || {
        std::thread::sleep(Duration::from_millis(20));
        c.notify.is_pending()
    });
    assert_eq!(count(), 1);
    unsafe { DestroyWindow(win).unwrap() };
}

#[test]
fn refresh_after_a_resize_ignores_the_two_second_limit() {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    fn counting(h: Hwnd) -> Option<TitleSample> {
        CALLS.fetch_add(1, Ordering::SeqCst);
        scan_sampler(h)
    }
    let mut c = ColorCache::with_title_sampler(counting);
    let t0 = Instant::now();
    c.scan_for(410, t0);
    eventually("first sample", || {
        std::thread::sleep(Duration::from_millis(10));
        matches!(c.scan_for(410, t0), ScanLookup::Ready(_))
    });
    assert_eq!(CALLS.load(Ordering::SeqCst), 1);
    let soon = t0 + Duration::from_millis(100);
    c.refresh_resized(410, soon);
    eventually("sample right after the first", || {
        std::thread::sleep(Duration::from_millis(10));
        CALLS.load(Ordering::SeqCst) == 2
    });
}

#[test]
fn refresh_after_a_resize_does_not_queue_twice() {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    static GATE: Gate = Gate::new();
    fn gated(h: Hwnd) -> Option<TitleSample> {
        CALLS.fetch_add(1, Ordering::SeqCst);
        GATE.wait();
        scan_sampler(h)
    }
    let mut c = ColorCache::with_title_sampler(gated);
    let t0 = Instant::now();
    c.scan_for(411, t0);
    c.store(411, scan_sampler(411), t0);
    for _ in 0..5 {
        c.refresh_resized(411, t0);
    }
    GATE.open();
    eventually("one sample", || {
        std::thread::sleep(Duration::from_millis(10));
        c.poll(t0);
        !c.in_flight.contains(&411)
    });
    assert_eq!(CALLS.load(Ordering::SeqCst), 1);
}

#[test]
fn refresh_after_a_resize_of_an_unknown_window_does_nothing() {
    let mut c = ColorCache::with_title_sampler(scan_sampler);
    c.refresh_resized(412, Instant::now());
    assert!(c.worker.is_none());
}

#[test]
fn an_unresponsive_window_does_not_block_the_queue() {
    const HUNG: Hwnd = 1;
    const FINE: Hwnd = 2;
    static BLOCKED: AtomicUsize = AtomicUsize::new(0);
    fn sample(h: Hwnd) -> Option<TitleSample> {
        if h == HUNG {
            // what PrintWindow does on a stuck app
            BLOCKED.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(Duration::from_secs(30));
        }
        scan_sampler(h)
    }
    let mut c = ColorCache::with_title_sampler(skip_unresponsive(|h| h != HUNG, sample));
    let t0 = Instant::now();
    c.scan_for(HUNG, t0);
    c.scan_for(FINE, t0);
    eventually("the second window is sampled", || {
        std::thread::sleep(Duration::from_millis(10));
        matches!(c.scan_for(FINE, t0), ScanLookup::Ready(_))
    });
    assert_eq!(
        BLOCKED.load(Ordering::SeqCst),
        0,
        "hung window never captured"
    );
    assert_eq!(c.scan_for(HUNG, t0), ScanLookup::Unavailable);
}

#[test]
fn a_skipped_window_is_retried_after_the_failure_delay() {
    static ANSWERS: AtomicUsize = AtomicUsize::new(0);
    fn responsive_second_time(_: Hwnd) -> bool {
        ANSWERS.fetch_add(1, Ordering::SeqCst) >= 1
    }
    let mut c =
        ColorCache::with_title_sampler(skip_unresponsive(responsive_second_time, scan_sampler));
    let t0 = Instant::now();
    c.scan_for(420, t0);
    eventually("first (skipped) result", || {
        std::thread::sleep(Duration::from_millis(10));
        c.scan_for(420, t0) == ScanLookup::Unavailable
    });
    let later = t0 + RETRY_AFTER_FAILURE + Duration::from_millis(1);
    c.scan_for(420, later);
    eventually("retry", || {
        std::thread::sleep(Duration::from_millis(10));
        matches!(c.scan_for(420, later), ScanLookup::Ready(_))
    });
}

// ----- suspension, resize rescans, invalidation, accent -----

/// Takes the finished samples until the cache holds an entry for `hwnd`.
fn stored(c: &mut ColorCache, hwnd: Hwnd, now: Instant) {
    eventually("sample stored", || {
        c.poll(now);
        c.map.contains_key(&hwnd)
    });
}

/// Waits until the sampler has run `n` times in total.
fn eventually_calls(calls: &AtomicUsize, n: usize) {
    eventually("sampler calls", || calls.load(Ordering::SeqCst) >= n);
}

#[test]
fn a_suspended_window_asks_for_no_samples_until_it_resumes() {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    fn sampler(h: Hwnd) -> Option<TitleSample> {
        CALLS.fetch_add(1, Ordering::SeqCst);
        scan_sampler(h)
    }
    let mut c = ColorCache::with_title_sampler(sampler);
    let t0 = Instant::now();
    c.scan_for(610, t0);
    eventually_calls(&CALLS, 1);
    stored(&mut c, 610, t0);
    c.suspend(610, t0);
    let later = t0 + Duration::from_secs(40);
    c.scan_for(610, later);
    c.refresh_if_older(610, later, Duration::from_secs(2));
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(CALLS.load(Ordering::SeqCst), 1, "no sample while suspended");
    assert!(
        matches!(c.scan_for(610, later), ScanLookup::Ready(_)),
        "the last scan is still served"
    );
    c.resume(610, later);
    eventually_calls(&CALLS, 2);
}

#[test]
fn a_size_change_bypasses_the_suspension_but_not_ten_requests_per_second() {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    fn sampler(h: Hwnd) -> Option<TitleSample> {
        CALLS.fetch_add(1, Ordering::SeqCst);
        scan_sampler(h)
    }
    let mut c = ColorCache::with_title_sampler(sampler);
    let t0 = Instant::now();
    c.scan_for(620, t0);
    eventually_calls(&CALLS, 1);
    stored(&mut c, 620, t0);
    c.suspend(620, t0);
    let ms = Duration::from_millis;
    c.refresh_resized(620, t0 + ms(10));
    eventually_calls(&CALLS, 2);
    c.poll(t0 + ms(20));
    // 50 requests within 90 ms: only the first one counts.
    for i in 1..=50 {
        c.refresh_resized(620, t0 + ms(10 + i * 80 / 50));
    }
    std::thread::sleep(ms(100));
    assert_eq!(CALLS.load(Ordering::SeqCst), 2, "rate limited");
    c.refresh_resized(620, t0 + ms(10 + 100));
    eventually_calls(&CALLS, 3);
}

#[test]
fn invalidate_all_samples_every_known_window_again() {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    fn sampler(h: Hwnd) -> Option<TitleSample> {
        CALLS.fetch_add(1, Ordering::SeqCst);
        scan_sampler(h)
    }
    let mut c = ColorCache::with_title_sampler(sampler);
    let t0 = Instant::now();
    for h in [630, 631, 632] {
        c.scan_for(h, t0);
    }
    eventually_calls(&CALLS, 3);
    for h in [630, 631, 632] {
        stored(&mut c, h, t0);
    }
    c.invalidate_all(t0 + Duration::from_secs(1));
    eventually_calls(&CALLS, 6);
}

#[test]
fn a_focus_change_samples_after_the_title_bar_has_repainted() {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    fn sampler(h: Hwnd) -> Option<TitleSample> {
        CALLS.fetch_add(1, Ordering::SeqCst);
        scan_sampler(h)
    }
    let mut c = ColorCache::with_title_sampler(sampler);
    let t0 = Instant::now();
    c.scan_for(640, t0);
    eventually_calls(&CALLS, 1);
    stored(&mut c, 640, t0);
    let asked = Instant::now();
    c.refresh_after_focus(640, t0 + Duration::from_secs(3));
    eventually_calls(&CALLS, 2);
    assert!(
        asked.elapsed() >= Duration::from_millis(100),
        "settle delay"
    );
}

#[test]
fn the_accent_is_the_scans_accent_else_the_bar_color() {
    fn with_accent(h: Hwnd) -> Option<TitleSample> {
        let scan = TitleBarScan {
            accent: Some([0, 120, 215]),
            ..(*scan_of(h as i32)).clone()
        };
        Some(TitleSample {
            color: Some([9, 9, 9]),
            scan: Some(Arc::new(scan)),
        })
    }
    fn without_accent(h: Hwnd) -> Option<TitleSample> {
        Some(TitleSample {
            color: Some([9, 9, 9]),
            scan: Some(scan_of(h as i32)),
        })
    }
    let t0 = Instant::now();
    for (sampler, expected) in [
        (
            with_accent as fn(Hwnd) -> Option<TitleSample>,
            [0, 120, 215],
        ),
        (without_accent, [9, 9, 9]),
    ] {
        let mut c = ColorCache::with_title_sampler(sampler);
        c.get(650, t0);
        eventually("sample", || c.accent(650, t0) == expected);
    }
}
