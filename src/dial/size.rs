//! The button size a percent leads to, by the product's own rules in `placement`: the automatic
//! size, the user's scale and the limit by the caption height.

use crate::placement::{apply_user_scale, default_size_px, fit_size_to_caption, scale_dp};
use crate::win32::BASE_DPI;
use crate::windows_filter::PinSlot;

/// Title bar height the preview assumes: the standard Windows 11 caption. Real windows differ,
/// and the product sizes each button from its own title bar.
const REFERENCE_CAPTION_DP: i32 = 32;
/// Shown next to the size when the caption height cut the chosen size down.
const LIMITED_NOTE: &str = "limited by title bar";

/// The button size for a percent in the reference title bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct EffectiveSize {
    /// Edge in physical pixels.
    pub(super) px: u32,
    /// The caption height reduced the size the percent alone would give.
    pub(super) limited: bool,
}

/// Button edge at `dpi` for the reference title bar, as the product would draw it.
pub(super) fn effective_size(percent: u32, dpi: u32) -> EffectiveSize {
    let caption_px = scale_dp(REFERENCE_CAPTION_DP, dpi);
    let slot = PinSlot {
        x: 0,
        y: 0,
        size: default_size_px(caption_px, dpi),
    };
    let scaled = apply_user_scale(slot, percent, dpi).size;
    // A 32 dp caption always holds the user minimum, so `None` (button hidden) does not occur.
    let fitted = fit_size_to_caption(scaled, caption_px, dpi).unwrap_or(scaled);
    EffectiveSize {
        px: fitted.max(0) as u32,
        limited: fitted != scaled,
    }
}

/// The readout texts: the percent ("125 %"), the size in dp ("(28 dp)") and, when the title bar
/// limits the size, the note that says so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Readout {
    pub(super) percent: String,
    pub(super) size: String,
    pub(super) note: Option<&'static str>,
}

pub(super) fn readout(percent: u32) -> Readout {
    let effective = effective_size(percent, BASE_DPI);
    Readout {
        percent: format!("{percent} %"),
        size: format!("({} dp)", effective.px),
        note: effective.limited.then_some(LIMITED_NOTE),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px_at_96(percent: u32) -> u32 {
        effective_size(percent, BASE_DPI).px
    }

    #[test]
    fn size_follows_the_product_rule_on_the_dp_grid() {
        let dp: Vec<u32> = (50..=150).step_by(10).map(px_at_96).collect();
        assert_eq!(dp, [12, 16, 16, 20, 24, 24, 28, 28, 28, 28, 28]);
        assert!(dp.iter().all(|d| d % 4 == 0));
    }

    #[test]
    fn sizes_above_the_automatic_one_take_effect_in_a_normal_title_bar() {
        assert!(px_at_96(110) > px_at_96(100));
    }

    #[test]
    fn the_limit_is_flagged_only_when_the_caption_changed_the_size() {
        assert!(!effective_size(100, BASE_DPI).limited);
        assert!(!effective_size(110, BASE_DPI).limited);
        assert!(effective_size(125, BASE_DPI).limited);
        assert!(effective_size(150, BASE_DPI).limited);
    }

    #[test]
    fn size_scales_with_the_dpi() {
        assert_eq!(effective_size(100, 192).px, 48);
        assert_eq!(effective_size(100, 144).px, 36);
    }

    #[test]
    fn readout_shows_percent_dp_and_the_limit() {
        let plain = readout(100);
        assert_eq!(plain.percent, "100 %");
        assert_eq!(plain.size, "(24 dp)");
        assert_eq!(plain.note, None);
        let limited = readout(150);
        assert_eq!(limited.size, "(28 dp)");
        assert_eq!(limited.note, Some("limited by title bar"));
    }
}
