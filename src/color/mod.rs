//! Accent color sampling from the title bar and color math.
//!
//! - `math`: pure color math.
//! - `accent`: the system accent color.
//! - `accent_scan`: the most prominent chromatic color of a title-bar strip.
//! - `capture`: GDI capture of a window into a small bitmap.
//! - `ink`: the color of the caption symbols, read from the minimize and maximize buttons.
//! - `plan`: where the title bar is and which pixels to read.
//! - `scan`: one title-bar capture (`TitleSample`, `TitleBarScan`).
//! - `worker`: the sampling thread and the "color ready" post.
//! - `cache`: `ColorCache`, the per-window cache the app talks to.

mod accent;
mod accent_scan;
mod cache;
mod capture;
mod ink;
mod math;
mod plan;
mod scan;
mod worker;

pub(crate) use accent::system_accent;
pub(crate) use cache::{ColorCache, ScanLookup};
#[cfg(test)]
pub(crate) use math::contrast_ratio;
pub(crate) use math::dominant_cluster;
pub(crate) use math::{ensure_visible, ensure_visible_with, hover_ink, mix, readable_on};
#[cfg(test)]
pub(crate) use scan::TitleSample;
#[cfg(test)]
pub(crate) use scan::sample_title_bar;

use windows::Win32::UI::WindowsAndMessaging::WM_APP;

/// Posted to the notify window after the worker stored a changed color (see `ColorCache::set_notify`).
pub(crate) const WM_COLOR_READY: u32 = WM_APP + 5;

#[cfg(test)]
mod ink_validation;
#[cfg(test)]
mod real_windows;
