use super::reparse::{guard_write_target, is_reparse_point};
use std::path::{Path, PathBuf};

const KEEP_BACKUPS: usize = 3;
const MAX_NAME_COLLISIONS: u32 = 100;

/// A backup file next to the settings file.
type Backup = (u64, String, PathBuf);

/// Moves a corrupt file to `<name>.bad-<unix-seconds>` and prunes old backups; returns the new path.
pub(super) fn backup_corrupt(path: &Path) -> Option<PathBuf> {
    guard_write_target(path).ok()?;
    let dest = free_backup_name(path)?;
    std::fs::rename(path, &dest).ok()?;
    prune_backups(path);
    Some(dest)
}

/// Copies a file that loaded with dropped values to `<name>.bad-<unix-seconds>` before a save
/// overwrites it, and prunes old backups; returns the copy's path.
pub(super) fn backup_copy(path: &Path) -> Option<PathBuf> {
    guard_write_target(path).ok()?;
    let dest = free_backup_name(path)?;
    std::fs::copy(path, &dest).ok()?;
    prune_backups(path);
    Some(dest)
}

fn free_backup_name(path: &Path) -> Option<PathBuf> {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    (0..MAX_NAME_COLLISIONS).find_map(|n| {
        let suffix = if n == 0 {
            format!(".bad-{secs}")
        } else {
            format!(".bad-{secs}-{n}")
        };
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        let candidate = PathBuf::from(name);
        (!candidate.exists()).then_some(candidate)
    })
}

fn prune_backups(path: &Path) {
    let mut found = list_backups(path);
    found.sort();
    let excess = found.len().saturating_sub(KEEP_BACKUPS);
    for (_, _, old) in found.into_iter().take(excess) {
        if !is_reparse_point(&old) {
            // Best effort: a backup that cannot be deleted only stays around.
            let _ = std::fs::remove_file(old);
        }
    }
}

fn list_backups(path: &Path) -> Vec<Backup> {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return Vec::new();
    };
    let prefix = format!("{}.bad-", name.to_string_lossy());
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|e| {
            let file_name = e.file_name().to_string_lossy().into_owned();
            let rest = file_name.strip_prefix(&prefix)?;
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            Some((digits.parse().ok()?, file_name, e.path()))
        })
        .collect()
}
