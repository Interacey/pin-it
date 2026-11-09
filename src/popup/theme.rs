//! Colors of the popup windows: a neutral ramp for the dark and the light Windows app theme, and
//! the brand accent with its glow.

use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::core::w;

/// The accent of the popup windows (#60273D), a dark burgundy. Crisp shapes use it as it is.
pub(crate) const BRAND_ACCENT: [u8; 3] = [0x60, 0x27, 0x3D];

const WHITE: [u8; 3] = [255, 255, 255];
/// Vivid rose the light theme's glow leans toward, so it stays saturated on a light surface.
const GLOW_LIGHT_TARGET: [u8; 3] = [255, 64, 128];
const GLOW_DARK_WHITE_PERCENT: u32 = 35;
const GLOW_LIGHT_TARGET_PERCENT: u32 = 40;
/// Glow tint on dark surfaces: the brand color lifted toward white, so the burgundy is visible.
pub(crate) const GLOW_DARK: [u8; 3] = mix_rgb(BRAND_ACCENT, WHITE, GLOW_DARK_WHITE_PERCENT);
/// Glow tint on light surfaces: a more saturated brand color.
pub(crate) const GLOW_LIGHT: [u8; 3] =
    mix_rgb(BRAND_ACCENT, GLOW_LIGHT_TARGET, GLOW_LIGHT_TARGET_PERCENT);

/// `from` moved `percent` of the way to `to`, per channel, rounded toward `from`.
pub(crate) const fn mix_rgb(from: [u8; 3], to: [u8; 3], percent: u32) -> [u8; 3] {
    let mut out = [0u8; 3];
    let mut i = 0;
    while i < 3 {
        let (a, b) = (from[i] as i32, to[i] as i32);
        out[i] = (a + (b - a) * percent as i32 / 100) as u8;
        i += 1;
    }
    out
}

/// The neutral colors of a window; the accent is `BRAND_ACCENT`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Theme {
    /// The dark Windows app theme (false: light).
    pub(crate) dark: bool,
    pub(crate) background: [u8; 3],
    /// The imitated title bar; also the hover tone of a list row.
    pub(crate) strip: [u8; 3],
    /// The knob face and the input field.
    pub(crate) surface: [u8; 3],
    pub(crate) hairline: [u8; 3],
    /// The empty part of the arc, the title text bar and the selected list row.
    pub(crate) track: [u8; 3],
    pub(crate) tick_idle: [u8; 3],
    pub(crate) tick_active: [u8; 3],
    pub(crate) text: [u8; 3],
    pub(crate) muted: [u8; 3],
    /// Text and outline of an invalid input, toned down to sit inside the neutrals.
    pub(crate) error: [u8; 3],
}

impl Theme {
    pub(crate) const DARK: Theme = Theme {
        dark: true,
        background: [28, 29, 31],
        strip: [38, 39, 42],
        surface: [40, 41, 44],
        hairline: [58, 59, 63],
        track: [52, 53, 57],
        tick_idle: [78, 79, 85],
        tick_active: [150, 152, 158],
        text: [236, 237, 240],
        muted: [140, 142, 148],
        error: [240, 120, 120],
    };

    pub(crate) const LIGHT: Theme = Theme {
        dark: false,
        background: [247, 247, 248],
        strip: [235, 236, 239],
        surface: [255, 255, 255],
        hairline: [218, 219, 223],
        track: [222, 223, 227],
        tick_idle: [190, 191, 196],
        tick_active: [96, 98, 106],
        text: [27, 28, 31],
        muted: [108, 110, 118],
        error: [180, 35, 42],
    };

    /// The tint of a glow around a `BRAND_ACCENT` shape on this theme.
    pub(crate) fn glow_color(&self) -> [u8; 3] {
        if self.dark { GLOW_DARK } else { GLOW_LIGHT }
    }

    /// The theme the user's Windows apps use; dark when the setting is missing.
    pub(crate) fn system() -> Theme {
        if apps_use_light_theme() {
            Theme::LIGHT
        } else {
            Theme::DARK
        }
    }
}

/// `AppsUseLightTheme` from the current user's personalization key; false (dark) when it cannot
/// be read, which is the case on Windows versions without a light theme.
fn apps_use_light_theme() -> bool {
    let mut value = 0u32;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: `value` and `size` are valid for the call and `size` is the size of `value`.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    status.is_ok() && value != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luma(c: [u8; 3]) -> u32 {
        c.iter().map(|&v| u32::from(v)).sum()
    }

    #[test]
    fn dark_text_is_lighter_than_its_background_and_light_is_darker() {
        assert!(luma(Theme::DARK.text) > luma(Theme::DARK.background));
        assert!(luma(Theme::LIGHT.text) < luma(Theme::LIGHT.background));
        assert!(luma(Theme::DARK.muted) < luma(Theme::DARK.text));
        assert!(luma(Theme::LIGHT.muted) > luma(Theme::LIGHT.text));
    }

    #[test]
    fn the_system_theme_is_one_of_the_two() {
        let theme = Theme::system();
        assert!(theme == Theme::DARK || theme == Theme::LIGHT);
    }

    #[test]
    fn the_brand_accent_is_60273d() {
        assert_eq!(BRAND_ACCENT, [0x60, 0x27, 0x3D]);
    }

    #[test]
    fn mixing_is_exact_at_the_ends_and_between() {
        assert_eq!(mix_rgb([0, 100, 200], [100, 200, 0], 0), [0, 100, 200]);
        assert_eq!(mix_rgb([0, 100, 200], [100, 200, 0], 100), [100, 200, 0]);
        assert_eq!(mix_rgb([0, 100, 200], [100, 200, 0], 50), [50, 150, 100]);
    }

    #[test]
    fn the_dark_glow_is_the_brand_lifted_toward_white() {
        let glow = Theme::DARK.glow_color();
        assert_eq!(glow, mix_rgb(BRAND_ACCENT, WHITE, 35));
        assert!(luma(glow) > luma(BRAND_ACCENT) + 150);
    }

    #[test]
    fn the_light_glow_is_more_saturated_than_the_dark_one() {
        let spread = |c: [u8; 3]| c.iter().max().unwrap() - c.iter().min().unwrap();
        assert!(spread(Theme::LIGHT.glow_color()) > spread(Theme::DARK.glow_color()));
    }

    #[test]
    fn the_dark_flag_matches_the_ramp() {
        assert_eq!([Theme::DARK.dark, Theme::LIGHT.dark], [true, false]);
    }
}
