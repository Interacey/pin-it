//! Log file `pinit.log` next to the settings: one line per entry, trimmed when it grows too big.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::guard_write_target;

const LOG_MAX_BYTES: u64 = 256 * 1024;
const LOG_KEEP_BYTES: usize = 128 * 1024;
const LOG_FILE_NAME: &str = "pinit.log";

const SECS_PER_DAY: u64 = 86_400;
const SECS_PER_HOUR: u64 = 3600;
const SECS_PER_MINUTE: u64 = 60;
/// Days from 0000-03-01 to 1970-01-01, the shift used by the civil-from-days algorithm.
const DAYS_FROM_CIVIL_EPOCH_TO_UNIX: i64 = 719_468;
const DAYS_PER_ERA: i64 = 146_097;

static LOG_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Points the log at `pinit.log` next to the given settings file.
pub(super) fn set_log_path_beside(settings_path: &Path) {
    *LOG_PATH.lock().unwrap_or_else(|e| e.into_inner()) =
        Some(settings_path.with_file_name(LOG_FILE_NAME));
}

/// `YYYY-MM-DD HH:MM:SS` in UTC for a Unix timestamp.
fn format_utc(unix_secs: u64) -> String {
    let (year, month, day) = civil_from_days((unix_secs / SECS_PER_DAY) as i64);
    let secs_of_day = unix_secs % SECS_PER_DAY;
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}",
        secs_of_day / SECS_PER_HOUR,
        secs_of_day % SECS_PER_HOUR / SECS_PER_MINUTE,
        secs_of_day % SECS_PER_MINUTE
    )
}

/// Howard Hinnant's civil-from-days algorithm; `days` counts from 1970-01-01.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + DAYS_FROM_CIVIL_EPOCH_TO_UNIX;
    let era = z.div_euclid(DAYS_PER_ERA);
    let day_of_era = z.rem_euclid(DAYS_PER_ERA);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// Keeps the last `keep` bytes of `text`, starting at a line boundary when there is one.
fn trim_log_text(text: &[u8], keep: usize) -> &[u8] {
    if text.len() <= keep {
        return text;
    }
    let tail = &text[text.len() - keep..];
    match tail.iter().position(|&b| b == b'\n') {
        Some(i) => &tail[i + 1..],
        None => tail,
    }
}

/// Appends `line`; a failing log must never disturb the app, so I/O errors are dropped here.
/// A log file or directory that is a symlink or junction is not written (and the refusal cannot
/// be logged: there is nowhere safe to put it).
fn append_log(path: &Path, line: &str) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if guard_write_target(path).is_err() {
        return;
    }
    trim_if_too_big(path);
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path);
    if let Ok(mut file) = file {
        let _ = file.write_all(line.as_bytes());
    }
}

fn trim_if_too_big(path: &Path) {
    let too_big = std::fs::metadata(path).is_ok_and(|m| m.len() > LOG_MAX_BYTES);
    if !too_big {
        return;
    }
    if let Ok(old) = std::fs::read(path) {
        let _ = std::fs::write(path, trim_log_text(&old, LOG_KEEP_BYTES));
    }
}

pub(super) fn log(level: &str, msg: &str) {
    let path = LOG_PATH.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let Some(path) = path else { return };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    append_log(&path, &format!("{} {level} {msg}\n", format_utc(now)));
}

pub(super) fn info(msg: &str) {
    log("INFO", msg);
}

pub(super) fn warn(msg: &str) {
    log("WARN", msg);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_format_known_values() {
        assert_eq!(format_utc(0), "1970-01-01 00:00:00");
        assert_eq!(format_utc(1_000_000_000), "2001-09-09 01:46:40");
        assert_eq!(format_utc(1_709_164_800 + 86_399), "2024-02-29 23:59:59");
        assert_eq!(format_utc(4_102_444_800), "2100-01-01 00:00:00");
    }

    #[test]
    fn trim_keeps_tail_from_line_start() {
        let text = b"aaaa\nbbbb\ncccc\n";
        assert_eq!(trim_log_text(text, 100), text);
        assert_eq!(trim_log_text(text, 7), b"cccc\n");
        assert_eq!(trim_log_text(b"abcdefgh", 3), b"fgh");
    }

    #[test]
    fn log_is_not_written_through_a_junction() {
        let dir = std::env::temp_dir().join(format!("pinit-log-junction-{}", std::process::id()));
        let real = dir.join("real");
        let link = dir.join("link");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&real).unwrap();
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&real)
            .output()
            .is_ok_and(|o| o.status.success());
        assert!(made, "mklink /J failed");
        append_log(
            &link.join("pinit.log"),
            "through the junction
",
        );
        assert!(!real.join("pinit.log").exists());
        append_log(
            &real.join("pinit.log"),
            "direct
",
        );
        assert!(real.join("pinit.log").exists());
        let _ = std::fs::remove_dir(&link);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn log_file_is_trimmed_below_limit() {
        let dir = std::env::temp_dir().join(format!("pinit-log-{}", std::process::id()));
        let path = dir.join("pinit.log");
        let _ = std::fs::remove_dir_all(&dir);
        let line = "x".repeat(99) + "\n";
        for _ in 0..(LOG_MAX_BYTES as usize / 100 + 50) {
            append_log(&path, &line);
        }
        let len = std::fs::metadata(&path).unwrap().len();
        assert!(len <= LOG_MAX_BYTES + 100, "len {len}");
        assert!(len > 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
