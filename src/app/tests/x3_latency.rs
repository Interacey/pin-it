//! Cost of re-anchoring the pin in the event cycle of a resize: time from the resize to the moved pin.
//! Run with `cargo test x3_latency -- --ignored --nocapture`.

use std::time::Instant;

use super::pb_common::{NativeWin, shown_slot};
use super::support::{options, pump_ms, temp_dir};
use crate::app::Runtime;
use crate::config::Settings;
use crate::events::WinEvent;

const STEPS: i32 = 200;
const WIDTH_LOW: i32 = 300;
const WIDTH_HIGH: i32 = 440;

#[test]
#[ignore]
fn x3_latency_of_the_reanchor_after_a_resize() {
    let _g = crate::window_test_guard();
    let w = NativeWin::new(0, None);
    let dir = temp_dir("x3-latency");
    let rt = Runtime::start(&options(&dir, vec![w.0], &Settings::default())).unwrap();
    while shown_slot(w.0).is_none() {
        pump_ms(25);
    }
    let mut micros = Vec::new();
    for i in 0..STEPS {
        let width = if i % 2 == 0 { WIDTH_LOW } else { WIDTH_HIGH };
        let start = Instant::now();
        w.resize_to(width);
        rt.inject_event(WinEvent::Moved(w.0));
        micros.push(start.elapsed().as_micros());
        pump_ms(20);
    }
    micros.sort_unstable();
    println!(
        "resize + Moved event handled: median {} us, p95 {} us, max {} us",
        micros[micros.len() / 2],
        micros[micros.len() * 95 / 100],
        micros[micros.len() - 1]
    );
    drop(rt);
    drop(w);
    let _ = std::fs::remove_dir_all(dir);
}
