//! The state of the running app. `App` is also the context handed to every observer.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::SystemTime;

use super::crash_recovery::pins_path_beside;
use super::entry::Entry;
use super::observer::Subject;
use super::pin_observers::{self, PinEvent};
use super::runtime::RunOptions;
use super::settings_observers::{self, SettingsEvent};
use crate::Hwnd;
use crate::color::{ColorCache, system_accent};
use crate::config::Settings;
use crate::events::EventHooks;
use crate::hotkey::Hotkey;
use crate::pin_stack::PinStack;
use crate::tray::Tray;

/// The settings file and when it was last read or written.
pub(super) struct SettingsFile {
    pub(super) path: PathBuf,
    /// Modification time of the file at the last load or save.
    pub(super) mtime: Option<SystemTime>,
    /// Modification time of a version of the file that did not parse. While the file still has it,
    /// it is a hand edit in progress: it is neither read again nor overwritten by a save.
    pub(super) invalid_mtime: Option<SystemTime>,
}

impl SettingsFile {
    /// Whether `mtime` is a version of the file that was already read, written or judged invalid.
    pub(super) fn is_known(&self, mtime: Option<SystemTime>) -> bool {
        mtime == self.mtime || (mtime.is_some() && mtime == self.invalid_mtime)
    }
}

/// The global hotkey: whether the app should have one and which one is registered.
pub(super) struct HotkeyBinding {
    pub(super) wanted: bool,
    pub(super) active: Option<Hotkey>,
}

/// The Run-key autostart entry.
pub(super) struct AutostartSync {
    /// Keep the entry in sync with the settings at all.
    pub(super) enabled: bool,
    /// Whether the Run key holds our exe, as far as we know.
    pub(super) registered: bool,
}

pub(super) struct App {
    pub(super) hwnd: Hwnd,
    pub(super) settings: Settings,
    pub(super) settings_file: SettingsFile,
    pub(super) pins: PinStack<Hwnd>,
    /// Pinned windows that were topmost before we pinned them.
    pub(super) prior_topmost: HashSet<Hwnd>,
    /// Windows that left the pin list but did not take the release yet; the sweep retries them.
    pub(super) pending_release: HashSet<Hwnd>,
    /// Windows whose queued (asynchronous) unpin was accepted while they still had the topmost
    /// flag: the target applies it later. Not persisted; the request waits in the target's queue.
    pub(super) queued_release: HashSet<Hwnd>,
    /// Pinned windows whose thread is hung or busy; they stay pinned and are retried by the sweep.
    pub(super) hung: HashSet<Hwnd>,
    pub(super) entries: HashMap<Hwnd, Entry>,
    pub(super) colors: ColorCache,
    pub(super) system: [u8; 3],
    pub(super) hooks: Option<EventHooks>,
    pub(super) tray: Option<Rc<Tray>>,
    pub(super) hotkey: HotkeyBinding,
    pub(super) autostart: AutostartSync,
    pub(super) only: Option<Vec<Hwnd>>,
    /// The tray icon could not be added and is retried by the sweep; set to log only once.
    pub(super) tray_add_failing: bool,
    /// Last pinnable foreground window, for "Pin / unpin current window" (the taskbar is foreground then).
    pub(super) last_fg: Option<Hwnd>,
    pub(super) down: bool,
    pub(super) pin_events: Subject<PinEvent, App>,
    pub(super) settings_events: Subject<SettingsEvent, App>,
}

impl App {
    pub(super) fn new(
        hwnd: Hwnd,
        opts: &RunOptions,
        mut settings: Settings,
        settings_mtime: Option<SystemTime>,
    ) -> App {
        settings.sanitize();
        let registered = if opts.autostart {
            crate::autostart::is_registered_for_current_exe()
        } else {
            settings.autostart
        };
        let pin_events = Subject::new();
        pin_observers::subscribe_all(&pin_events, pins_path_beside(&opts.settings_path));
        let settings_events = Subject::new();
        settings_observers::subscribe_all(&settings_events);
        App {
            hwnd,
            settings,
            settings_file: SettingsFile {
                path: opts.settings_path.clone(),
                mtime: settings_mtime,
                invalid_mtime: None,
            },
            pins: PinStack::new(),
            prior_topmost: HashSet::new(),
            pending_release: HashSet::new(),
            queued_release: HashSet::new(),
            hung: HashSet::new(),
            entries: HashMap::new(),
            colors: ColorCache::new(),
            system: system_accent(),
            hooks: None,
            tray: None,
            hotkey: HotkeyBinding {
                wanted: opts.hotkey,
                active: None,
            },
            autostart: AutostartSync {
                enabled: opts.autostart,
                registered,
            },
            only: opts.only.clone(),
            tray_add_failing: false,
            last_fg: None,
            down: false,
            pin_events,
            settings_events,
        }
    }
}
