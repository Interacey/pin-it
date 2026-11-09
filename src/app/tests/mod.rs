//! Integration tests for `app`: own windows only, no input injection, no foreign windows.
//! Clicks are posted messages to the overlay windows; everything runs under the window test lock.

mod d3_egg;
mod dial_changed;
mod dial_window;
mod exclusions_changed;
mod exclusions_window;
mod f3_firefox;
mod f3_runtime;
mod lifecycle;
mod observers;
mod p2_colors;
mod p2_overlap;
mod p2_vanish;
mod pb_common;
mod pb_leak;
mod pb_maximize;
mod pin_flow;
mod pin_safety;
mod recovery;
mod settings;
mod support;
mod t2_tray_commands;
mod tray;
mod x3_fade;
mod x3_latency;
mod x_drag;
mod x_vanish;
