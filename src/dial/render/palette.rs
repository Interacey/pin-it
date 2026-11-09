//! The materials of the dial for the dark and the light theme: charcoal or light grey plastic,
//! engraved marks, and the opacities of the glows. The light always comes from the top left.

use crate::popup::Theme;

/// An RGB color with an opacity from 0 to 1.
#[derive(Debug, Clone, Copy)]
pub(super) struct Tint {
    pub(super) rgb: [u8; 3],
    pub(super) alpha: f32,
}

const fn tint(rgb: [u8; 3], alpha: f32) -> Tint {
    Tint { rgb, alpha }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Plastic {
    /// The rim between its lit and its shaded side.
    pub(super) rim: [u8; 3],
    pub(super) rim_lit: Tint,
    pub(super) rim_shade: Tint,
    /// The recessed face: lit near the top left, darker toward the edge.
    pub(super) face_lit: [u8; 3],
    pub(super) face_edge: [u8; 3],
    /// Shade just inside the rim of the face, where the recess casts a shadow.
    pub(super) recess_shade: Tint,
    /// The thin bright lip at the lower right of the recess.
    pub(super) recess_lip: Tint,
    /// A dark line where rim and face meet, and around the knob.
    pub(super) seam: Tint,
    /// The knurling: dark flank and light flank of each ridge.
    pub(super) ridge_dark: Tint,
    pub(super) ridge_light: Tint,
    /// The fixed highlight on the face and along the rim.
    pub(super) specular: Tint,
    /// The recessed track of the scale ring and the engraved marks are opaque: they are drawn over
    /// their own light edge, which a see-through color would let shine through.
    pub(super) groove: Tint,
    /// An engraved mark: the cut itself and the light edge one pixel below it.
    pub(super) engraved: Tint,
    pub(super) engraved_edge: Tint,
    /// Strength of the brand glow at rest and with a flash. Above 1 on purpose: the blurred mask
    /// of a thin line peaks far below 1, so the tint needs the headroom to read.
    pub(super) glow: f32,
    pub(super) flash_glow: f32,
}

const DARK: Plastic = Plastic {
    rim: [66, 68, 74],
    rim_lit: tint([170, 173, 182], 0.65),
    rim_shade: tint([8, 8, 10], 0.70),
    face_lit: [78, 80, 87],
    face_edge: [30, 31, 34],
    recess_shade: tint([0, 0, 0], 0.60),
    recess_lip: tint([255, 255, 255], 0.14),
    seam: tint([0, 0, 0], 0.65),
    ridge_dark: tint([0, 0, 0], 0.55),
    ridge_light: tint([255, 255, 255], 0.30),
    specular: tint([255, 255, 255], 0.30),
    groove: tint([13, 13, 14], 1.0),
    engraved: tint([7, 7, 8], 1.0),
    engraved_edge: tint([255, 255, 255], 0.16),
    glow: 2.2,
    flash_glow: 3.5,
};

const LIGHT: Plastic = Plastic {
    rim: [212, 214, 220],
    rim_lit: tint([255, 255, 255], 0.90),
    rim_shade: tint([70, 73, 84], 0.55),
    face_lit: [238, 240, 244],
    face_edge: [168, 172, 184],
    recess_shade: tint([40, 42, 52], 0.40),
    recess_lip: tint([255, 255, 255], 0.90),
    seam: tint([50, 52, 62], 0.45),
    ridge_dark: tint([50, 52, 62], 0.40),
    ridge_light: tint([255, 255, 255], 0.90),
    specular: tint([255, 255, 255], 0.35),
    groove: tint([205, 206, 210], 1.0),
    engraved: tint([133, 134, 139], 1.0),
    engraved_edge: tint([255, 255, 255], 0.95),
    glow: 1.4,
    flash_glow: 2.4,
};

impl Plastic {
    pub(super) fn of(theme: &Theme) -> Plastic {
        if theme.dark { DARK } else { LIGHT }
    }
}
