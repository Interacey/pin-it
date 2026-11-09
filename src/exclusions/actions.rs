//! What the window asks the app to do. The window never touches the `App`: it queues an action
//! here and posts one message to the app window, which drains the queue.

use std::cell::RefCell;

/// A change of the exclusion list the user asked for. The name of `Add` is normalized already;
/// the app checks it again, since it is the one that owns the settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Action {
    Add(String),
    Remove(String),
}

thread_local! {
    static QUEUE: RefCell<Vec<Action>> = const { RefCell::new(Vec::new()) };
}

pub(super) fn push(action: Action) {
    QUEUE.with(|queue| queue.borrow_mut().push(action));
}

/// Takes the queued actions, oldest first.
pub(crate) fn drain_actions() -> Vec<Action> {
    QUEUE.with(|queue| std::mem::take(&mut *queue.borrow_mut()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draining_returns_the_actions_in_order_and_empties_the_queue() {
        drain_actions();
        push(Action::Add("a.exe".into()));
        push(Action::Remove("b.exe".into()));
        assert_eq!(
            drain_actions(),
            [Action::Add("a.exe".into()), Action::Remove("b.exe".into())]
        );
        assert!(drain_actions().is_empty());
    }
}
