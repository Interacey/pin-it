use super::reparse::{guard_write_target, parent_dir, refuse_reparse};
use super::settings::Settings;
use std::io;
use std::path::{Path, PathBuf};

/// Writes settings atomically: temp file in the same directory, then replace.
///
/// Refuses to write through a symlink or junction (directory, target or temp file).
pub(crate) fn save_to(path: &Path, settings: &Settings) -> io::Result<()> {
    if let Some(dir) = parent_dir(path) {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = sibling_with_suffix(path, ".tmp");
    guard_write_target(path)?;
    refuse_reparse(&tmp)?;
    let json = serde_json::to_string_pretty(settings).map_err(io::Error::other)?;
    let res = std::fs::write(&tmp, json).and_then(|_| std::fs::rename(&tmp, path));
    if res.is_err() {
        // The write error is what the caller needs; a leftover temp file is harmless.
        let _ = std::fs::remove_file(&tmp);
    }
    res
}

fn sibling_with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}
