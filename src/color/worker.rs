//! The sampling thread, its request/result channels and the "color ready" notification.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use super::scan::TitleSample;
use crate::Hwnd;
use crate::win32::to_hwnd;

/// Remembered last samples; four times the cache cap, then the memory is dropped and every sample counts as changed once.
const REMEMBERED_SAMPLES_LIMIT: usize = 4 * 256;

/// Captures one window's title bar; runs on the worker thread.
pub(super) type Sampler = Arc<dyn Fn(Hwnd) -> Option<TitleSample> + Send + Sync>;

pub(super) type Sample = (Hwnd, Option<TitleSample>);

/// How long the worker waits before it samples a window that has just changed focus: the title bar
/// repaints in its active or inactive look a moment after the focus event.
const SETTLE_DELAY: Duration = Duration::from_millis(150);

/// How a sampling job is served.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    Plain,
    /// Wait `SETTLE_DELAY` first, to let the window repaint.
    Settled,
    /// Like `Settled`, and wake the UI thread even when the sample equals the last one: a window that waits for
    /// a plausible scan must look at it again.
    Awaited,
}

/// One sampling job.
#[derive(Clone, Copy)]
struct Request {
    hwnd: Hwnd,
    mode: Mode,
}

/// What the cache tells the worker thread.
enum Job {
    Sample(Request),
    /// The window is gone: drop what the worker remembers about it.
    Forget(Hwnd),
}

/// Shared between the cache and its worker: where to post, and whether a post is still unread.
/// The worker calls `post` after a changed result; the cache calls `acknowledge` when it reads results.
#[derive(Default)]
pub(super) struct Notify {
    hwnd: AtomicIsize,
    msg: AtomicU32,
    pending: AtomicBool,
}

impl Notify {
    /// Sets the window and message that `post` sends to.
    pub(super) fn set_target(&self, hwnd: Hwnd, msg: u32) {
        self.msg.store(msg, Ordering::Release);
        self.hwnd.store(hwnd, Ordering::Release);
    }

    /// Posts the message unless one is already waiting.
    pub(super) fn post(&self) {
        let hwnd = self.hwnd.load(Ordering::Acquire);
        if hwnd == 0 || self.pending.swap(true, Ordering::AcqRel) {
            return;
        }
        let msg = self.msg.load(Ordering::Acquire);
        // SAFETY: plain post to a window handle that may be gone; failure is handled.
        let posted = unsafe { PostMessageW(Some(to_hwnd(hwnd)), msg, WPARAM(0), LPARAM(0)) };
        if posted.is_err() {
            self.pending.store(false, Ordering::Release);
        }
    }

    /// Marks the posted message as read, so the next changed result posts again.
    pub(super) fn acknowledge(&self) {
        self.pending.store(false, Ordering::Release);
    }

    #[cfg(test)]
    pub(super) fn is_pending(&self) -> bool {
        self.pending.load(Ordering::SeqCst)
    }
}

/// Request channel to, and result channel from, the one sampling thread.
pub(super) struct Worker {
    requests: mpsc::Sender<Job>,
    results: mpsc::Receiver<Sample>,
}

impl Worker {
    /// Starts the thread; it ends when the `Worker` (the only request sender) is dropped.
    pub(super) fn spawn(sampler: Sampler, notify: Arc<Notify>) -> Option<Worker> {
        let (requests, request_rx) = mpsc::channel();
        let (result_tx, results) = mpsc::channel();
        std::thread::Builder::new()
            .name("pin-it-color".into())
            .spawn(move || {
                let sampling = Sampling {
                    last: LastSamples::default(),
                    results: result_tx,
                    sampler,
                    notify,
                };
                run(request_rx, sampling)
            })
            .ok()?;
        Some(Worker { requests, results })
    }

    /// Queues a sample request; false when the thread is gone.
    pub(super) fn request(&self, hwnd: Hwnd, mode: Mode) -> bool {
        self.requests
            .send(Job::Sample(Request { hwnd, mode }))
            .is_ok()
    }

    /// Tells the worker to forget its last sample of `hwnd`.
    pub(super) fn forget(&self, hwnd: Hwnd) {
        // A worker that is gone remembers nothing, so a failed send needs no handling.
        let _ = self.requests.send(Job::Forget(hwnd));
    }

    /// The samples finished since the last call.
    pub(super) fn finished(&self) -> Vec<Sample> {
        self.results.try_iter().collect()
    }
}

/// Last sample sent per window (`None` for a failed capture), so only a changed color or scan, or the first
/// failure, wakes the UI thread.
#[derive(Default)]
struct LastSamples(HashMap<Hwnd, Option<TitleSample>>);

impl LastSamples {
    /// Remembers a sample and says whether it differs from the previous one of that window. A failure is news
    /// only when nothing is known of the window yet: a window that waits for its first scan must hear of it.
    fn record(&mut self, hwnd: Hwnd, sampled: Option<&TitleSample>) -> bool {
        let known = self.0.contains_key(&hwnd);
        let changed = match sampled {
            Some(s) => self
                .0
                .insert(hwnd, Some(s.clone()))
                .is_none_or(|last| last.as_ref() != Some(s)),
            None if known => false,
            None => self.0.insert(hwnd, None).is_none(),
        };
        if self.0.len() > REMEMBERED_SAMPLES_LIMIT {
            self.0.clear();
        }
        changed
    }

    fn forget(&mut self, hwnd: Hwnd) {
        self.0.remove(&hwnd);
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.0.len()
    }
}

/// The worker thread's state: what it sampled last, and where results and notifications go.
struct Sampling {
    last: LastSamples,
    results: mpsc::Sender<Sample>,
    sampler: Sampler,
    notify: Arc<Notify>,
}

impl Sampling {
    /// Samples one window and reports it; false when the cache no longer listens.
    fn serve(&mut self, Request { hwnd, mode }: Request) -> bool {
        if mode != Mode::Plain {
            std::thread::sleep(SETTLE_DELAY);
        }
        let sampled = (self.sampler)(hwnd);
        let changed = self.last.record(hwnd, sampled.as_ref()) || mode == Mode::Awaited;
        if self.results.send((hwnd, sampled)).is_err() {
            return false;
        }
        if changed {
            self.notify.post();
        }
        true
    }
}

fn run(jobs: mpsc::Receiver<Job>, mut sampling: Sampling) {
    // Ends when the cache (the only sender) is dropped.
    while let Ok(job) = jobs.recv() {
        match job {
            Job::Forget(hwnd) => sampling.last.forget(hwnd),
            Job::Sample(request) if !sampling.serve(request) => break,
            Job::Sample(_) => {}
        }
    }
}

/// Wraps `sample` so a window whose thread does not answer `is_responsive` is skipped (result `None`,
/// which the cache retries later) instead of blocking the one worker thread inside `PrintWindow`.
pub(super) fn skip_unresponsive(
    is_responsive: impl Fn(Hwnd) -> bool,
    sample: impl Fn(Hwnd) -> Option<TitleSample>,
) -> impl Fn(Hwnd) -> Option<TitleSample> {
    move |hwnd| is_responsive(hwnd).then(|| sample(hwnd)).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(shade: u8) -> TitleSample {
        TitleSample::from_color([shade; 3])
    }

    #[test]
    fn a_forgotten_window_is_not_remembered_and_counts_as_changed_again() {
        let mut last = LastSamples::default();
        assert!(last.record(1, Some(&sample(5))));
        assert!(!last.record(1, Some(&sample(5))));
        last.forget(1);
        assert_eq!(last.len(), 0);
        assert!(last.record(1, Some(&sample(5))));
    }

    #[test]
    fn the_first_failure_is_news_and_a_repeat_or_a_failure_after_a_success_is_not() {
        let mut last = LastSamples::default();
        assert!(last.record(1, None));
        assert!(!last.record(1, None));
        assert!(last.record(1, Some(&sample(5))));
        assert!(!last.record(1, None));
        assert!(!last.record(1, Some(&sample(5))));
    }

    #[test]
    fn the_worker_drops_what_it_remembers_of_a_window_the_cache_forgets() {
        let counted = Arc::new(AtomicU32::new(0));
        let sampler: Sampler = {
            let counted = counted.clone();
            Arc::new(move |_| {
                counted.fetch_add(1, Ordering::SeqCst);
                Some(sample(7))
            })
        };
        let notify = Arc::new(Notify::default());
        let worker = Worker::spawn(sampler, notify.clone()).unwrap();
        assert!(worker.request(9, Mode::Plain));
        wait_until(|| !worker.finished().is_empty());
        // `notify` has no target window, so it never posts: the forget path is checked through the result of a second sample
        worker.forget(9);
        assert!(worker.request(9, Mode::Plain));
        wait_until(|| counted.load(Ordering::SeqCst) == 2);
    }

    fn wait_until(mut condition: impl FnMut() -> bool) {
        let end = std::time::Instant::now() + Duration::from_secs(5);
        while !condition() {
            assert!(std::time::Instant::now() < end, "timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
