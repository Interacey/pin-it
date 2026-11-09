//! Per-window cache of sampled title colors and scans, fed by the worker thread.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::accent::system_accent;
use super::scan::{TitleSample, sample_title_bar};
use super::worker::{Mode, Notify, Sampler, Worker, skip_unresponsive};
use crate::Hwnd;
use crate::placement::TitleBarScan;
use crate::win32::is_responsive;

const DEFAULT_TTL: Duration = Duration::from_secs(30);
const RETRY_AFTER_FAILURE: Duration = Duration::from_secs(5);
/// A window that does not answer a message within this time is not captured (PrintWindow would block on it).
const RESPONSIVE_TIMEOUT_MS: u32 = 150;
const MAX_CACHE_ENTRIES: usize = 256;
const MAX_QUEUED_REQUESTS: usize = 64;
/// Entries asked for within this time are never evicted.
const KEEP_TOUCHED: Duration = Duration::from_secs(5);
/// A focus change asks for a new sample at most this often per window.
const MIN_FOCUS_REFRESH_INTERVAL: Duration = Duration::from_millis(500);
/// A size change asks for a new sample at most this often per window (ten times a second).
const MIN_RESIZE_REFRESH_INTERVAL: Duration = Duration::from_millis(100);
/// A suspended window (inside a move or size loop) is sampled again after this long, in case the end
/// of the loop was never reported.
const SUSPEND_LIMIT: Duration = Duration::from_secs(30);

struct Entry {
    color: Option<[u8; 3]>,
    scan: Option<Arc<TitleBarScan>>,
    expires: Instant,
    touched: Instant,
    /// When the last sample result was stored; limits `refresh`.
    sampled_at: Instant,
}

/// What `ColorCache::scan_for` knows about a window's title-bar scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ScanLookup {
    /// Requested, no result yet.
    Pending,
    /// A capture finished but gave no scan (and none came earlier).
    Unavailable,
    Ready(Arc<TitleBarScan>),
}

/// Per-window cache of sampled title colors with TTL, failure fallback and a size cap.
///
/// `get` and `scan_for` never sample on the caller's thread: it answers from the cache and asks the worker
/// thread to refresh. The worker starts on the first request and ends when the cache is dropped.
///
/// Notification: after the worker stored a changed color or scan it posts the message set with `set_notify`
/// to that window, at most one at a time; `poll` (also called by `get` and `scan_for`) reads the results and
/// re-arms it.
pub(crate) struct ColorCache {
    map: HashMap<Hwnd, Entry>,
    ttl: Duration,
    sampler: Sampler,
    worker: Option<Worker>,
    in_flight: HashSet<Hwnd>,
    notify: Arc<Notify>,
    /// Windows inside a modal move or size loop: no requests of their own until `resume`.
    suspended: HashMap<Hwnd, Instant>,
    /// When a size change last asked for a sample, per window.
    resize_requested: HashMap<Hwnd, Instant>,
    /// How often `invalidate_all` ran; tests read it.
    #[cfg(test)]
    invalidations: u32,
}

impl Default for ColorCache {
    fn default() -> Self {
        Self::new()
    }
}

impl ColorCache {
    /// Creates an empty cache with a 30 s TTL.
    pub(crate) fn new() -> Self {
        Self::with_title_sampler(skip_unresponsive(
            |hwnd| is_responsive(hwnd, RESPONSIVE_TIMEOUT_MS),
            sample_title_bar,
        ))
    }

    /// Same as `new` with an injectable sampler for the worker thread.
    #[cfg(test)]
    fn with_sampler(sampler: fn(Hwnd) -> Option<[u8; 3]>) -> Self {
        Self::with_title_sampler(move |h| sampler(h).map(TitleSample::from_color))
    }

    /// Same as `new` with an injectable sampler that also returns scans.
    fn with_title_sampler(
        sampler: impl Fn(Hwnd) -> Option<TitleSample> + Send + Sync + 'static,
    ) -> Self {
        Self {
            map: HashMap::new(),
            ttl: DEFAULT_TTL,
            sampler: Arc::new(sampler),
            worker: None,
            in_flight: HashSet::new(),
            notify: Arc::new(Notify::default()),
            suspended: HashMap::new(),
            resize_requested: HashMap::new(),
            #[cfg(test)]
            invalidations: 0,
        }
    }

    /// A cache whose worker runs `sampler`, for tests that control what the worker returns and when.
    #[cfg(test)]
    pub(crate) fn with_injected_sampler(
        sampler: impl Fn(Hwnd) -> Option<TitleSample> + Send + Sync + 'static,
    ) -> Self {
        Self::with_title_sampler(sampler)
    }

    /// Sets how long a sampled color stays valid.
    #[cfg(test)]
    fn set_ttl(&mut self, ttl: Duration) {
        self.ttl = ttl;
    }

    /// Makes the worker post `msg` to `notify_hwnd` after it stored a changed color; at most one post waits unread until the next `poll` or `get`.
    pub(crate) fn set_notify(&mut self, notify_hwnd: Hwnd, msg: u32) {
        self.notify.set_target(notify_hwnd, msg);
    }

    /// Takes the finished samples from the worker into the cache.
    pub(crate) fn poll(&mut self, now: Instant) {
        // Cleared before draining: a result that lands later posts again.
        self.notify.acknowledge();
        let Some(worker) = &self.worker else { return };
        for (hwnd, sampled) in worker.finished() {
            // Removed by `forget` while in flight: the result is stale.
            if self.in_flight.remove(&hwnd) {
                self.store(hwnd, sampled, now);
            }
        }
    }

    /// Color for the window without blocking: the cached value if younger than the TTL, else the last good value, else the system accent.
    /// A missing or expired entry queues a refresh on the worker thread (one per window, at most 64 waiting); the result shows up in a later `get` or `poll`.
    pub(crate) fn get(&mut self, hwnd: Hwnd, now: Instant) -> [u8; 3] {
        self.poll(now);
        if self.touch_is_fresh(hwnd, now) {
            return self.cached_color(hwnd).unwrap_or_else(system_accent);
        }
        let last = self.cached_color(hwnd);
        self.request(hwnd);
        last.unwrap_or_else(system_accent)
    }

    /// The window's accent without blocking: the most prominent chromatic color of its title-bar strip, else
    /// the title-bar color (`get`).
    pub(crate) fn accent(&mut self, hwnd: Hwnd, now: Instant) -> [u8; 3] {
        let bar = self.get(hwnd, now);
        self.map
            .get(&hwnd)
            .and_then(|e| e.scan.as_ref())
            .and_then(|scan| scan.accent)
            .unwrap_or(bar)
    }

    /// Title-bar scan of the window without blocking: the cached one (even past its TTL, a stale scan is replaced when the refresh lands).
    /// A missing or expired entry queues a refresh like `get`.
    pub(crate) fn scan_for(&mut self, hwnd: Hwnd, now: Instant) -> ScanLookup {
        self.poll(now);
        if !self.touch_is_fresh(hwnd, now) {
            self.request(hwnd);
        }
        match self.map.get(&hwnd) {
            None => ScanLookup::Pending,
            Some(e) => e
                .scan
                .clone()
                .map_or(ScanLookup::Unavailable, ScanLookup::Ready),
        }
    }

    /// Asks for a fresh sample when the last one is `age` old or older.
    pub(crate) fn refresh_if_older(&mut self, hwnd: Hwnd, now: Instant, age: Duration) {
        let due = self
            .map
            .get(&hwnd)
            .is_some_and(|e| now.saturating_duration_since(e.sampled_at) >= age);
        if due {
            self.expire_and_request(hwnd, now, false);
        }
    }

    /// Asks for a fresh sample after the window gained or lost the foreground: taken a moment later,
    /// when the title bar shows its new active or inactive look.
    pub(crate) fn refresh_after_focus(&mut self, hwnd: Hwnd, now: Instant) {
        let due = self.map.get(&hwnd).is_some_and(|e| {
            now.saturating_duration_since(e.sampled_at) >= MIN_FOCUS_REFRESH_INTERVAL
        });
        if due {
            self.expire_and_request(hwnd, now, true);
        }
    }

    /// Asks for a fresh sample after a size change: the old scan is for another window size. Not held back by
    /// the 2 s limit or by `suspend`, but at most ten times a second per window. A request already in flight
    /// is not doubled.
    pub(crate) fn refresh_resized(&mut self, hwnd: Hwnd, now: Instant) {
        let due = self
            .resize_requested
            .get(&hwnd)
            .is_none_or(|&t| now.saturating_duration_since(t) >= MIN_RESIZE_REFRESH_INTERVAL);
        if due && self.map.contains_key(&hwnd) {
            self.resize_requested.insert(hwnd, now);
            if let Some(e) = self.map.get_mut(&hwnd) {
                e.expires = now;
            }
            self.send_request(hwnd, false);
        }
    }

    /// A custom title bar waits for a scan it can use (the last one failed or shows an unpainted window): asks for
    /// a new sample after the settle delay and wakes the UI thread with the result even when it equals the last,
    /// so the wait ends by itself. Nothing to do before the first result: that request is on its way.
    pub(crate) fn refresh_awaited(&mut self, hwnd: Hwnd, now: Instant) {
        let due = self
            .resize_requested
            .get(&hwnd)
            .is_none_or(|&t| now.saturating_duration_since(t) >= MIN_RESIZE_REFRESH_INTERVAL);
        if due && self.map.contains_key(&hwnd) && !self.is_suspended(hwnd) {
            self.resize_requested.insert(hwnd, now);
            self.send(hwnd, Mode::Awaited);
        }
    }

    /// Marks every cached sample as expired and asks for new ones (theme or accent change).
    pub(crate) fn invalidate_all(&mut self, now: Instant) {
        #[cfg(test)]
        {
            self.invalidations += 1;
        }
        let windows: Vec<Hwnd> = self.map.keys().copied().collect();
        for hwnd in windows {
            self.expire_and_request(hwnd, now, false);
        }
    }

    /// No samples of `hwnd` of its own until `resume` (a move loop: the window is busy and only its
    /// position changes).
    pub(crate) fn suspend(&mut self, hwnd: Hwnd, now: Instant) {
        self.suspended.insert(hwnd, now);
    }

    /// Ends `suspend` and asks for a fresh sample.
    pub(crate) fn resume(&mut self, hwnd: Hwnd, now: Instant) {
        if self.suspended.remove(&hwnd).is_some() {
            self.expire_and_request(hwnd, now, false);
        }
    }

    /// How many entries of per-window state the cache holds (samples, requests in flight, suspensions, resize throttles).
    #[cfg(test)]
    pub(crate) fn tracked_windows(&self) -> usize {
        self.map.len() + self.in_flight.len() + self.suspended.len() + self.resize_requested.len()
    }

    #[cfg(test)]
    pub(crate) fn invalidation_count(&self) -> u32 {
        self.invalidations
    }

    fn is_suspended(&self, hwnd: Hwnd) -> bool {
        self.suspended
            .get(&hwnd)
            .is_some_and(|&t| t.elapsed() < SUSPEND_LIMIT)
    }

    fn expire_and_request(&mut self, hwnd: Hwnd, now: Instant, settle: bool) {
        if let Some(e) = self.map.get_mut(&hwnd) {
            e.expires = now;
        }
        if !self.is_suspended(hwnd) {
            self.send_request(hwnd, settle);
        }
    }

    /// Same as `get` but samples inline with the given sampler (for pure tests).
    #[cfg(test)]
    fn get_with(
        &mut self,
        sampler: fn(Hwnd) -> Option<[u8; 3]>,
        hwnd: Hwnd,
        now: Instant,
    ) -> [u8; 3] {
        if self.touch_is_fresh(hwnd, now) {
            return self.cached_color(hwnd).unwrap_or_else(system_accent);
        }
        self.store(hwnd, sampler(hwnd).map(TitleSample::from_color), now)
            .unwrap_or_else(system_accent)
    }

    /// Removes the window's entry (including the last good color) and ignores a result still in
    /// flight for it; the next `get` resamples.
    pub(crate) fn forget(&mut self, hwnd: Hwnd) {
        self.map.remove(&hwnd);
        self.in_flight.remove(&hwnd);
        self.suspended.remove(&hwnd);
        self.resize_requested.remove(&hwnd);
        if let Some(worker) = &self.worker {
            worker.forget(hwnd);
        }
    }

    /// Marks the entry as asked for; true when it exists and has not expired.
    fn touch_is_fresh(&mut self, hwnd: Hwnd, now: Instant) -> bool {
        let Some(e) = self.map.get_mut(&hwnd) else {
            return false;
        };
        e.touched = now;
        now < e.expires
    }

    fn cached_color(&self, hwnd: Hwnd) -> Option<[u8; 3]> {
        self.map.get(&hwnd).and_then(|e| e.color)
    }

    fn request(&mut self, hwnd: Hwnd) {
        if !self.is_suspended(hwnd) {
            self.send_request(hwnd, false);
        }
    }

    fn send_request(&mut self, hwnd: Hwnd, settle: bool) {
        self.send(hwnd, if settle { Mode::Settled } else { Mode::Plain });
    }

    fn send(&mut self, hwnd: Hwnd, mode: Mode) {
        if self.in_flight.contains(&hwnd) || self.in_flight.len() >= MAX_QUEUED_REQUESTS {
            return;
        }
        if self.worker.is_none() {
            self.worker = Worker::spawn(self.sampler.clone(), self.notify.clone());
        }
        let Some(worker) = &self.worker else { return };
        if worker.request(hwnd, mode) {
            self.in_flight.insert(hwnd);
        }
    }

    /// Records a sample: a success restarts the TTL, a failure keeps the last good color and scan and retries soon.
    /// A success without a color keeps the last color.
    fn store(&mut self, hwnd: Hwnd, sampled: Option<TitleSample>, now: Instant) -> Option<[u8; 3]> {
        let (color, scan, wait) = self.merge_with_last(hwnd, sampled);
        if !self.map.contains_key(&hwnd) {
            self.make_room(now);
        }
        self.map.insert(
            hwnd,
            Entry {
                color,
                scan,
                expires: now + wait,
                touched: now,
                sampled_at: now,
            },
        );
        color
    }

    /// Color, scan and validity period for a new sample, falling back to the last good values.
    fn merge_with_last(
        &self,
        hwnd: Hwnd,
        sampled: Option<TitleSample>,
    ) -> (Option<[u8; 3]>, Option<Arc<TitleBarScan>>, Duration) {
        let last = self.map.get(&hwnd);
        let last_color = last.and_then(|e| e.color);
        match sampled {
            Some(s) => (s.color.or(last_color), s.scan, self.ttl),
            None => (
                last_color,
                last.and_then(|e| e.scan.clone()),
                self.ttl.min(RETRY_AFTER_FAILURE),
            ),
        }
    }

    /// Evicts entries with the oldest expiry until one slot is free, but never one asked for in the last 5 s.
    /// With more than the cap in live use the cache grows instead, so a sweep over many windows does not resample in a loop.
    fn make_room(&mut self, now: Instant) {
        while self.map.len() >= MAX_CACHE_ENTRIES {
            let victim = self
                .map
                .iter()
                .filter(|(_, e)| now.saturating_duration_since(e.touched) >= KEEP_TOUCHED)
                .min_by_key(|(_, e)| e.expires)
                .map(|(k, _)| *k);
            match victim {
                Some(k) => self.map.remove(&k),
                None => break,
            };
        }
    }
}

#[cfg(test)]
mod tests;
