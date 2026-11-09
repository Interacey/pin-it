//! Where everything of the dial window sits, in dp and for one window scale in pixels.

use crate::dial::model::angle_of_percent;

/// Window size in dp.
pub(in crate::dial) const WIDTH_DP: f32 = 280.0;
pub(in crate::dial) const HEIGHT_DP: f32 = 356.0;

const MARGIN_DP: f32 = 16.0;
const STRIP_TOP_DP: f32 = 16.0;
const STRIP_HEIGHT_DP: f32 = 48.0;
/// Width of one caption button of the imitated title bar.
const CAPTION_CELL_DP: f32 = 40.0;
const CAPTION_CELLS: usize = 3;

const DIAL_CENTER_Y_DP: f32 = 188.0;
/// Pointer presses within this radius grab the knob.
const GRAB_RADIUS_DP: f32 = 104.0;
/// Distance of a scale label from the center.
const LABEL_RADIUS_DP: f32 = 112.0;
const READOUT_BASELINE_DP: f32 = 308.0;
/// Baseline of the line under the readout that names the title bar limit.
const NOTE_BASELINE_DP: f32 = 334.0;

/// Pixel positions for one window scale (pixels per dp).
#[derive(Debug, Clone, Copy)]
pub(in crate::dial) struct Layout {
    pub(in crate::dial) scale: f32,
}

impl Layout {
    pub(in crate::dial) fn new(scale: f32) -> Self {
        Self { scale }
    }

    pub(in crate::dial) fn px(&self, dp: f32) -> f32 {
        dp * self.scale
    }

    /// Window size in pixels.
    pub(in crate::dial) fn size_px(&self) -> (u32, u32) {
        (
            self.px(WIDTH_DP).round() as u32,
            self.px(HEIGHT_DP).round() as u32,
        )
    }

    /// Center of the dial.
    pub(in crate::dial) fn center(&self) -> (f32, f32) {
        (self.px(WIDTH_DP / 2.0), self.px(DIAL_CENTER_Y_DP))
    }

    /// Distance from the center within which a press grabs the knob.
    pub(in crate::dial) fn grab_radius(&self) -> f32 {
        self.px(GRAB_RADIUS_DP)
    }

    /// Point at `radius_dp` from the center in direction `angle` (degrees from up, clockwise).
    pub(in crate::dial) fn polar(&self, angle: f32, radius_dp: f32) -> (f32, f32) {
        let (cx, cy) = self.center();
        let (sin, cos) = angle.to_radians().sin_cos();
        let r = self.px(radius_dp);
        (cx + r * sin, cy - r * cos)
    }

    /// Center of the label of `percent`, outside the ticks.
    pub(in crate::dial) fn label_center(&self, percent: u32) -> (f32, f32) {
        self.polar(angle_of_percent(percent as f32), LABEL_RADIUS_DP)
    }

    /// Horizontal center and text baseline of the readout.
    pub(in crate::dial) fn readout(&self) -> (f32, f32) {
        (self.px(WIDTH_DP / 2.0), self.px(READOUT_BASELINE_DP))
    }

    /// Horizontal center and text baseline of the note under the readout.
    pub(in crate::dial) fn note(&self) -> (f32, f32) {
        (self.px(WIDTH_DP / 2.0), self.px(NOTE_BASELINE_DP))
    }

    /// Left, top, width and height of the imitated title bar.
    pub(super) fn strip(&self) -> (f32, f32, f32, f32) {
        (
            self.px(MARGIN_DP),
            self.px(STRIP_TOP_DP),
            self.px(WIDTH_DP - 2.0 * MARGIN_DP),
            self.px(STRIP_HEIGHT_DP),
        )
    }

    /// Left edge of the three caption buttons.
    pub(super) fn caption_group_left(&self) -> f32 {
        let (x, _, w, _) = self.strip();
        x + w - self.px(CAPTION_CELL_DP * CAPTION_CELLS as f32)
    }

    pub(super) fn caption_cell(&self) -> f32 {
        self.px(CAPTION_CELL_DP)
    }
}
