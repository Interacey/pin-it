//! The colors of the trash button. They are the user's choice and meant to be swapped here.

use crate::popup::Theme;

/// The trash can at rest, in both themes.
pub(super) const TRASH_REST: [u8; 3] = [0x73, 0x76, 0x72];
/// Under the mouse on the dark theme: the brighter red, visible on the dark surface.
pub(super) const TRASH_HOVER_DARK: [u8; 3] = [0xa4, 0x00, 0x07];
/// Under the mouse on the light theme: the darker red, with contrast on the light surface.
pub(super) const TRASH_HOVER_LIGHT: [u8; 3] = [0x6b, 0x01, 0x06];

pub(super) fn trash_hover(theme: &Theme) -> [u8; 3] {
    if theme.dark {
        TRASH_HOVER_DARK
    } else {
        TRASH_HOVER_LIGHT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_theme_gets_its_own_red() {
        assert_eq!(trash_hover(&Theme::DARK), [0xa4, 0x00, 0x07]);
        assert_eq!(trash_hover(&Theme::LIGHT), [0x6b, 0x01, 0x06]);
    }
}
