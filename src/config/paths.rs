use std::path::{Path, PathBuf};
use std::time::SystemTime;

const APP_DIR_NAME: &str = "PinIt";
const FILE_NAME: &str = "settings.json";

/// Returns `%APPDATA%\PinIt\settings.json`; never a relative path.
pub(crate) fn default_path() -> PathBuf {
    roaming_appdata()
        .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))
        .unwrap_or_else(std::env::temp_dir)
        .join(APP_DIR_NAME)
        .join(FILE_NAME)
}

fn roaming_appdata() -> Option<PathBuf> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{
        FOLDERID_RoamingAppData, KF_FLAG_DEFAULT, SHGetKnownFolderPath,
    };
    // SAFETY: the returned string is copied and then freed with CoTaskMemFree as the API requires.
    unsafe {
        let p = SHGetKnownFolderPath(&FOLDERID_RoamingAppData, KF_FLAG_DEFAULT, None).ok()?;
        let s = p.to_string().ok().filter(|s| !s.is_empty());
        CoTaskMemFree(Some(p.0.cast()));
        s.map(PathBuf::from)
    }
}

/// Last write time of the settings file, to detect hand edits.
pub(crate) fn modified_time(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}
