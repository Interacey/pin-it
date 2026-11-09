use std::sync::atomic::{AtomicBool, Ordering};

pub(super) static MENU_OPEN: AtomicBool = AtomicBool::new(false);
pub(super) static PICKER_OPEN: AtomicBool = AtomicBool::new(false);

/// Whether `flag` is currently held by an `OpenGuard`.
pub(super) fn is_taken(flag: &AtomicBool) -> bool {
    flag.load(Ordering::Acquire)
}

/// Holds a "modal UI is open" flag until dropped.
pub(super) struct OpenGuard(&'static AtomicBool);

impl OpenGuard {
    /// Takes the flag; `None` if it is already taken.
    pub(super) fn acquire(flag: &'static AtomicBool) -> Option<OpenGuard> {
        flag.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .ok()
            .map(|_| OpenGuard(flag))
    }
}

impl Drop for OpenGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_guard_is_exclusive_and_released() {
        static FLAG: AtomicBool = AtomicBool::new(false);
        let g = OpenGuard::acquire(&FLAG).expect("first acquire");
        assert!(OpenGuard::acquire(&FLAG).is_none());
        drop(g);
        assert!(OpenGuard::acquire(&FLAG).is_some());
    }
}
