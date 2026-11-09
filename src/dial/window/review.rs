//! Writes pictures of the dial window, with its texts, for a visual check. Runs only when
//! `PIN_IT_DIAL_PNG_DIR` names a directory; no window is created and nothing is shown.

use std::path::Path;

use tiny_skia::{IntSize, Pixmap, PixmapPaint, Transform};

use super::Request;
use super::ui::Ui;
use crate::dial::model::angle_of_percent;
use crate::dial::state::Key;
use crate::popup::Theme;

const FILMSTRIP_FRAMES: u32 = 8;
const FILMSTRIP_COLUMNS: u32 = 4;
const FRAME_SECS: f32 = 0.015;
const POINTER_RADIUS_DP: f32 = 80.0;

fn ui_at(percent: u32, dpi: u32, theme: Theme) -> Ui {
    let request = Request {
        notify: 0,
        message: 0,
        percent,
        click_sound: false,
    };
    Ui::with_theme(request, dpi, theme).expect("ui")
}

/// The window as it is drawn right now, as a pixmap.
fn picture(ui: &mut Ui) -> Pixmap {
    ui.redraw();
    let (w, h) = (ui.canvas.width, ui.canvas.height);
    Pixmap::from_vec(ui.canvas.read_rgba(), IntSize::from_wh(w, h).expect("size")).expect("pixmap")
}

fn save(pm: &Pixmap, dir: &Path, name: &str) {
    pm.save_png(dir.join(format!("{name}.png")))
        .expect("save png");
}

/// Presses the knob at 100 percent and drags the pointer to `angle` degrees.
fn drag_to(ui: &mut Ui, angle: f32) {
    let r = ui.layout.px(POINTER_RADIUS_DP);
    let at = |a: f32| (r * a.to_radians().sin(), -r * a.to_radians().cos());
    let (x, y) = at(angle_of_percent(100.0));
    ui.dial.press(x, y);
    let (x, y) = at(angle);
    ui.dial.drag_to(x, y);
}

fn filmstrip(dpi: u32, theme: Theme) -> Pixmap {
    let mut ui = ui_at(100, dpi, theme);
    ui.dial.key(Key::Move(crate::dial::model::Step::Next));
    let frames: Vec<Pixmap> = (0..FILMSTRIP_FRAMES)
        .map(|_| {
            let frame = picture(&mut ui);
            ui.dial.tick(FRAME_SECS);
            frame
        })
        .collect();
    let (w, h) = (frames[0].width(), frames[0].height());
    let rows = FILMSTRIP_FRAMES.div_ceil(FILMSTRIP_COLUMNS);
    let mut sheet = Pixmap::new(w * FILMSTRIP_COLUMNS, h * rows).expect("sheet");
    for (i, frame) in frames.iter().enumerate() {
        let (col, row) = (i as u32 % FILMSTRIP_COLUMNS, i as u32 / FILMSTRIP_COLUMNS);
        sheet.draw_pixmap(
            (col * w) as i32,
            (row * h) as i32,
            frame.as_ref(),
            &PixmapPaint::default(),
            Transform::identity(),
            None,
        );
    }
    sheet
}

#[test]
fn write_pictures_for_review() {
    let Some(dir) = std::env::var_os("PIN_IT_DIAL_PNG_DIR") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).expect("png dir");
    for (name, theme) in [("dark", Theme::DARK), ("light", Theme::LIGHT)] {
        for (dpi, dpi_name) in [(96, "100"), (192, "200")] {
            let prefix = format!("{name}-{dpi_name}");
            for percent in [50, 100, 125, 150] {
                save(
                    &picture(&mut ui_at(percent, dpi, theme)),
                    &dir,
                    &format!("{prefix}-v{percent}"),
                );
            }
            let mut ui = ui_at(100, dpi, theme);
            drag_to(&mut ui, 7.0);
            save(&picture(&mut ui), &dir, &format!("{prefix}-middrag"));
            let mut ui = ui_at(100, dpi, theme);
            ui.dial.key(Key::Move(crate::dial::model::Step::Next));
            ui.dial.tick(FRAME_SECS);
            save(&picture(&mut ui), &dir, &format!("{prefix}-flash"));
            save(&filmstrip(dpi, theme), &dir, &format!("{prefix}-filmstrip"));
        }
    }
}
