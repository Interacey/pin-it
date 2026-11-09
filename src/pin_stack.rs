//! Order of pinned windows, independent of Win32.
//!
//! Pinned windows form a stack by pin time: the newest pin is on top, the
//! oldest at the bottom. Unpinned windows are not stored; they always sit
//! behind the oldest pinned window.

/// Pinned ids ordered by pin time, oldest first.
///
/// Activating or re-clicking an older pinned window does not change its rank.
/// The applier must call [`PinStack::bottom_to_top`] again to restore the
/// order, so no separate restack method exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PinStack<T: Copy + Eq> {
    /// Oldest pin at index 0, newest at the end.
    items: Vec<T>,
}

impl<T: Copy + Eq> Default for PinStack<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Copy + Eq> PinStack<T> {
    /// Creates an empty stack.
    pub(crate) fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// Returns true if no window is pinned.
    pub(crate) fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Returns true if `id` is pinned.
    pub(crate) fn is_pinned(&self, id: &T) -> bool {
        self.items.contains(id)
    }

    /// Puts `id` on top; returns false and changes nothing if already pinned.
    pub(crate) fn pin(&mut self, id: T) -> bool {
        if self.is_pinned(&id) {
            return false;
        }
        self.items.push(id);
        true
    }

    /// Removes `id`; returns whether it was pinned.
    pub(crate) fn unpin(&mut self, id: &T) -> bool {
        match self.items.iter().position(|x| x == id) {
            Some(pos) => {
                self.items.remove(pos);
                true
            }
            None => false,
        }
    }

    /// Iterates oldest first, the order for SetWindowPos(HWND_TOPMOST) so the newest ends on top.
    pub(crate) fn bottom_to_top(&self) -> impl Iterator<Item = &T> {
        self.items.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order(s: &PinStack<i32>) -> Vec<i32> {
        s.bottom_to_top().copied().collect()
    }

    fn stack(ids: &[i32]) -> PinStack<i32> {
        let mut s = PinStack::new();
        for &id in ids {
            s.pin(id);
        }
        s
    }

    #[test]
    fn empty_stack() {
        let mut s: PinStack<i32> = PinStack::default();
        assert!(s.is_empty());
        assert!(!s.is_pinned(&1));
        assert!(!s.unpin(&1));
        assert!(order(&s).is_empty());
    }

    #[test]
    fn pin_order_newest_on_top() {
        let s = stack(&[1, 2, 3]);
        assert_eq!(order(&s).last(), Some(&3));
        assert!(s.is_pinned(&2));
    }

    #[test]
    fn bottom_to_top_is_oldest_first() {
        assert_eq!(order(&stack(&[5, 7, 6])), vec![5, 7, 6]);
    }

    #[test]
    fn pin_twice_is_noop() {
        let mut s = stack(&[1, 2]);
        assert!(!s.pin(1));
        assert_eq!(order(&s), vec![1, 2]);
    }

    #[test]
    fn unpin_middle_keeps_order() {
        let mut s = stack(&[1, 2, 3]);
        assert!(s.unpin(&2));
        assert!(!s.is_pinned(&2));
        assert_eq!(order(&s), vec![1, 3]);
    }

    #[test]
    fn unpin_unknown_returns_false() {
        let mut s = stack(&[1, 2]);
        assert!(!s.unpin(&9));
        assert_eq!(order(&s), vec![1, 2]);
    }

    #[test]
    fn repin_after_unpin_goes_to_top() {
        let mut s = stack(&[1, 2, 3]);
        s.unpin(&1);
        assert!(s.pin(1));
        assert_eq!(order(&s), vec![2, 3, 1]);
    }
}
