//! Settings struct and JSON load/save.
//!
//! - `settings`: the `Settings` type and its rules
//! - `paths`: where the file lives and when it changed
//! - `load`: lenient reading, one bad field never resets the rest
//! - `backup`: moving a corrupt file aside (startup) or copying it (reload) and pruning old backups
//! - `reparse`: refusing to follow symlinks and junctions
//! - `save`: atomic writing

mod backup;
mod load;
mod paths;
mod reparse;
mod save;
mod settings;

#[cfg(test)]
pub(crate) use load::load_from;
pub(crate) use load::{LoadStatus, load_report_from, reload_report_from};
pub(crate) use paths::{default_path, modified_time};
pub(crate) use reparse::guard_write_target;
pub(crate) use save::save_to;
pub(crate) use settings::{Settings, clamp_button_scale};

#[cfg(test)]
mod tests;
