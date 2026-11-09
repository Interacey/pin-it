use super::backup::{backup_copy, backup_corrupt};
use super::settings::Settings;
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

pub(super) const MAX_FILE_BYTES: u64 = 1 << 20;
const UTF8_BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];
const UTF16_LE_BOM: [u8; 2] = [0xFF, 0xFE];
const UTF16_BE_BOM: [u8; 2] = [0xFE, 0xFF];

/// Outcome of reading the settings file.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum LoadStatus {
    /// No file; defaults are used.
    Missing,
    /// Parsed without problems.
    Ok,
    /// The file could not be read (locked, access denied); it is left untouched and defaults are used.
    IoError(String),
    /// Not usable JSON; moved to `backup` (None if the move failed) and defaults are used.
    Corrupt { backup: Option<PathBuf> },
    /// Parsed, but these fields (or list elements, as `name[index]` / `name.key`) held invalid
    /// values and were dropped or fell back to their defaults. The original file was copied to
    /// `backup` (None if the copy failed) because the next save rewrites it without them.
    Lenient {
        fixed_fields: Vec<String>,
        backup: Option<PathBuf>,
    },
}

type JsonObject = serde_json::Map<String, serde_json::Value>;

/// Loads settings, ignoring the status; see `load_report_from`.
#[cfg(test)]
pub(crate) fn load_from(path: &Path) -> Settings {
    load_report_from(path).0
}

/// Loads settings and says what happened.
///
/// A missing file gives defaults. A read error keeps the file. A file that is not a JSON object
/// (or is larger than 1 MiB) is moved to `settings.json.bad-<unix-seconds>`, keeping the newest 3.
/// A UTF-8 BOM and UTF-16 (with BOM) are accepted. Invalid single fields and single list elements
/// are dropped individually; the original is then copied to the same kind of backup file.
pub(crate) fn load_report_from(path: &Path) -> (Settings, LoadStatus) {
    let bytes = match read_capped(path) {
        Ok(Some(b)) => b,
        Ok(None) => return corrupt(path),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return (Settings::default(), LoadStatus::Missing);
        }
        Err(e) => return (Settings::default(), LoadStatus::IoError(e.to_string())),
    };
    let Some(obj) = parse_json_object(&bytes) else {
        return corrupt(path);
    };
    let (mut settings, fixed_fields) = settings_from_object(&obj);
    settings.sanitize();
    let status = if fixed_fields.is_empty() {
        LoadStatus::Ok
    } else {
        LoadStatus::Lenient {
            backup: backup_copy(path),
            fixed_fields,
        }
    };
    (settings, status)
}

fn corrupt(path: &Path) -> (Settings, LoadStatus) {
    let backup = backup_corrupt(path);
    (Settings::default(), LoadStatus::Corrupt { backup })
}

/// Reads at most 1 MiB; `Ok(None)` means the file is larger.
fn read_capped(path: &Path) -> io::Result<Option<Vec<u8>>> {
    use std::io::Read;
    let mut buf = Vec::new();
    std::fs::File::open(path)?
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut buf)?;
    Ok((buf.len() as u64 <= MAX_FILE_BYTES).then_some(buf))
}

fn parse_json_object(bytes: &[u8]) -> Option<JsonObject> {
    let text = decode_text(bytes)?;
    match serde_json::from_str::<serde_json::Value>(&text).ok()? {
        serde_json::Value::Object(obj) => Some(obj),
        _ => None,
    }
}

fn decode_text(bytes: &[u8]) -> Option<String> {
    if let Some(rest) = bytes.strip_prefix(&UTF16_LE_BOM) {
        decode_utf16(rest, u16::from_le_bytes)
    } else if let Some(rest) = bytes.strip_prefix(&UTF16_BE_BOM) {
        decode_utf16(rest, u16::from_be_bytes)
    } else {
        let rest = bytes.strip_prefix(&UTF8_BOM).unwrap_or(bytes);
        String::from_utf8(rest.to_vec()).ok()
    }
}

fn decode_utf16(data: &[u8], to_unit: fn([u8; 2]) -> u16) -> Option<String> {
    if !data.len().is_multiple_of(2) {
        return None;
    }
    let units: Vec<u16> = data
        .chunks_exact(2)
        .map(|pair| to_unit([pair[0], pair[1]]))
        .collect();
    String::from_utf16(&units).ok()
}

/// Copies each field that parses; names of fields with invalid values are returned.
fn settings_from_object(obj: &JsonObject) -> (Settings, Vec<String>) {
    let mut s = Settings::default();
    let mut fixed = Vec::new();
    macro_rules! field {
        ($($name:ident),*) => {$(
            if let Some(v) = obj.get(stringify!($name)) {
                match serde_json::from_value(v.clone()) {
                    Ok(x) => s.$name = x,
                    Err(_) => fixed.push(stringify!($name).to_string()),
                }
            }
        )*};
    }
    field!(
        version,
        enabled,
        hotkey,
        use_window_color,
        fixed_accent,
        animate,
        autostart,
        button_scale_percent,
        dial_click_sound
    );
    if let Some(v) = obj.get("excluded_apps") {
        s.excluded_apps = lenient_list(v, "excluded_apps", &mut fixed);
    }
    if let Some(v) = obj.get("per_app_accent") {
        s.per_app_accent = lenient_map(v, "per_app_accent", &mut fixed);
    }
    (s, fixed)
}

/// Keeps the elements of a JSON array that parse; the others are reported as `name[index]`.
fn lenient_list<T: serde::de::DeserializeOwned>(
    value: &serde_json::Value,
    name: &str,
    fixed: &mut Vec<String>,
) -> Vec<T> {
    let Some(items) = value.as_array() else {
        fixed.push(name.to_string());
        return Vec::new();
    };
    let mut kept = Vec::new();
    for (index, item) in items.iter().enumerate() {
        match serde_json::from_value(item.clone()) {
            Ok(x) => kept.push(x),
            Err(_) => fixed.push(format!("{name}[{index}]")),
        }
    }
    kept
}

/// Keeps the entries of a JSON object that parse; the others are reported as `name.key`.
fn lenient_map<T: serde::de::DeserializeOwned>(
    value: &serde_json::Value,
    name: &str,
    fixed: &mut Vec<String>,
) -> HashMap<String, T> {
    let Some(entries) = value.as_object() else {
        fixed.push(name.to_string());
        return HashMap::new();
    };
    let mut kept = HashMap::new();
    for (key, item) in entries {
        match serde_json::from_value(item.clone()) {
            Ok(x) => {
                kept.insert(key.clone(), x);
            }
            Err(_) => fixed.push(format!("{name}.{key}")),
        }
    }
    kept
}
