# Pin It

Pin It is a background process for Windows 11 (no console window, not a service). It puts a small pin button in the title bar of other programs' windows. Clicking the button makes the window always on top and brings it to the front with keyboard focus.

## Download (Windows)

Each release on GitHub has two files:

- `PinIt-Setup.exe`: the installer. It installs for the current user without administrator rights into `%LOCALAPPDATA%\Programs\Pin It`, adds a Start menu entry, and lists Pin It under Settings > Apps > Installed apps, where it can be uninstalled. A running Pin It is closed first, so an upgrade over an existing install works. The uninstaller asks whether the settings and the log in `%APPDATA%\PinIt` should be deleted; the default is No.
- `PinIt.exe`: the same program as a single file. It needs no installation and can be run from any folder.

The files `PinIt-Setup.exe.sha256` and `PinIt.exe.sha256` next to them allow verifying a download. The programs are not code-signed, so Windows SmartScreen warns about them; choose "More info" > "Run anyway".

Pin It does not start with Windows by default. Turn on "Start with Windows" in the tray menu; the installer does not set this. Settings, log and pins are stored in `%APPDATA%\PinIt` for both files (see "Files in `%APPDATA%\PinIt`" below).

To publish a release, set `version` in `Cargo.toml`, commit it, then push a tag with the same number: `git tag v0.1.0 && git push origin v0.1.0`. The `release.yml` workflow builds the exe and the installer and attaches both, with their checksums, to the release.

## Behavior

- Clicking the button, pressing the hotkey, or the tray command "Pin / unpin current window" pins or unpins a window. A new pin is activated (foreground and keyboard focus) once the window is topmost. If the system refuses the topmost flag at first and the same action settles it, the window is activated too. If it is still refused or hung, it is not activated, and neither is it when the 2 second sweep settles it later. Unpinning never activates.
- The newest pin is above older pins. Windows that are not pinned stay behind the oldest pin.
- Pinned windows that lose the topmost flag are pinned again by the sweep (every 2 seconds).

### Button

- Only the pin glyph is drawn: no disc, no ring, no border around the window.
- Outline and needle take the color of the symbols on the window's neighbouring caption buttons (minimize and maximize). The color is read from the title-bar scan and follows the latest scan. If no symbol is found, the outline is black or white, whichever reads better on the title bar, at 78 percent opacity at rest. A sampled color that would hide on the title bar is nudged until its contrast is at least 1.5.
- Under the mouse the outline moves 45 percent toward white (dark bar) or black (light bar), as the caption symbols do; the black or white fallback becomes fully opaque. The glyph grows by up to 6 percent.
- Unpinned: only the outline. Pinned: the head of the pin is filled with the accent color; outline and needle keep the ink color. The head is moved lighter (dark bar) or darker (light bar) until its contrast to the title bar is at least 2.5, so it reads as a clear fill even in a window without an accent color of its own (a neutral bar gives a lighter or darker grey). The accent is the largest chromatic region of the title bar that is at least 18 x 18 dp (an "Update" button or a colored banner; a 16 dp favicon or a thin colored line does not count), else the title-bar color; or the fixed or per-app accent from the settings.
- Pressing gives spring feedback: the button shrinks to about 0.88 and swings back with a slight overshoot. With `animate` off, every change is instant. The press, release and hover frames are drawn inside the mouse message itself, with the animation advanced by 8 ms, so the change shows at once (`src/overlay/state.rs`).
- The whole button box is clickable, not only the glyph: every pixel has an alpha of at least 1 of 255 (`src/overlay/render.rs`), because Windows lets the mouse pass through fully transparent pixels of a layered window.
- The app icon is embedded in the exe (`assets/pin_it.ico`, `assets/pin_it.rc`); the tray icon is loaded from that resource.

### Placement

- The button sits in free space of the title bar, left of the caption buttons, and never over content. It ends 4 dp left of the caption-button group. For windows with a custom title bar the group is read from the title-bar scan (it can be one close button or have more icons next to it), not assumed to be three buttons.
- The 4 dp gap is measured to the caption buttons as they are drawn, found from the title-bar pixels. The group bounds DWM reports are about 8 px wider than the drawn buttons on Windows 11, so the pixel reading replaces the DWM edge when the two agree within one button; if they differ by more, the DWM edge stands (`pin_limit` and `PlacementInput.group_right` in `src/placement.rs`).
- When the target is minimized, restored or maximized, Windows animates a bitmap while the real window is already at its final place. The pin fades out in 100 ms, stays invisible for 200 ms and fades in over 150 ms, with ease-out; it is click-through while invisible. With `animate` off it hides and shows at once (`src/overlay/fade.rs`, `src/overlay/button.rs`, `src/overlay/wndproc.rs`).
- Size: 0.7 times the caption height on a 4 dp grid, 16 to 32 dp. The user scale (50 to 200 percent) is applied and snapped to 4 dp (12 to 40 dp). The size is also limited to the caption height minus 2 dp above and 2 dp below (caption minus 4 dp), rounded down to the 4 dp grid, with a minimum of 12 dp; below that the button is hidden. On a 32 dp title bar the largest button is 28 dp. The horizontal margin to other title-bar content stays 4 dp.
- A maximized window starts above the screen by its invisible resize border. For a window with a system title bar the caption row is taken from the border's lower edge to the bottom of the caption buttons; for a window that draws its own caption (Chrome, Electron) the reported bounds move down by the border (`src/placement/maximized.rs`).
- When the space is tight, the button shrinks in 4 dp steps before it hides.
- With no free space, the button is hidden. The window stays pinned if it was pinned.
- Windows with a custom title bar (VS Code, Chromium browsers, Firefox) have only an estimated slot, which in Firefox lies on the list-all-tabs button. The button is never shown there. It waits up to 3 seconds for a scan of the painted window (an unpainted or half-painted scan does not count) and is placed from that scan. If no usable scan arrives within 3 seconds the button stays hidden and the log says `button hidden: no title bar scan`. After a resize, the old scan is stale and the button stays at the default slot next to the caption buttons until the new scan arrives.
- A strip that shades smoothly from one color to another (Firefox) counts as background along its whole length; a tab fill or a button on it still counts as content. Content right next to the caption-button group (the list-all-tabs chevron of Firefox, the layout buttons of VS Code) is content like any other: the button keeps 4 dp from it. Title bars with an image or transparency (mica) may count as occupied, so the button is hidden.
- For windows without a button, use the tray command or the hotkey.

### Button size dial

"Button size..." in the tray menu opens a small window with a rotary dial.

- 21 detents every 5 percent from 50 to 150, 100 percent at 12 o'clock with the largest tick, 25 percent steps bigger, 270 degree arc. Click effects: kick, tick flash, readout pulse, quiet click sound (setting `dial_click_sound`).
- Drag the knob with the mouse, or use the arrow keys, Home, End or the mouse wheel (one detent per step). A change is applied and saved at once.
- The readout shows the percent and the effective size, for example `120 %  (28 dp)`, calculated for a 32 dp title bar by the same rules as the button. It adds "limited by title bar" when the caption height cut the size. The preview in the window draws that size.
- Esc, Enter, Space, or a click outside the window close it. Losing the focus within 500 ms of opening does not close it, because Windows hands the focus back to the previous window right after the tray menu closes; after such a loss one click inside the window is needed before leaving it closes it. The same holds for the excluded apps window. A value above 150 in the settings file (up to 200) still works; the dial shows it at its upper end and leaves it unchanged until a detent is chosen.
- Colors follow the Windows light or dark app theme. The dial and the excluded apps window use the color #60273D with a glow.

### Excluded apps window

"Excluded apps..." in the tray menu opens a window with the list of excluded programs and an input under it. Excluded programs get no button and are never pinned by the hotkey or the tray command.

- The list shows the exe names in alphabetical order, one per row. The trash can at the right end of a row removes that program at once, without a question. A row is highlighted under the mouse. The mouse wheel scrolls the list by one row per notch; a scroll indicator appears only when the list is longer than the window. An empty list shows "No excluded apps".
- The input is a single line that scrolls sideways. It takes up to 255 characters, the longest file name Windows allows. Enter adds the program. The name is trimmed and lower-cased, and `.exe` is appended when it contains no dot (`Chrome` becomes `chrome.exe`).
- A name is refused when it is empty, contains `\ / : * ? " < > |` or a control character, ends with a dot, or is longer than 255 characters including an appended `.exe`. The window then shows "Not a valid program name" under the input and keeps the text; the hint goes away when you type on.
- A name that is already in the list is ignored: the input is cleared and the existing row flashes briefly. A name that is added scrolls into view and flashes the same way.
- Keys: Esc closes the window, Up and Down select a row, Delete removes the selected row while the input is empty (otherwise it edits the text).
- Nothing is protected: `explorer.exe` or `applicationframehost.exe` can be excluded too.
- Position, behavior and theme are those of the size dial: one window at a time (a second request brings it to the front), bottom right of the work area of the monitor under the cursor, no taskbar button, closed by Esc, a click outside the window or when Pin It exits. The colors follow the Windows light or dark app theme.
- The trash can is a vector outline drawn at the window's scale. It is grey (`#737672`) and turns red under the mouse: `#a40007` on the dark theme, `#6b0106` on the light theme. The three colors are constants in `src/exclusions/colors.rs`.

### Window order and logging

- After an activation (foreground event) the buttons are restacked at once and again after 30 ms and 100 ms (also after a cloak), because the system raises a window that is being activated after the event.
- The overlay of the foreground window checks every 10 ms that it still sits directly above its window and corrects it at once; a click on the title bar of the active window raises it without any event. The other overlays do not run this timer.
- While the left mouse button is down, and for 200 ms after it goes up, that overlay is restacked every 1 ms (a multimedia timer posts the message; `timeBeginPeriod(1)` is set only while it runs). An activation starts the same pulse for 120 ms. The system raises a clicked window within about one display frame, and the overlay has to be back before that to avoid flicker (`src/overlay/pulse.rs`, `src/overlay/stacking.rs`).
- A button that has to hide for a reason that can be a flicker (the window's styles or size changing for a moment, no caption slot) stays until the reason has held for 150 ms at a check; a button waiting for its first scan is never hidden. These reasons hide at once: minimized, not visible, cloaked, destroyed, Pin It switched off, no free space in a scan of the current window size, and no title bar scan within 3 seconds for a custom title bar. The checks run on window events and in the 2 second sweep.
- While any unpinned window is moved or resized, the pin order is checked at most every 100 ms, so a dragged window cannot stay above a pin.
- Cloak and uncloak events (virtual desktop switch, suspended apps) are handled: a cloaked window's button is refreshed (hidden), an uncloaked window gets its button back.
- If an unpinned, visible window lies above the lowest pin (for example an app's own always-on-top window), the pins are applied again and one line is logged, at most every 5 seconds: `z-order anomaly: window ... is above pinned ...; re-asserting pins; top of z-order: ...`. A window that stays above the pins afterwards is not handled again.
- The title bar is scanned again: after a resize (at most 10 times per second per window), when a window gains or loses the foreground (at most every 500 ms, taken 150 ms later when the bar shows its new look), and every 5 seconds for the foreground window. While a window is in a move or size loop no scans of it are taken; one follows when the loop ends. A change of the system accent or theme (`WM_DWMCOLORIZATIONCOLORCHANGED`, `WM_THEMECHANGED`, `WM_SETTINGCHANGE` with `ImmersiveColorSet`, or a different accent seen by the sweep) makes every cached scan expire. The sampling thread forgets the last sample of a window that has closed.
- Each change of a button's visibility is logged once: `button shown at <x>,<y> size <n> (<hwnd>)` or `button hidden: <reason> (<hwnd>)`.

## Install and run

To build from source, use Rust 1.96 and the MSVC build tools:

    cargo build --release

Run `target\release\pin_it.exe`. It has no window, only a tray icon. Only one instance runs at a time. The release build links the C runtime statically and loads DLLs only from System32.

## Tray menu

Labels are built in `menu_model` in `src/tray/model.rs`. The first line is the title "Pin It" (bold, not selectable); then:

- Enabled
- Pin / unpin current window (the label shows the exe name of the last app window in front; greyed out when that window cannot be pinned)
- Colors: Use window color, Fixed accent color..., Reset fixed color
- Animation
- Button size... (opens the size dial)
- Hotkey: None (off), Ctrl+Alt+P, Ctrl+Shift+P, Shift+Win+P, Ctrl+Alt+Shift+P
- Excluded apps... (opens the excluded apps window)
- Start with Windows
- Unpin all
- Open settings file (opened with the app registered for `.json`; when there is none, in Notepad, and one info line in the log says so: `src/app/open_settings.rs`)
- Exit

Every command chosen in the menu is logged: `tray command: <command>`, followed by `tray command done` or `tray command failed: <reason>`.

If the tray icon cannot be added, it is retried every sweep. When Explorer restarts, the icon is added again.

## Hotkey

The hotkey is off by default and nothing is registered until one is chosen, in the tray menu or in the settings file. It acts on the foreground window.

- Valid: a key (virtual-key code 1 to 254) with Ctrl, Alt or Win, or Shift together with F1 to F24. Invalid values in the file are dropped.
- Example in the settings file: `"hotkey": {"ctrl": true, "alt": true, "shift": false, "win": false, "vk": 80}` (80 = P).
- If registration fails (combination taken), the previous hotkey is registered again, or none, and the settings are changed to match.
- The hotkey and the tray command do not pin windows of elevated programs, excluded apps, or Pin It's own windows.

## Settings

File: `%APPDATA%\PinIt\settings.json`.

| Field | Default | Meaning |
|---|---|---|
| `version` | 1 | file format version |
| `enabled` | true | master switch. Off: all pins are released, no buttons, hotkey and tray pin do nothing. Switching on does not restore pins. |
| `hotkey` | none | see above |
| `use_window_color` | true | accent from the window's title bar |
| `fixed_accent` | none | `[r, g, b]` used when `use_window_color` is off; none means the system accent |
| `animate` | true | animations; off makes everything instant |
| `autostart` | false | writes a `PinIt` value to `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` |
| `button_scale_percent` | 100 | clamped to 50..200; the dial offers 50 to 150 |
| `dial_click_sound` | true | quiet click on every detent of the dial (edit in the file) |
| `excluded_apps` | [] | lowercase exe names without a button; edited in the excluded apps window |
| `per_app_accent` | {} | exe name to `[r, g, b]`; wins over the other accent settings |

The field `border` no longer exists. Old files that contain it still load; unknown fields are ignored.

Loading is lenient. An invalid field falls back to its default, and an invalid list element or map entry is dropped. The original file is then copied to `settings.json.bad-<unix seconds>` before the next save rewrites it. A file larger than 1 MiB or not valid JSON (or not a JSON object) is moved to `settings.json.bad-<unix seconds>` and defaults are used. The newest 3 such files are kept. UTF-8 with BOM and UTF-16 with BOM are accepted.

Hand edits are loaded at start. A running instance picks them up when the file's modified time changed and a tray menu opens or a tray command runs. A hand-edited hotkey is registered at that point or at the next start.

Settings, log and `pins.json` are not written when the file or its folder is a junction or symlink.

## Files in `%APPDATA%\PinIt`

- `settings.json` and its backups.
- `pinit.log`: one line per event with a UTC timestamp. At 256 KiB it is cut to the newest 128 KiB.
- `pins.json`: the pinned windows with window handle, process id, process start time, `was_topmost` and `pending_release`. It exists while pins exist, and is deleted when the list is empty.

## Exit and crashes

- The pin is recorded in `pins.json` before the topmost flag is set, so a crash in between is handled by the next start.
- On exit, all pins are released. The release is queued and does not wait for a busy window.
- A release that fails stays in `pins.json` (`pending_release`) and is retried by the sweep and by the next start.
- After a crash, the next start clears the pins left in `pins.json`. A window is only touched if its process id and process start time match, so a reused window handle is safe.
- Windows that were already topmost before Pin It keep their topmost flag.
- When HWND_TOPMOST does not set the flag, the pin is retried in this order: HWND_NOTOPMOST then HWND_TOPMOST, then HWND_TOPMOST with SWP_SHOWWINDOW (no activation); only then the window counts as refused (`src/zorder/primitives.rs`).
- A window that refuses HWND_TOPMOST stays pinned and is retried every 2 second sweep. A hung window keeps its pin and is retried. One log line is written per episode.

## Limits

- Windows of elevated programs get no button and cannot be pinned by hotkey or tray.
- UWP windows are named by the app's own exe when it can be found; otherwise by `applicationframehost.exe`, and excluding that name excludes all of them.
- Fullscreen games are not treated specially.
- Pins are kept while a window is hidden, minimized or on another virtual desktop; only its button is hidden.
- Display and DPI changes are picked up by the next 2 second sweep.
- Windows 11 is the tested system. Windows 10 is untested.

## Development

- `cargo build --release`, `cargo fmt --check`, `cargo clippy --all-targets`.
- `cargo test` needs an interactive desktop and runs focus-sensitive window actions. Run it while no other program moves windows. The system sometimes ignores HWND_TOPMOST for the test windows, so tests wait with a deadline.
- Ignored tests are diagnostics, run with `cargo test <name> -- --ignored --nocapture`:
  - `foreign_windows_full_vs_caption_capture` (src/color/real_windows.rs): read-only PrintWindow of up to 2 foreign windows, full vs caption-only bitmap.
  - `register_unregister_and_conflict` (src/hotkey.rs): hotkey registration and conflicts on private windows.
  - `real_windows_placement` (src/placement/real_windows.rs): scans real windows and writes PNGs of the title-bar strip with default and chosen slot to the folder in `PLACE_OUT`.
  - `f3_firefox_timeline` (src/app/tests/f3_firefox.rs): on the one running Firefox window, prints when the pin first appears and where, and saves a PNG of the title-bar strip with the pin marked to the folder in `PLACE_OUT` (file name in `PNG_NAME`). It reads a foreign window.
  - `real_windows_report` (src/windows_filter/tests.rs): prints exe names and compares fallback with native slots on real windows.
  - `spacing_survey` (src/placement/real_windows/survey.rs): lists every visible captioned window with the pin's gap to the drawn caption buttons; set `PLACE_OUT`. It reads foreign windows.
  - `x3_latency_of_the_reanchor_after_a_resize` (src/app/tests/x3_latency.rs): time from a resize to the moved pin, on a private window.
  - `pulse_restacks_a_raised_target_within_a_few_milliseconds` (src/overlay/stacking_tests.rs): restack delay with and without the 1 ms pulse, on private windows.
  - `z_order` (src/zorder/real_windows.rs): z-order behavior on private test windows.
  - `hung_target_returns_hung_without_blocking` (src/zorder/tests.rs): a PowerShell window with a sleeping UI thread must give `Hung` without blocking.
  - `busy_target_unpin_is_queued_and_applied_after_resume` (src/zorder/tests.rs): unpin of a busy window returns at once and takes effect after it resumes.
- `build.rs` embeds `assets/pin_it.rc` (icon, manifest with per-monitor DPI v2, version info) with the `embed-resource` crate.

## License

MIT, see `LICENSE`.
