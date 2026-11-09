//! GDI text on the dial: the readout and the labels of the scale.

use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Gdi::{
    DT_CENTER, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, FW_NORMAL, FW_SEMIBOLD, HDC, SetTextAlign,
    TA_LEFT, TA_TOP,
};

use super::model::{CENTER_PERCENT, MAX_PERCENT, MIN_PERCENT};
use super::render::Layout;
use super::size::Readout;
use crate::popup::Theme;
use crate::popup::text::{Font, draw_at_baseline, draw_in_rect, set_color, text_width, utf16};

/// Pixel height of the big percent figure and of the small texts, in dp.
const READOUT_FONT_DP: f32 = 28.0;
const SMALL_FONT_DP: f32 = 13.0;
/// Space between the percent figure and the size in dp.
const READOUT_GAP_DP: f32 = 8.0;
/// Box of a scale label around its center.
const LABEL_HALF_WIDTH_DP: f32 = 24.0;
const LABEL_HALF_HEIGHT_DP: f32 = 10.0;
/// The labels of the scale: the ends, the big ticks and the middle. The middle one is stronger.
const LABELS: [u32; 5] = [MIN_PERCENT, 75, CENTER_PERCENT, 125, MAX_PERCENT];
/// Sizes of the readout figure during a pulse, in steps from rest to the peak.
const PULSE_STEPS: [f32; 3] = [1.0, 1.04, 1.08];

/// The fonts of the window; the big one exists once per pulse step, so a pulse only picks one.
pub(super) struct Fonts {
    big: [Font; PULSE_STEPS.len()],
    small: Font,
}

impl Fonts {
    pub(super) fn new(scale: f32) -> windows::core::Result<Self> {
        let weight = FW_SEMIBOLD.0 as i32;
        Ok(Self {
            big: [
                Font::new(READOUT_FONT_DP * PULSE_STEPS[0], scale, weight)?,
                Font::new(READOUT_FONT_DP * PULSE_STEPS[1], scale, weight)?,
                Font::new(READOUT_FONT_DP * PULSE_STEPS[2], scale, weight)?,
            ],
            small: Font::new(SMALL_FONT_DP, scale, FW_NORMAL.0 as i32)?,
        })
    }

    /// The big font closest to a readout scale of `pulse` (1 at rest, 1.08 at the peak).
    fn big_for(&self, pulse: f32) -> &Font {
        let nearest = PULSE_STEPS
            .iter()
            .enumerate()
            .min_by(|a, b| (a.1 - pulse).abs().total_cmp(&(b.1 - pulse).abs()))
            .map_or(0, |(i, _)| i);
        &self.big[nearest]
    }
}

/// Everything needed to draw the texts of one frame onto `dc`.
pub(super) struct TextPass<'a> {
    pub(super) dc: HDC,
    pub(super) fonts: &'a Fonts,
    pub(super) layout: Layout,
    pub(super) theme: Theme,
}

impl TextPass<'_> {
    /// "125 %" in the text color and "(24 dp)" in the muted one, centered as one line, and the
    /// note about the title bar limit, when there is one, centered below. `pulse` scales the
    /// percent figure.
    pub(super) fn readout(&self, readout: &Readout, pulse: f32) {
        let big = self.fonts.big_for(pulse);
        let (percent, size) = (utf16(&readout.percent), utf16(&readout.size));
        let gap = self.layout.scale * READOUT_GAP_DP;
        let width_percent = text_width(self.dc, big, &percent);
        let width_size = text_width(self.dc, &self.fonts.small, &size);
        let (center, baseline) = self.layout.readout();
        let left = center - (width_percent + gap + width_size) / 2.0;
        set_color(self.dc, self.theme.text);
        draw_at_baseline(self.dc, big, (left, baseline), &percent);
        set_color(self.dc, self.theme.muted);
        let size_left = left + width_percent + gap;
        draw_at_baseline(self.dc, &self.fonts.small, (size_left, baseline), &size);
        if let Some(note) = readout.note {
            self.note(note);
        }
    }

    fn note(&self, note: &str) {
        let note = utf16(note);
        let width = text_width(self.dc, &self.fonts.small, &note);
        let (center, baseline) = self.layout.note();
        set_color(self.dc, self.theme.muted);
        draw_at_baseline(
            self.dc,
            &self.fonts.small,
            (center - width / 2.0, baseline),
            &note,
        );
    }

    /// 60, 75, 100, 125 and 140 outside the ticks, at the angle of each.
    pub(super) fn scale_labels(&self) {
        reset_text_align(self.dc);
        for percent in LABELS {
            let color = if percent == CENTER_PERCENT {
                self.theme.text
            } else {
                self.theme.muted
            };
            set_color(self.dc, color);
            let (x, y) = self.layout.label_center(percent);
            let (half_w, half_h) = (
                self.layout.scale * LABEL_HALF_WIDTH_DP,
                self.layout.scale * LABEL_HALF_HEIGHT_DP,
            );
            let rect = RECT {
                left: (x - half_w).round() as i32,
                top: (y - half_h).round() as i32,
                right: (x + half_w).round() as i32,
                bottom: (y + half_h).round() as i32,
            };
            let format = DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX;
            draw_in_rect(
                self.dc,
                &self.fonts.small,
                rect,
                &percent.to_string(),
                format,
            );
        }
    }
}

/// The readout draws at its baseline and leaves that alignment in the DC, which would lift
/// `DrawText` by the font height on the next frame; the labels start from the default again.
fn reset_text_align(dc: HDC) {
    // SAFETY: `dc` is a live memory DC.
    unsafe {
        SetTextAlign(dc, TA_TOP | TA_LEFT);
    }
}
