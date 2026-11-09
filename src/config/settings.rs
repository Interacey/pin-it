use crate::hotkey::Hotkey;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const FORMAT_VERSION: u32 = 1;
const DEFAULT_BUTTON_SCALE_PERCENT: u32 = 100;
/// Smallest and largest accepted pin button size in percent.
const BUTTON_SCALE_RANGE_PERCENT: std::ops::RangeInclusive<u32> = 50..=200;

/// User settings stored as JSON; every field falls back to its default when missing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Settings {
    /// File format version.
    pub(crate) version: u32,
    /// Master switch for the pin buttons.
    pub(crate) enabled: bool,
    /// Global hotkey that pins the foreground window; `None` (the default) registers nothing.
    pub(crate) hotkey: Option<Hotkey>,
    /// Take the accent color from the pinned window.
    pub(crate) use_window_color: bool,
    /// Accent used when `use_window_color` is off; `None` means the system accent.
    pub(crate) fixed_accent: Option<[u8; 3]>,
    /// Animate state changes.
    pub(crate) animate: bool,
    /// Start with Windows.
    pub(crate) autostart: bool,
    /// Pin button size in percent, clamped to 50..=200.
    pub(crate) button_scale_percent: u32,
    /// Play a quiet click on every detent of the button size dial.
    pub(crate) dial_click_sound: bool,
    /// Lowercase exe names that get no pin button.
    pub(crate) excluded_apps: Vec<String>,
    /// Accent override per lowercase exe name.
    pub(crate) per_app_accent: HashMap<String, [u8; 3]>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            version: FORMAT_VERSION,
            enabled: true,
            hotkey: None,
            use_window_color: true,
            fixed_accent: None,
            animate: true,
            autostart: false,
            button_scale_percent: DEFAULT_BUTTON_SCALE_PERCENT,
            dial_click_sound: true,
            excluded_apps: Vec::new(),
            per_app_accent: HashMap::new(),
        }
    }
}

impl Settings {
    /// Returns true if the exe name is on the exclusion list, ignoring case.
    pub(crate) fn is_excluded(&self, exe: &str) -> bool {
        self.excluded_apps
            .iter()
            .any(|e| e.eq_ignore_ascii_case(exe))
    }

    /// Picks the accent: per-app override, then window color, else fixed or system accent.
    pub(crate) fn accent_for(
        &self,
        exe: Option<&str>,
        window_color: [u8; 3],
        system: [u8; 3],
    ) -> [u8; 3] {
        if let Some(color) = exe.and_then(|exe| self.per_app_override(exe)) {
            return color;
        }
        if self.use_window_color {
            window_color
        } else {
            self.fixed_accent.unwrap_or(system)
        }
    }

    fn per_app_override(&self, exe: &str) -> Option<[u8; 3]> {
        self.per_app_accent
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(exe))
            .map(|(_, color)| *color)
    }

    /// Clamps values into their valid range and normalizes exe names to lowercase.
    pub(crate) fn sanitize(&mut self) {
        self.button_scale_percent = clamp_button_scale(self.button_scale_percent);
        self.hotkey = self.hotkey.filter(Hotkey::is_valid);
        self.normalize_excluded_apps();
        self.normalize_per_app_accent();
    }

    fn normalize_excluded_apps(&mut self) {
        for exe in &mut self.excluded_apps {
            *exe = exe.trim().to_lowercase();
        }
        self.excluded_apps.retain(|exe| !exe.is_empty());
        self.excluded_apps.sort();
        self.excluded_apps.dedup();
    }

    fn normalize_per_app_accent(&mut self) {
        self.per_app_accent = std::mem::take(&mut self.per_app_accent)
            .into_iter()
            .map(|(exe, color)| (exe.trim().to_lowercase(), color))
            .collect();
    }
}

/// Clamps a button size into `BUTTON_SCALE_RANGE_PERCENT`.
pub(crate) fn clamp_button_scale(percent: u32) -> u32 {
    percent.clamp(
        *BUTTON_SCALE_RANGE_PERCENT.start(),
        *BUTTON_SCALE_RANGE_PERCENT.end(),
    )
}
