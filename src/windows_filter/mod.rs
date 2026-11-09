//! Decides which top-level windows are eligible for a pin button and where the button goes.
//! All functions are stateless Win32 queries; `compute_slot` is pure.

mod eligibility;
mod process;
mod slot;

pub(crate) use eligibility::{enumerate_pinnable, is_pinnable, is_shell_class};
pub(crate) use process::{exe_name, is_elevated_target};
#[cfg(test)]
pub(crate) use slot::{DefaultPlacement, compute_fallback_slot, compute_slot};
pub(crate) use slot::{PinSlot, default_placement, top_border};

/// Caption geometry that `color` uses for windows without native caption-button bounds.
pub(crate) use slot::{
    CAPTION_BUTTON_COUNT, FALLBACK_BUTTON_WIDTH_DP, FALLBACK_CAPTION_HEIGHT_DP, WINDOW_BORDER_DP,
};

#[cfg(test)]
mod tests;
