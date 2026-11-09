//! Pin order: checks a snapshot against the wanted order and re-applies it when it differs.

use super::ApplyError;
use super::anomaly::{describe, find_intrusion, is_competing_window, judge_now};
use super::primitives::{is_topmost, pin};
use super::snapshot::z_order_snapshot;
use crate::Hwnd;

/// What `apply_order` did: the windows it pinned successfully and the ones it could not pin.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct OrderOutcome {
    pub(crate) applied: Vec<Hwnd>,
    pub(crate) failed: Vec<(Hwnd, ApplyError)>,
    /// Log line when an unpinned window above the pins made the pins be applied again.
    pub(crate) anomaly: Option<String>,
}

/// Returns true when the pinned windows present in the snapshot have the same relative order as `bottom_to_top`; absent windows are ignored.
#[cfg(test)]
pub(crate) fn is_in_order(snapshot_top_to_bottom: &[Hwnd], bottom_to_top: &[Hwnd]) -> bool {
    let expected = bottom_to_top
        .iter()
        .rev()
        .filter(|h| snapshot_top_to_bottom.contains(h));
    let actual = snapshot_top_to_bottom
        .iter()
        .filter(|h| bottom_to_top.contains(h));
    expected.eq(actual)
}

/// Pins the windows from the oldest one that is not topmost or not in its place upwards, so the
/// newest ends on top. Windows below that point are left alone: they are already right, and
/// raising them again would be repeated work for every sweep while one window refuses.
/// When the pins are in order but an unpinned window lies above the lowest of them, all pins are
/// applied again, once per such window. Hung windows are skipped as `Hung`. Does nothing when all
/// are topmost, in order and nothing competes.
pub(crate) fn apply_order(bottom_to_top: &[Hwnd]) -> OrderOutcome {
    let snapshot = z_order_snapshot();
    let mut outcome = OrderOutcome::default();
    let first = first_unsettled(&snapshot, bottom_to_top, is_topmost)
        .or_else(|| first_after_intrusion(&snapshot, bottom_to_top, &mut outcome));
    let Some(first) = first else {
        return outcome;
    };
    for &h in bottom_to_top.iter().skip_while(|&&h| h != first) {
        match pin(h) {
            Ok(()) => outcome.applied.push(h),
            Err(e) => outcome.failed.push((h, e)),
        }
    }
    outcome
}

/// The oldest pinned window in the snapshot when an unpinned window competes with the pins and
/// the episode logic wants them applied again; the log line goes into `outcome`.
fn first_after_intrusion(
    snapshot: &[Hwnd],
    bottom_to_top: &[Hwnd],
    outcome: &mut OrderOutcome,
) -> Option<Hwnd> {
    let found = find_intrusion(snapshot, bottom_to_top, is_competing_window);
    let verdict = judge_now(found);
    let found = found.filter(|_| verdict.reassert)?;
    if verdict.log {
        outcome.anomaly = Some(describe(&found, snapshot));
    }
    bottom_to_top.iter().copied().find(|h| snapshot.contains(h))
}

/// The oldest pinned window (of those in the snapshot) that is not topmost or whose place from
/// the bottom differs from the wanted one; everything above it has to be applied again.
fn first_unsettled(
    snapshot_top_to_bottom: &[Hwnd],
    bottom_to_top: &[Hwnd],
    is_topmost: impl Fn(Hwnd) -> bool,
) -> Option<Hwnd> {
    let wanted = bottom_to_top
        .iter()
        .copied()
        .filter(|h| snapshot_top_to_bottom.contains(h));
    let actual = snapshot_top_to_bottom
        .iter()
        .rev()
        .copied()
        .filter(|h| bottom_to_top.contains(h));
    wanted
        .zip(actual)
        .find(|&(want, have)| want != have || !is_topmost(want))
        .map(|(want, _)| want)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_order_empty_and_single() {
        assert!(is_in_order(&[], &[]));
        assert!(is_in_order(&[1, 2], &[]));
        assert!(is_in_order(&[], &[1, 2]));
        assert!(is_in_order(&[5, 1, 6], &[1]));
    }

    #[test]
    fn in_order_newest_first_in_snapshot() {
        // bottom_to_top [1,2,3] means 3 is on top, so the snapshot lists 3,2,1.
        assert!(is_in_order(&[3, 2, 1], &[1, 2, 3]));
    }

    #[test]
    fn swapped_is_out_of_order() {
        assert!(!is_in_order(&[2, 3, 1], &[1, 2, 3]));
        assert!(!is_in_order(&[1, 2, 3], &[1, 2, 3]));
        assert!(!is_in_order(&[1, 2], &[1, 2]));
    }

    #[test]
    fn missing_windows_are_ignored() {
        assert!(is_in_order(&[3, 1], &[1, 2, 3]));
        assert!(!is_in_order(&[1, 3], &[1, 2, 3]));
        assert!(is_in_order(&[9, 8], &[1, 2, 3]));
    }

    #[test]
    fn unrelated_windows_between_are_ignored() {
        assert!(is_in_order(&[7, 3, 8, 2, 9, 1, 10], &[1, 2, 3]));
        assert!(!is_in_order(&[7, 2, 8, 3, 9, 1], &[1, 2, 3]));
    }

    fn all_top(_: Hwnd) -> bool {
        true
    }

    #[test]
    fn nothing_to_do_when_topmost_and_in_order() {
        assert_eq!(first_unsettled(&[3, 2, 1], &[1, 2, 3], all_top), None);
        assert_eq!(first_unsettled(&[9, 3, 8, 1], &[1, 3], all_top), None);
        assert_eq!(first_unsettled(&[], &[1, 2], all_top), None);
    }

    #[test]
    fn raised_middle_window_restarts_from_its_rank() {
        // 1 is right at the bottom; 2 and 3 are swapped: only 2 and 3 are applied again.
        assert_eq!(first_unsettled(&[2, 3, 1], &[1, 2, 3], all_top), Some(2));
    }

    #[test]
    fn raised_oldest_window_restarts_everything() {
        assert_eq!(first_unsettled(&[1, 3, 2], &[1, 2, 3], all_top), Some(1));
        assert_eq!(first_unsettled(&[3, 2, 1], &[2, 1, 3], all_top), Some(2));
    }

    #[test]
    fn window_that_is_not_topmost_is_applied_with_everything_above_it() {
        let only_one_missing = |h: Hwnd| h != 2;
        assert_eq!(
            first_unsettled(&[3, 2, 1], &[1, 2, 3], only_one_missing),
            Some(2)
        );
        let newest_missing = |h: Hwnd| h != 3;
        assert_eq!(
            first_unsettled(&[3, 2, 1], &[1, 2, 3], newest_missing),
            Some(3)
        );
    }
}
