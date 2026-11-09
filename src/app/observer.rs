//! A small single-threaded observer mechanism.
//!
//! A `Subject<E, C>` keeps an ordered list of observers. `notify` hands every observer the event
//! and a mutable context `C` (for the app: the `App` itself), so observers need no interior
//! mutability of their own. Only the observer list is behind a `RefCell`, which is what makes a
//! subject cheap to clone and safe to use from inside one of its own observers.
//!
//! Rules:
//! - Observers run in subscription order.
//! - `notify` works on a snapshot: an observer subscribed during a round first sees the next
//!   event.
//! - A nested `notify` on the same subject runs all observers except those that are already
//!   running further up the stack.
//! - A panic in an observer unwinds through `notify` (nothing is swallowed); the subject stays
//!   usable afterwards.

use std::cell::RefCell;
use std::rc::Rc;

/// Receives events of type `E` together with the shared context.
pub(crate) trait Observer<E, C> {
    fn on_event(&mut self, event: &E, ctx: &mut C);
}

struct FnObserver<F>(F);

impl<E, C, F: FnMut(&E, &mut C)> Observer<E, C> for FnObserver<F> {
    fn on_event(&mut self, event: &E, ctx: &mut C) {
        (self.0)(event, ctx);
    }
}

type SharedObserver<E, C> = Rc<RefCell<dyn Observer<E, C>>>;

/// An event source. Cloning gives another handle to the same observer list.
pub(crate) struct Subject<E, C> {
    observers: Rc<RefCell<Vec<SharedObserver<E, C>>>>,
}

impl<E, C> Clone for Subject<E, C> {
    fn clone(&self) -> Self {
        Subject {
            observers: Rc::clone(&self.observers),
        }
    }
}

impl<E, C> Default for Subject<E, C> {
    fn default() -> Self {
        Subject {
            observers: Rc::new(RefCell::new(Vec::new())),
        }
    }
}

impl<E: 'static, C: 'static> Subject<E, C> {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Adds an observer behind all existing ones.
    pub(crate) fn subscribe(&self, observer: impl Observer<E, C> + 'static) {
        self.observers
            .borrow_mut()
            .push(Rc::new(RefCell::new(observer)));
    }

    /// Adds a closure as observer.
    pub(crate) fn subscribe_fn(&self, f: impl FnMut(&E, &mut C) + 'static) {
        self.subscribe(FnObserver(f));
    }

    /// Calls every observer in subscription order.
    pub(crate) fn notify(&self, event: &E, ctx: &mut C) {
        let snapshot = self.observers.borrow().clone();
        for observer in snapshot {
            // Already borrowed means this observer is running further up the stack.
            if let Ok(mut observer) = observer.try_borrow_mut() {
                observer.on_event(event, ctx);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    type Log = Vec<String>;

    fn recorder(name: &'static str) -> impl FnMut(&i32, &mut Log) {
        move |event, log| log.push(format!("{name}{event}"))
    }

    #[test]
    fn observers_run_in_subscription_order() {
        let subject: Subject<i32, Log> = Subject::new();
        subject.subscribe_fn(recorder("a"));
        subject.subscribe_fn(recorder("b"));
        subject.subscribe_fn(recorder("c"));
        let mut log = Log::new();
        subject.notify(&1, &mut log);
        subject.notify(&2, &mut log);
        assert_eq!(log, ["a1", "b1", "c1", "a2", "b2", "c2"]);
    }

    #[test]
    fn notify_without_observers_does_nothing() {
        let subject: Subject<i32, Log> = Subject::new();
        let mut log = Log::new();
        subject.notify(&1, &mut log);
        assert!(log.is_empty());
    }

    #[test]
    fn struct_observer_keeps_its_own_state() {
        struct Counter(u32);
        impl Observer<i32, Vec<u32>> for Counter {
            fn on_event(&mut self, _: &i32, ctx: &mut Vec<u32>) {
                self.0 += 1;
                ctx.push(self.0);
            }
        }
        let subject: Subject<i32, Vec<u32>> = Subject::new();
        subject.subscribe(Counter(0));
        let mut seen = Vec::new();
        subject.notify(&0, &mut seen);
        subject.notify(&0, &mut seen);
        assert_eq!(seen, [1, 2]);
    }

    #[test]
    fn observer_subscribed_during_a_round_first_sees_the_next_event() {
        let subject: Subject<i32, Log> = Subject::new();
        let handle = subject.clone();
        let mut added = false;
        subject.subscribe_fn(move |_, _: &mut Log| {
            if !added {
                added = true;
                handle.subscribe_fn(recorder("late"));
            }
        });
        let mut log = Log::new();
        subject.notify(&1, &mut log);
        assert!(log.is_empty());
        subject.notify(&2, &mut log);
        assert_eq!(log, ["late2"]);
    }

    #[test]
    fn nested_notify_does_not_deadlock_and_skips_running_observers() {
        let subject: Subject<i32, Log> = Subject::new();
        let handle = subject.clone();
        subject.subscribe_fn(move |event, log: &mut Log| {
            log.push(format!("outer{event}"));
            if *event == 1 {
                handle.notify(&2, log);
            }
        });
        subject.subscribe_fn(recorder("b"));
        let mut log = Log::new();
        subject.notify(&1, &mut log);
        // The nested round skips "outer" (still running) but reaches "b".
        assert_eq!(log, ["outer1", "b2", "b1"]);
    }

    #[test]
    fn observer_panic_is_not_swallowed_and_subject_stays_usable() {
        let subject: Subject<i32, Log> = Subject::new();
        subject.subscribe_fn(|event, _: &mut Log| {
            assert!(*event != 13, "unlucky");
        });
        subject.subscribe_fn(recorder("after"));
        let mut log = Log::new();
        let result = catch_unwind(AssertUnwindSafe(|| subject.notify(&13, &mut log)));
        assert!(result.is_err(), "the panic must reach the caller");
        assert!(
            log.is_empty(),
            "observers behind the panicking one did not run"
        );
        subject.notify(&1, &mut log);
        assert_eq!(log, ["after1"]);
    }
}
