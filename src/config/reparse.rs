use std::io;
use std::path::Path;

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

/// True if the path exists and is a symlink or junction (any reparse point).
pub(super) fn is_reparse_point(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    std::fs::symlink_metadata(path)
        .map(|m| m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0)
        .unwrap_or(false)
}

pub(super) fn refuse_reparse(path: &Path) -> io::Result<()> {
    if is_reparse_point(path) {
        return Err(io::Error::other(format!(
            "refusing to touch reparse point {}",
            path.display()
        )));
    }
    Ok(())
}

/// Fails when the file or its directory is a symlink or junction; for anything that writes a
/// file the app owns (settings, log, crash list) so it cannot be redirected elsewhere.
pub(crate) fn guard_write_target(path: &Path) -> io::Result<()> {
    if let Some(dir) = parent_dir(path) {
        refuse_reparse(dir)?;
    }
    refuse_reparse(path)
}

/// The parent directory, or `None` for a bare file name.
pub(super) fn parent_dir(path: &Path) -> Option<&Path> {
    path.parent().filter(|d| !d.as_os_str().is_empty())
}
