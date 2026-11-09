//! The data of the excluded apps window: how an entered name is checked and normalized, and the
//! list with its scroll position, selection, hover and the flash of a row. No Win32 here.

use super::layout::{Area, Layout};

/// Longest program name: a file name component is limited to 255 characters (UTF-16 units) by
/// NTFS and by the Win32 file APIs, so no executable can have a longer name. Also the limit of
/// the input field.
pub(crate) const MAX_NAME_LEN: usize = 255;
/// Appended to a name without any dot, so "chrome" means "chrome.exe".
const DEFAULT_EXTENSION: &str = ".exe";
/// Characters Windows does not allow in a file name, besides control characters.
const INVALID_NAME_CHARS: [char; 9] = ['\\', '/', ':', '*', '?', '"', '<', '>', '|'];
/// Wheel units of one notch.
const WHEEL_DELTA: f32 = 120.0;
/// How long the flash of a row lasts.
const FLASH_SECS: f32 = 0.6;

/// Why an entered name was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryError {
    Empty,
    /// More than `MAX_NAME_LEN` characters, counting the `.exe` that gets appended.
    TooLong,
    /// A path separator, one of `: * ? " < > |` or a control character.
    InvalidChar,
    /// Windows strips a trailing dot, so the name would not be the one entered.
    TrailingDot,
}

/// The name as it is stored: trimmed, lower case, `.exe` when it has no dot. The settings keep
/// lower case names and compare ignoring case.
pub(crate) fn normalize_entry(raw: &str) -> Result<String, EntryError> {
    let mut name = raw.trim().to_lowercase();
    if name.is_empty() {
        return Err(EntryError::Empty);
    }
    if name
        .chars()
        .any(|c| c.is_control() || INVALID_NAME_CHARS.contains(&c))
    {
        return Err(EntryError::InvalidChar);
    }
    if name.ends_with('.') {
        return Err(EntryError::TrailingDot);
    }
    if !name.contains('.') {
        name.push_str(DEFAULT_EXTENSION);
    }
    if name.encode_utf16().count() > MAX_NAME_LEN {
        return Err(EntryError::TooLong);
    }
    Ok(name)
}

/// What the pointer is over: a row, or the trash button of a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Hit {
    Row(usize),
    Trash(usize),
}

/// A row that was just added or entered again: it is shown and lights up for a moment.
#[derive(Debug, Clone, PartialEq)]
struct Flash {
    name: String,
    /// 1 at the start, 0 when over.
    level: f32,
    /// The row still has to be scrolled into view (it may not be in the list yet).
    reveal: bool,
}

/// The excluded programs as the window shows them.
#[derive(Debug, Clone)]
pub(super) struct ListState {
    layout: Layout,
    entries: Vec<String>,
    /// Pixels the content is scrolled up.
    scroll: f32,
    selected: Option<usize>,
    hover: Option<Hit>,
    flash: Option<Flash>,
}

impl ListState {
    pub(super) fn new(entries: &[String], layout: Layout) -> Self {
        Self {
            layout,
            entries: entries.to_vec(),
            scroll: 0.0,
            selected: None,
            hover: None,
            flash: None,
        }
    }

    pub(super) fn entries(&self) -> &[String] {
        &self.entries
    }

    pub(super) fn contains(&self, name: &str) -> bool {
        self.entries.iter().any(|e| e == name)
    }

    pub(super) fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub(super) fn selected_name(&self) -> Option<&str> {
        self.selected.map(|i| self.entries[i].as_str())
    }

    pub(super) fn hover(&self) -> Option<Hit> {
        self.hover
    }

    #[cfg(test)]
    pub(super) fn scroll(&self) -> f32 {
        self.scroll
    }

    fn viewport_height(&self) -> f32 {
        self.layout.list().h
    }

    fn content_height(&self) -> f32 {
        self.entries.len() as f32 * self.layout.row_height()
    }

    fn max_scroll(&self) -> f32 {
        (self.content_height() - self.viewport_height()).max(0.0)
    }

    /// More rows than the list shows: the scroll indicator appears.
    pub(super) fn overflows(&self) -> bool {
        self.max_scroll() > 0.0
    }

    /// The area of row `index` as it is on screen now.
    pub(super) fn row_area(&self, index: usize) -> Area {
        let top = self.layout.list().y + index as f32 * self.layout.row_height() - self.scroll;
        self.layout.row_at(top)
    }

    /// The rows that are at least partly inside the list.
    pub(super) fn visible(&self) -> std::ops::Range<usize> {
        let row = self.layout.row_height();
        let first = (self.scroll / row).floor() as usize;
        let last = ((self.scroll + self.viewport_height()) / row).ceil() as usize;
        first.min(self.entries.len())..last.min(self.entries.len())
    }

    pub(super) fn scroll_by(&mut self, pixels: f32) {
        self.scroll = (self.scroll + pixels).clamp(0.0, self.max_scroll());
    }

    /// A wheel turn of `delta` (`WHEEL_DELTA` per notch, positive away from the user, which
    /// moves the content down): one row per notch.
    pub(super) fn scroll_wheel(&mut self, delta: i32) {
        self.scroll_by(-(delta as f32) / WHEEL_DELTA * self.layout.row_height());
    }

    /// Scrolls just far enough that row `index` is fully inside the list.
    pub(super) fn reveal(&mut self, index: usize) {
        let row = self.layout.row_height();
        let top = index as f32 * row;
        if top < self.scroll {
            self.scroll_by(top - self.scroll);
        } else if top + row > self.scroll + self.viewport_height() {
            self.scroll_by(top + row - self.scroll - self.viewport_height());
        }
    }

    /// Moves the selection by `delta` rows, starting at the first (down) or last (up) row when
    /// nothing is selected, and keeps it inside the list.
    pub(super) fn select_step(&mut self, delta: i32) {
        let Some(last) = self.entries.len().checked_sub(1) else {
            return;
        };
        let next = match self.selected {
            Some(i) => (i as i64 + i64::from(delta)).clamp(0, last as i64) as usize,
            None if delta < 0 => last,
            None => 0,
        };
        self.selected = Some(next);
        self.reveal(next);
    }

    pub(super) fn select(&mut self, index: usize) {
        if index < self.entries.len() {
            self.selected = Some(index);
        }
    }

    pub(super) fn clear_selection(&mut self) {
        self.selected = None;
    }

    /// Sets the hover; true when it changed.
    pub(super) fn set_hover(&mut self, hover: Option<Hit>) -> bool {
        std::mem::replace(&mut self.hover, hover) != hover
    }

    /// What is under the client point `at`, if anything.
    pub(super) fn hit(&self, at: (i32, i32)) -> Option<Hit> {
        let (x, y) = (at.0 as f32, at.1 as f32);
        if !self.layout.list().contains(x, y) {
            return None;
        }
        let index = ((y - self.layout.list().y + self.scroll) / self.layout.row_height()) as usize;
        if index >= self.entries.len() {
            return None;
        }
        let row = self.row_area(index);
        if !row.contains(x, y) {
            return None;
        }
        if self.layout.trash_box(&row).contains(x, y) {
            Some(Hit::Trash(index))
        } else {
            Some(Hit::Row(index))
        }
    }

    /// Takes the list as the settings have it now. The selection stays on its name, or on the
    /// row that took its place; the scroll position stays valid.
    pub(super) fn refresh(&mut self, entries: &[String]) {
        let kept = self.selected_name().map(str::to_owned);
        let old_index = self.selected;
        self.entries = entries.to_vec();
        let last = self.entries.len().checked_sub(1);
        self.selected = kept
            .and_then(|name| self.entries.iter().position(|e| *e == name))
            .or(old_index.zip(last).map(|(i, last)| i.min(last)));
        self.hover = None;
        self.scroll_by(0.0);
        self.reveal_flashed_row();
    }

    /// Lights up the row of `name` for a moment and scrolls to it, now or when it arrives.
    pub(super) fn highlight(&mut self, name: &str) {
        self.flash = Some(Flash {
            name: name.to_owned(),
            level: 1.0,
            reveal: true,
        });
        self.reveal_flashed_row();
    }

    fn reveal_flashed_row(&mut self) {
        let Some(flash) = self.flash.as_mut().filter(|f| f.reveal) else {
            return;
        };
        if let Some(index) = self.entries.iter().position(|e| *e == flash.name) {
            flash.reveal = false;
            self.reveal(index);
        }
    }

    /// How much row `index` is lit, from 1 down to 0.
    pub(super) fn flash_level(&self, index: usize) -> f32 {
        self.flash
            .as_ref()
            .filter(|f| self.entries.get(index) == Some(&f.name))
            .map_or(0.0, |f| f.level)
    }

    pub(super) fn is_animating(&self) -> bool {
        self.flash.is_some()
    }

    /// Advances the flash by `dt` seconds; true while it is still going.
    pub(super) fn tick(&mut self, dt: f32) -> bool {
        if let Some(flash) = &mut self.flash {
            flash.level -= dt / FLASH_SECS;
            if flash.level <= 0.0 {
                self.flash = None;
            }
        }
        self.is_animating()
    }

    /// The scroll indicator's handle, only when the list overflows.
    pub(super) fn scrollbar_thumb(&self) -> Option<Area> {
        if !self.overflows() {
            return None;
        }
        let track = self.layout.scrollbar_track();
        let share = self.viewport_height() / self.content_height();
        let height = (track.h * share).max(self.layout.scrollbar_min_thumb());
        let travel = track.h - height;
        let top = track.y + travel * self.scroll / self.max_scroll();
        Some(Area {
            y: top,
            h: height,
            ..track
        })
    }
}

#[cfg(test)]
mod tests;
