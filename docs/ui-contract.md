# TakTak app ↔ UI contract (Milestones 3–5)

The Tauri app crate (`src-tauri`, Rust) owns all state and talks to `taktak-core`. The web
UI (`src/`, Svelte 5 + TypeScript) is a thin view: it renders `AppState`, calls commands, and
re-renders on the `state-changed` event. The UI never reads files, never touches audio, and
never sees what the user types.

Milestone 4 adds per-app rules, auto-mute and the macOS permission onboarding. Everything from
Milestone 3 keeps working unchanged; every Milestone 4 addition is marked **(M4)**. Milestone 5
adds the in-app Mechvibes import, the "fully offline" statement in the UI, and the Windows and
Linux listener cases (the Linux `input` group, platform-specific restart texts); its additions
are marked **(M5)**.

## Windows

| Label | Purpose | Lifecycle |
|---|---|---|
| `tray` | Small popover anchored to the menu-bar/tray icon (left click). | Created on open. Hidden on blur, on a second tray click and on Escape (`hide_tray`). Destroyed once it has stayed hidden for 60 s, and 1 s after Settings opens; the next click creates it again (~0.2 s). |
| `settings` | Full settings window, title "TakTak Settings". | Created on demand; destroyed on close (saves memory). |
| `onboarding` **(M4)** | Welcome and permission guide, title "Welcome to TakTak". 480 × 440 (was 560 × 640 before the minimal redesign; the content scrolls when a disclosure is open), not resizable, not minimizable or maximizable, centered, focused, normal title bar. | Created by the app at startup when `onboarding.offer` is true (see "Onboarding"), and by `open_onboarding` (focuses it if it exists). Destroyed on close. Closing it by the user or `finish_onboarding` (close button, ⌘W, the window's own buttons) sets `settings.onboardingDone = true`; quitting or `relaunch` while it is open does not (see "Relaunch"). Opening it destroys the hidden popover 1 s later, like Settings. |

Each webview keeps WebKit helper processes alive (WebContent alone is 20–40 MB), which is why
no window outlives its use. The UI must not rely on the popover's page staying loaded
between opens.

There is no main window and, on macOS, no Dock icon (accessory app). All windows load the
same bundle; the UI picks the view from `window.__TAURI_INTERNALS__.metadata.currentWindow.label`
(via `getCurrentWindow().label`). The `default` capability (`src-tauri/capabilities/default.json`)
lists all three labels: each may only listen to events.

Opening TakTak again while it runs shows (or focuses) the settings window: on macOS from
Finder, Spotlight, Launchpad or `open` (the running app gets a reopen event), and on every OS
when a second process starts (`open -n`, running the binary again), which hands over and
exits. On macOS, hiding the popover with a tray click or Escape also hands the keyboard back to
the app the user was typing in (TakTak hides itself unless Settings is showing).

The tray icon's **right-click** opens a native menu: `Sounds On` (check), `Pack ▸` (radio
list), `Mute` (check, shows the hotkey), `Settings…`, `Quit TakTak`. Tooltip: `TakTak`.

**(M4)** Additions to the tray:

- A **status item** at the top of the menu, followed by a separator, present only while sounds
  are on, not muted by hand, and still silent for one of the reasons below (texts exactly as
  given; `<app>` is `frontmostApp.name`, escaped like pack names):

  | Reason (first that applies) | Text | Item |
  |---|---|---|
  | `autoMute === "screenLocked"` | `Muted — screen locked` | disabled |
  | `autoMute === "outputChanged"` | `Muted — output device changed` | disabled |
  | `permission === "denied"` (macOS) | `Needs Input Monitoring…` | enabled: opens the onboarding window |
  | `permission === "denied"`, Linux, `onboarding.inputGroupNeeded` **(M5)** | `Needs keyboard access…` | enabled: opens the onboarding window |
  | `permission === "denied"`, Windows or Linux otherwise **(M5)** | `Key listener stopped…` | enabled: opens the onboarding window |
  | `audio.state === "fault"` | `No sound output` | disabled |
  | `ruleBlocked` | `Silent in <app>` (`Silent in this app` when `frontmostApp` is null) | disabled |

- `Mute` is checked when `muted || autoMute === "outputChanged"`. Clicking it (and pressing the
  mute hotkey) toggles that value: if it is on, it unmutes, which clears both; otherwise it
  mutes (see "Auto-mute").
- Tooltip: `TakTak`, or `TakTak — <status item text>` while the status item shows (without the
  trailing `…`).

## Types (TypeScript view; Rust mirrors them with `serde(rename_all = "camelCase")`)

```ts
type VariantMode = "consistent" | "random";

interface Settings {
  enabled: boolean;          // master on/off (tray toggle)
  packId: string;            // active pack id
  masterVolume: number;      // 0..1, slider position; engine gain = masterVolume^2
  pressVolume: number;       // 0..1, linear
  releaseVolume: number;     // 0..1, linear
  variantMode: VariantMode;  // default "consistent"
  humanize: number;          // 0..1, default 0.25
  muteHotkey: string | null; // Tauri accelerator, e.g. "CommandOrControl+Alt+Shift+M"; null = none
  launchAtLogin: boolean;
  appRule: AppRule;              // (M4) per-app rules; default { mode: "everywhere", apps: [] }
  muteOnOutputChange: boolean;   // (M4) auto-mute when the default output device changes; default false
  onboardingDone: boolean;       // (M4) the onboarding window was closed at least once; default false
}

// (M4) Per-app rules
type AppRuleMode =
  | "everywhere"             // sounds in every app; the list is kept but ignored (default)
  | "only"                   // sounds only while a listed app is frontmost
  | "never";                 // silent while a listed app is frontmost

interface AppRef {           // an app, as TakTak identifies it
  id: string;                // macOS bundle identifier, e.g. "com.apple.Safari"
  name: string;              // display name, e.g. "Safari"
}

interface AppRuleEntry {     // one listed app. An object, not a bare id: per-app overrides
  id: string;                // (e.g. packId, volume) will be added later as optional fields
  name: string;              // where absent = follow the global setting, with no migration.
}                            // Readers ignore entry fields they do not know.

interface AppRule {
  mode: AppRuleMode;
  apps: AppRuleEntry[];      // one list for both "only" and "never"; append order; ids unique;
}                            // at most 200 entries

interface AppInfo {          // a pickable app (list_running_apps, choose_app)
  id: string;                // bundle identifier
  name: string;              // localized display name
  iconDataUrl: string | null;// "data:image/png;base64,…", 32 × 32 px (16 pt @2x); null = no icon
}

type AutoMute =
  | "screenLocked"           // the screen is locked, or this user session is inactive
                             // (fast user switching); clears by itself on unlock / return
  | "outputChanged";         // the default output device changed while muteOnOutputChange was
                             // on; stays until the user unmutes (see "Auto-mute")

interface OnboardingStatus {
  offer: boolean;            // offer the onboarding: !settings.onboardingDone, or
                             // permissionRequired && permission === "denied" (live)
  permissionRequired: boolean; // the platform needs a permission the user grants (macOS with
                             // the key listener on); false: the window skips the permission step
  relaunchSuggested: boolean;  // the permission looks granted (macOS: Input Monitoring; Windows
                             // and Linux X11 have none to grant) but the key listener still
                             // cannot start: a relaunch usually fixes it (permission shows
                             // "denied" meanwhile). Never together with inputGroupNeeded
  inputGroupNeeded: boolean;   // (M5) Linux: the listener must read the keyboard devices
                             // (Wayland, or TAKTAK_INPUT=evdev) and cannot: the user is not in
                             // the `input` group (permission shows "denied" meanwhile)
}

interface PackSummary {
  id: string;
  name: string;
  author: string;
  license: string;           // SPDX
  description: string | null;
  attribution: string | null;
  origin: "bundled" | "user";
  hasRelease: boolean;       // any release sounds at all
  perKey: boolean;           // has per-key entries
  warnings: string[];        // human-readable, already escaped
}

interface InvalidPack {
  location: string;          // path shown to the user
  problems: string[];        // formatted Problem lines
}

type Permission = "granted" | "denied" | "unknown";

interface AudioStatus {           // the output device in use; while the stream is closed (see
                                  // "Power" below), the default device the next open would use
  device: string | null;
  sampleRate: number | null;
  bufferFrames: number | null;
  state: "ok" | "starting" | "fault";   // "fault" also while no output device exists at all
  message: string | null;    // user-facing, e.g. "Output device disconnected — reconnecting…"
}

interface AppState {
  version: string;
  settings: Settings;
  muted: boolean;            // the manual mute: hotkey, tray, set_muted (separate from enabled).
                             // Auto-mute never changes it.
  playing: boolean;          // a key press makes a sound now: enabled && !muted &&
                             // autoMute === null && !ruleBlocked && permission granted && audio ok
  packs: PackSummary[];      // sorted by name
  invalidPacks: InvalidPack[];
  playingPackId: string | null;   // the pack whose sounds are loaded now: the selected one, a fallback,
                                  // or the selected pack's last working version; null = built-in click
                                  // (also before the first pack has loaded)
  activePackError: string | null; // set when the selected pack is not the one playing: why, and what plays instead
  muteHotkeyError: string | null; // set when the saved settings.muteHotkey could not be registered at
                                  // startup (invalid, or taken by another app): it does nothing, and
                                  // the tray menu does not show it. Cleared by a successful set_mute_hotkey.
  userPacksDir: string | null;
  permission: Permission;
  audio: AudioStatus;
  frontmostApp: AppRef | null;    // (M4) the app in front now (TakTak's own windows excluded); null
                                  // when unknown, when it has no bundle id, or !rulesSupported.
                                  // Current value only: never logged, persisted or collected
  ruleBlocked: boolean;           // (M4) settings.appRule silences frontmostApp right now
  autoMute: AutoMute | null;      // (M4) why TakTak muted itself; null = not auto-muted
  rulesSupported: boolean;        // (M4) per-app rules work on this platform (macOS); false on
                                  // Windows and Linux for now: rules are kept but ignored
  onboarding: OnboardingStatus;   // (M4)
}

interface LatencyReport {     // timings only, never key identities
  count: number;
  totalP50Ms: number; totalP95Ms: number; totalMaxMs: number;
  inputP50Ms: number; queueP50Ms: number; outputMs: number;
}

// (M5) Importing Mechvibes packs
type PickKind = "any" | "folder" | "zip";  // macOS: one panel takes a folder or a .zip,
                             // whatever the kind. Windows and Linux pickers take one or the
                             // other; "any" is the .zip picker there

interface ImportSummary {    // what an import wrote; every string is pack content, escaped
  id: string;                // the new pack's id ("mv-<name>"), listed in packs within ~1 s
  name: string;
  source: string;            // the source folder or .zip file name
  format: string;            // e.g. "Mechvibes v2", "MechvibesDX config v2"
  keysMapped: number;        // keys with a sound of their own
  keysWithRelease: number;   // how many of them also have their own release sound
  soundsWritten: number;     // sound files written (the preview not counted)
  replaced: boolean;         // an earlier import of the same pack was replaced
  warnings: string[];        // skipped keys, missing or unreadable files, other notes; at most
}                            // 30 lines (the last then says how many more)

type MechvibesImport =
  | { outcome: "imported"; pack: ImportSummary }
  | { outcome: "alreadyImported"; id: string; source: string };  // imported before (same id,
                             // same source name); nothing changed; overwrite_mechvibes_pack
                             // replaces it
```

## Commands (`invoke`)

All return `Promise<AppState>` unless noted, and reject with a user-facing string on error.

| Command | Args | Notes |
|---|---|---|
| `get_state` | — | |
| `set_enabled` | `{ enabled: boolean }` | **(M4)** `enabled: true` also clears an `outputChanged` auto-mute. |
| `set_muted` | `{ muted: boolean }` | Same as the hotkey. **(M4)** Either value also clears an `outputChanged` auto-mute (the manual mute takes over). It never touches `screenLocked`. |
| `set_pack` | `{ id: string }` | Loads off the main thread; state updates again via event when loaded. Persisted. |
| `set_master_volume` | `{ value: number }` | Clamped 0..1. Persisted (debounced). |
| `set_press_volume` / `set_release_volume` | `{ value: number }` | Clamped 0..1. |
| `set_variant_mode` | `{ mode: VariantMode }` | |
| `set_humanize` | `{ value: number }` | Clamped 0..1. |
| `set_mute_hotkey` | `{ accelerator: string \| null }` | Validates and registers; rejects with a message if invalid or already taken by the system. `null` or `""` removes the hotkey. Besides unparseable accelerators it rejects media/volume keys and any key other than F1–F24 without Ctrl, Alt/Option or Cmd/Win (Shift alone would swallow typing); F1–F24 work alone or with Shift only. On rejection the previous hotkey stays registered. Success clears `muteHotkeyError`. See "Hotkey keys and keyboard layouts" below. |
| `set_launch_at_login` | `{ enabled: boolean }` | |
| `preview_pack` | `{ id: string }` | Plays that pack's preview clip once (does not change the active pack), opening the output for it if it is closed. Returns `void` once the clip plays. Rejects with a user-facing message when it cannot: no output device, the pack's sounds cannot be decoded, or it has nothing to preview. A clip still decoding after 5 s resolves and plays when ready. A preview replaced by another or stopped resolves too. Previews ignore mute, auto-mute and per-app rules. |
| `stop_preview` | — | Returns `void`. |
| `open_settings` | — | Shows/creates the settings window. Returns `void`. |
| `hide_tray` | — | Hides the tray popover (use it for Escape instead of hiding the window from JS): on macOS it also hands the keyboard back to the app the user was in. Returns `void`. |
| `open_user_packs_dir` | — | Creates it if needed and reveals it in Finder/Explorer. Returns `void`. |
| `open_permission_settings` | — | macOS: opens Privacy & Security → Input Monitoring. Returns `boolean`: whether TakTak is in that list now (always `true` on Windows and Linux, where it opens nothing). **(M4)** If macOS has never decided on TakTak (`IOHIDCheckAccess` unknown), it first asks macOS to list TakTak (`IOHIDRequestAccess(kIOHIDRequestTypeListenEvent)` on the main thread; `CGRequestListenEventAccess` once more if that left no trace) and waits up to ~1 s per request for the entry to appear, then opens the pane, so the pane lists TakTak and does not race macOS's alert. It does not open the pane when the request itself granted access. `false` means macOS added nothing (its alert, which is what creates the entry, stops coming once macOS has recorded the app, even after `tccutil reset`; see `platform-notes.md` § macOS): the user adds TakTak with + (see `reveal_app`). Async; may take ~2 s. |
| `get_latency` | — | Returns `LatencyReport \| null` (null until 5 presses measured since the window opened). |
| `quit` | — | Exits the app. Returns `void` (in practice the promise never settles). |
| `list_running_apps` **(M4)** | — | Returns `AppInfo[]`: the running regular apps (those with a Dock icon), without TakTak itself and apps that have no bundle identifier, one per id, sorted by name (case-insensitive). Icons are rendered once per id and kept in an in-memory cache (at most 256 icons, least recently used dropped first; never persisted). Async; the first call may take ~100 ms. `[]` when `!rulesSupported`. |
| `choose_app` **(M4)** | — | Opens the native file picker for an app bundle (`.app`, starting in `/Applications`) and returns its `AppInfo` (id = `CFBundleIdentifier`, name = the bundle's localized display name, icon) or `null` when cancelled. It does not add the app; call `add_rule_app`. Rejects when the bundle has no `CFBundleIdentifier` ("That app has no bundle identifier, so TakTak can't tell it apart."), when a picker is already open ("The app chooser is already open."), and when `!rulesSupported` ("Per-app rules aren't available on this system yet."). Async: resolves when the user closes the picker. |
| `get_app_icons` **(M4)** | `{ ids: string[] }` | Returns `Record<string, string \| null>`: an `iconDataUrl` for each requested id (at most 200), looked up by bundle id among installed apps, from the same cache; `null` for an app macOS cannot find. For showing listed apps that are not running. Only icons of apps on the rule list (`settings.appRule.apps`) are added to the cache; any other id, such as `frontmostApp.id`, is rendered for this call and not kept, so the cache never collects the apps the user brought to the front. Async. All `null` when `!rulesSupported`. |
| `set_app_rule_mode` **(M4)** | `{ mode: AppRuleMode }` | Persisted. Keeps the list. |
| `add_rule_app` **(M4)** | `{ id: string; name: string }` | Appends `{ id, name }` to `settings.appRule.apps`. Both are trimmed; an empty name becomes the id. An id that is already listed changes nothing (no error, the entry keeps its fields). Rejects an empty id, one longer than 255 characters or containing whitespace or control characters ("That is not an app TakTak can recognize."), TakTak's own id `tech.taktak.app` ("TakTak itself can't be listed: its windows always follow the app you were in."), and a 201st entry ("You can list up to 200 apps."). Persisted. |
| `remove_rule_app` **(M4)** | `{ id: string }` | Removes that entry; an id that is not listed changes nothing. Persisted. |
| `set_mute_on_output_change` **(M4)** | `{ enabled: boolean }` | Persisted. `false` also clears an `outputChanged` auto-mute. |
| `open_onboarding` **(M4)** | — | Shows/creates the onboarding window (activating TakTak). Returns `void`. Async (window creation off the main thread). |
| `finish_onboarding` **(M4)** | — | Sets `settings.onboardingDone = true` (persisted), then closes the onboarding window if it is open. Returns the new `AppState`. |
| `relaunch` **(M4)** | — | Quits like `quit` (pending settings saved, audio stopped) and starts TakTak again. Returns `void` (the promise never settles). For when macOS only lets a relaunched TakTak listen (**(M5)** and for a Windows or Linux key listener that failed to start). See "Relaunch" for what the app must guarantee. |
| `import_mechvibes_pack` **(M5)** | `{ kind: PickKind }` | Opens the native picker for a Mechvibes pack (its folder, the one with `config.json`, or its `.zip`), then converts it into the user packs folder on a worker thread (never the main thread) and returns `MechvibesImport`, or `null` when the picker was cancelled. The pack is written to a hidden folder, validated and only then moved into place, so hot reload lists it (as `origin: "user"`, `license: "LicenseRef-Personal"`) within about a second, without a restart. One import at a time. Rejects with a user-facing message: not a Mechvibes pack (with a hint to choose the folder with `config.json` or the `.zip`), an unreadable `config.json`, an unsupported pack (mouse packs, unknown config versions; names the supported formats), no usable sounds (with a hint to check that the files `config.json` names are there and readable), file errors, no user packs folder ("This system has no folder for your own packs."), and an import or picker already running ("TakTak is already importing a pack, or its pack chooser is still open."). Async: resolves when the import is done. |
| `reveal_app` | — | Shows TakTak itself selected in the file manager (macOS: `TakTak.app` in Finder, to drag it into the Input Monitoring list or find it from +). Returns `void`. Async. |
| `overwrite_mechvibes_pack` **(M5)** | — | Imports again, replacing the earlier import, the source of the last `import_mechvibes_pack` that returned `alreadyImported` (kept in memory only; the UI never passes a path). Returns `MechvibesImport` (`imported`, `replaced: true`). Rejects when nothing is waiting ("There is no import waiting to be replaced. Choose the pack again with Import Mechvibes pack…") and with the errors above. |

## Events

| Event | Payload | When |
|---|---|---|
| `state-changed` | `AppState` | After any change: commands, tray menu actions, hotkey, pack hot-reload, permission granted, audio fault/recovery, pack load finished. **(M4)** Also: the frontmost app changes (event-driven, never polled), the screen locks or unlocks, the session becomes inactive or active, an auto-mute starts or clears, the onboarding window closes. |

## Behaviour rules

- Settings persist to `<app config dir>/settings.json` (atomic write, debounced ~300 ms).
  Missing/corrupt file → defaults (`buckling-spring`, master 0.7, press/release 1.0, consistent,
  humanize 0.25, hotkey `CommandOrControl+Alt+Shift+M`, launch at login off, enabled on;
  **(M4)** `appRule` `{ mode: "everywhere", apps: [] }`, `muteOnOutputChange` false,
  `onboardingDone` false).
- **(M4) Settings migration.** A Milestone 3 file has none of the three new fields; it loads with
  the defaults above, except that `onboardingDone` is **true** when the file exists but has no
  `onboardingDone` field (whoever has a settings file has run TakTak before; if Input
  Monitoring is missing, the onboarding still opens, see "Onboarding"). Leniency extends into
  `appRule`: a wrong-typed `appRule.mode` becomes `"everywhere"` and keeps the list; a
  wrong-typed `appRule.apps` becomes `[]`; an entry that is not an object with a non-empty string
  `id` is dropped and the others kept; ids are trimmed; a later duplicate id is dropped; a
  missing, wrong-typed or empty `name` becomes the id; entries past the 200th are dropped. Unknown
  fields, at the top level and inside entries, are ignored (and not written back).
- **Retired packs.** Earlier versions bundled three synthesized packs, `deep-thock` (then the
  default), `crisp-clack` and `blue-click`, which are no longer shipped. On startup, once the
  packs are scanned and before the first load, a saved `packId` naming one of them moves
  silently to the default `buckling-spring` and is saved, so no `activePackError` appears for
  users whose old default went away (Rust: `catalog::migrate_retired`, called from the control
  thread's start in `service.rs`). If a user pack with that id is installed, the selection is
  kept. Any other missing pack takes the normal fallback below.
- If the saved pack no longer exists, fall back to `buckling-spring`, then to any pack, then to
  the built-in click; set `activePackError` when a selected pack fails to load. `settings.packId`
  keeps the user's choice; `playingPackId` says what actually plays (Rust:
  `AppState::playing_pack_id`, set where the control thread swaps a bank in, `on_bank` in
  `service.rs`), and the UI names that pack (or "Built-in click" when it is null) wherever it
  says what is playing.
- Hot reload: when the active pack's files change, reload it; when it disappears, fall back as
  above. When it breaks on disk, its last working version keeps playing (`playingPackId`
  unchanged, `activePackError` says so).
  On startup and after a reload, a selected pack that is in `invalidPacks` (broken on disk) is
  reported as having errors, not as "not installed". The last working version survives the
  output being closed and reopened (Power, below) and a reopen on a device with the same rate.
  After a stream fault, or on a device with another rate, it cannot be decoded again: the
  fallback plays and `activePackError` says the pack has errors.
- Audio faults (device unplugged, stream invalidated) and the system moving the output to a new
  default device (macOS does this when headphones are plugged in or AirPods connect): reopen on
  the new default device, so its own rate and the 64-frame buffer apply, reload the pack at
  that rate if it differs, keep the listener working; report via `audio`.
- Power: the output stream and the key listener run only while a key press can make a sound
  (`enabled && !muted`, **(M4)** `autoMute === null`, not rule-blocked for 5 s or longer,
  permission granted) or a preview plays. An open stream keeps the
  audio device awake, which costs `coreaudiod` 5–9 % of a core even in silence. While it is
  closed, the playing pack stays decoded, `audio` shows the default device (state `ok`, or
  `fault` when there is none), and turning sounds on or unmuting opens it again in ~0.1 s.
  **(M4)** Auto-mute closes it at once. A rule block silences keys at once but closes the
  output only once it has lasted 5 s (a one-shot wake-up, not a poll), so hopping through a
  blocked app with ⌘Tab does not reopen the device; leaving the blocked app reopens it at once.
- Missing Input Monitoring permission: `permission = "denied"`, `playing = false`; poll and
  start listening automatically once granted. A key
  listener that fails to start although permission is granted (macOS sometimes refuses the tap
  right after the user grants it) also shows `"denied"`, and is retried every 30 s; **(M4)**
  `onboarding.relaunchSuggested` is true meanwhile.
  `"unknown"` means the platform has no key listener, or the app runs with `TAKTAK_NO_INPUT=1`.
  Windows and Linux have one since Milestone 5 (`"granted"`). **(M5)** `"denied"` there means
  either that the Linux listener cannot read the keyboard devices (Wayland or
  `TAKTAK_INPUT=evdev` without the `input` group: `onboarding.inputGroupNeeded`, see
  `platform-notes.md`), or that the listener failed to start (`onboarding.relaunchSuggested`,
  retried every 30 s). **(M4)** The onboarding window is the full guide (see "Onboarding").
- Mute hotkey: a saved `muteHotkey` that cannot be registered at startup stays in the settings
  but sets `muteHotkeyError` (see `AppState`).
- Privacy: no network access (CSP `default-src 'self'`; no remote URLs), no logging of keys,
  the hotkey recorder only reads key events inside the settings window while recording.
  **(M4)** See "Privacy (Milestone 4)".

### The gate, `muted` and `playing` (M4)

- The key hook still checks **one** `AtomicBool` and nothing else. Its value is
  `enabled && !muted && autoMute === null && !ruleBlocked`, recomputed off the hook thread on
  every state change (in `Shared::update`, as today). The hot path stays one atomic load.
- `muted` is the manual mute only. Auto-mute and rules never change `muted` or `enabled`, and
  never fight them: each reason is tracked separately, and sound plays only when none applies.
- `playing` = the gate value `&& permission === "granted" && audio.state === "ok"`.
- The mute switch the UI shows (popover, Settings) and the tray `Mute` check show
  `muted || autoMute === "outputChanged"`. Turning that switch off sends
  `set_muted({ muted: false })`, which clears both; turning it on sends `set_muted({ muted: true })`.
  The hotkey and the tray item toggle the same value. `screenLocked` is not part of the switch:
  it is transient and clears only on unlock.
- **Why it is silent**, first match wins (the UI status line and the tray status item use this
  order; texts in "UI requirements"): sounds off → muted by hand → `screenLocked` →
  `outputChanged` → no key listener and no permission prompt (`permission !== "granted" &&
  !onboarding.permissionRequired`): **(M5)** with `inputGroupNeeded` status `Needs keyboard
  access` and a warning notice "Keyboard access needed" with a Help button (the onboarding
  window); with `relaunchSuggested` (Windows, Linux) status `Key listener stopped` and a
  warning notice "TakTak needs to restart" with Quit & Reopen, in words that never mention
  macOS or Input Monitoring; otherwise (`TAKTAK_NO_INPUT=1`) status `Key sounds unavailable`
  and an info notice "Key sounds are turned off" without buttons → permission missing (macOS)
  → audio fault → `ruleBlocked` → audio starting.

### Per-app rules (M4)

- Identity: the macOS bundle identifier. Windows (exe path) and Linux (`WM_CLASS`) are future:
  there `rulesSupported` is false, `frontmostApp` null, `ruleBlocked` always false, and the
  rule settings are kept (and editable) but have no effect.
- Evaluation (`ruleBlocked`), with `listed` = `frontmostApp` is non-null and its id is in
  `settings.appRule.apps`: `"everywhere"` → false; `"only"` → `!listed` (an unknown frontmost
  app is not listed, so it is silent; an empty list is silent everywhere); `"never"` → `listed`.
- The frontmost app is tracked **event-driven** on macOS: `NSWorkspace`
  `didActivateApplicationNotification`, plus one `frontmostApplication` read at startup.
  Nothing polls. Activations of TakTak itself (opening the popover, Settings or the onboarding
  window) are ignored: `frontmostApp` keeps the app the user came from, so Settings can say
  "Silent in Slack" about it and offer to list it, and typing in TakTak's own windows follows
  that app's rule.
- Known limits (show them in the UI): Spotlight, Raycast and Alfred panels do not become the
  frontmost app, so typing there follows the rule of the app that was in front before. Password
  fields are always silent whatever the rules: macOS hides keystrokes there (Secure Input).

### Auto-mute (M4)

Two independent reasons, tracked as separate flags; `autoMute` shows the stronger one
(`screenLocked` before `outputChanged`), so an `outputChanged` that started before a lock is
still there after the unlock.

- **`screenLocked`** (always on, no setting): set on `com.apple.screenIsLocked` (distributed
  notification) and on `NSWorkspace` `sessionDidResignActiveNotification` (fast user switching);
  cleared when both have been undone (`com.apple.screenIsUnlocked`,
  `sessionDidBecomeActiveNotification`). At startup the screen counts as unlocked and the session
  as active. Nothing the user does in TakTak clears it.
- **`outputChanged`** (only while `settings.muteOnOutputChange`): set when the system default
  output device becomes a different device (by name) than the one TakTak last saw, while sounds
  are on and not muted by hand (`enabled && !muted`; a lock does not matter: unplugging
  headphones at the lock screen still mutes). Examples: headphones plugged in or unplugged,
  AirPods connecting, another output picked in Sound settings. The first device after launch,
  and the same device returning after a fault, do not count. Seen within ~0.5 s while the output
  is open (reroute and fault handling), and while it is closed at the next look at the default
  device (the existing 5 s refresh, or the next open). It stays until the user unmutes:
  `set_muted` (either value), the hotkey or the tray `Mute` item, `set_enabled({ enabled: true })`,
  or turning `muteOnOutputChange` off. `audio.device` names the new device.

### Onboarding (M4)

- Decision: `onboarding.offer = !settings.onboardingDone || (permissionRequired && permission
  === "denied")`. `permissionRequired` is true on macOS with the key listener on (false under
  `TAKTAK_NO_INPUT=1`, and on Windows and Linux, which have no permission prompt). **(M5)** So on
  Linux the welcome window explains the `input` group on the first launch, but does not open
  again at every launch while the user has not joined it: joining is an opt-in with a security
  cost, not something to nag about. The status line, the notice and the tray item still say
  it, and each leads to the window.
- At startup the app opens the onboarding window when `offer` is true **after the first
  permission check** (macOS), or right away where no permission is required. So it shows on the
  first launch, and on every launch where Input Monitoring is missing; it does not show again
  once permission is granted and the window has been closed once.
- TakTak no longer shows the macOS permission prompt at startup. The first time the onboarding
  window appears in a launch (~0.7 s after it shows), the app asks macOS once to list TakTak
  under Input Monitoring, if macOS has never decided on it, so TakTak is in the list even when
  the user opens System Settings by hand. At most once per launch automatically, never in a
  loop. `open_permission_settings` asks the same way (when undecided) and opens the pane.
- While the window is open, the existing 2 s permission poll flips `permission` to `"granted"`
  without a restart whenever macOS allows it; the window turns into its success state from the
  `state-changed` event. When macOS reports access but the listener still cannot start,
  `relaunchSuggested` turns true and the window leads with "Quit & Reopen" (`relaunch`).
- The window can be opened again any time with `open_onboarding` (Settings, and the tray status
  item while permission is missing).

### Relaunch (M4)

`relaunch` must reliably end with exactly one new TakTak running:

- It runs the normal exit path first (settings flushed, audio stopped, `instance.sock` removed).
- If the onboarding window was open, the new instance opens it again (e.g. the old one passes
  `--onboarding`), so the user sees the success state, or the troubleshooting if TakTak still
  cannot listen.
- The new process must not hand over to the exiting one: Tauri's `AppHandle::restart` spawns the
  new binary *before* the old process exits, while the old one still holds `instance.lock`, so
  the new one would see a running instance and exit. The new process must wait for the lock
  (up to ~5 s) instead of handing over, or be started only after the old one is gone.
- On macOS the new instance should be started through LaunchServices (`open`), the way the user
  starts it, so that macOS attributes Input Monitoring to TakTak itself, not to a dying parent.

### Privacy (Milestone 4)

- `frontmostApp` holds only the app in front now. Its changes are never logged at any level,
  never persisted, never kept as a history, and only ever sent to TakTak's own windows. The UI
  must not collect its values either (no "recent apps").
- Only the user's rule list (`settings.appRule`: bundle ids and names) is persisted.
  `list_running_apps` results and icons live in memory only. The icon cache is bounded (256)
  and holds only listed, chosen and running-list apps, never the frontmost app as such.
- `choose_app` reads only the chosen bundle's `Info.plist` (identifier and name) and icon.
- Lock, unlock and session changes are logged at debug level at most, without times kept.
- Nothing new uses the network; the CSP stays as it is (`img-src 'self' data:` already allows the
  icons' `data:` URLs).
- **(M5)** The Mechvibes import reports only pack contents (file and key names from the pack's
  config) and logs only the new pack's id, the number of keys and the kind of a failure: never
  the source path. The source of a pending overwrite is held in memory only.

## UI requirements (Milestone 4)

- **Status texts** (popover status line, Settings), matching the tray: `Muted — screen locked`,
  `Muted — output device changed`, `Silent in <app>` (`Silent in this app` when `frontmostApp`
  is null). The `outputChanged` notice offers Unmute and names `audio.device`, e.g. "TakTak
  muted itself because the sound output changed to AirPods Pro. Unmute to keep typing sounds on
  this device."
- **Per-app rules** (Settings → Apps): mode choice "Everywhere" / "Only in these apps" / "Never
  in these apps"; the list (icon, name, remove) in `settings.appRule.apps` order, icons from
  `list_running_apps`, `choose_app` and `get_app_icons` (generic icon for `null`); "Add app"
  offering the running apps and "Choose app…"; when `frontmostApp` is known and not listed, a
  shortcut to list it. With mode `"only"` and an empty list, warn "No apps listed: TakTak is
  silent everywhere." Show the known limits (Spotlight/Raycast/Alfred, Secure Input). When
  `!rulesSupported`: the section says "Per-app rules aren't available on this system yet." and
  its controls are disabled.
- **Auto-mute** (Settings → General): the switch "Mute when the output device changes" (help: "When
  headphones are plugged in or unplugged, or AirPods connect, TakTak mutes itself until you
  unmute."), and the note "TakTak is always silent while the screen is locked."
- **Onboarding window** (`onboarding` label), minimal and friendly; the words live in
  `src/lib/onboarding.ts` (`COPY`):
  1. The keycap logo, the title "Welcome to TakTak" and ONE line: on macOS "TakTak needs Input
     Monitoring to hear when keys go down. It never sees what you type."; elsewhere "Mechanical
     keyboard sounds as you type, in every app." (Linux without the `input` group: "On Wayland,
     TakTak needs your user in the input group to hear your keys.").
  2. A small badge "Fully offline · never uses the internet" (the full offline sentence as its
     tooltip).
  3. ONE big primary button: "Allow Input Monitoring" (`open_permission_settings`) while
     permission is missing; "Quit & Reopen" (`relaunch`) when `relaunchSuggested`; "Done"
     (`finish_onboarding`) once there is nothing left to do (granted, or `!permissionRequired`).
  4. A live status (`aria-live`) under it, from `permission`: "Then switch on TakTak in the
     list." → (after the button) a spinner and "Waiting for you to switch on TakTak…" → "✓
     You're all set — start typing" (with no restart; a short keycap-confetti celebration when
     it happens while the window is open) and a hint where TakTak lives. When the button
     answered `false` (TakTak not listed): "TakTak isn't in the list? Click + below it and choose
     TakTak." When `relaunchSuggested`: "Almost there — TakTak needs a quick restart." (Windows and
     Linux: "TakTak's key listener couldn't start. A restart usually fixes it.").
  5. Small links under the status, collapsed by default: "Why?" (macOS: why Input Monitoring is
     needed and the privacy promise "TakTak only notices that a key went down or up — never what
     you type. Nothing is recorded, stored or sent anywhere.", plus the full offline sentence;
     Linux: why the `input` group), "Having trouble?" (macOS while TakTak can't listen yet) and
     "Later" (closes, like Done, while something is left to do).
  6. "Having trouble?" opens by itself when `relaunchSuggested`, ~20 s after the user pressed
     "Allow Input Monitoring" with permission still missing, or right away when that press
     answered `false`. It holds: "TakTak isn't in the list? Click + below the list and choose
     TakTak, or drag TakTak into the list." with "Show TakTak in Finder" (`reveal_app`); "On, but
     still no sound? macOS may remember an older copy of TakTak. Remove TakTak with − and add it
     again with +, or run `tccutil reset ListenEvent tech.taktak.app` in Terminal and reopen
     TakTak" (with Copy); "Quit & Reopen" (unless it is already the main button); and the macOS
     12 lock hint.
  7. Linux without the `input` group: the command `sudo usermod -aG input $USER` with Copy, "Run
     it in a terminal, then log out and back in." and its cost (always visible), then Done.
- **Settings → About** (or General): "Show welcome guide" (`open_onboarding`).

## UI requirements (Milestone 5)

- **"Fully offline", loud and clear.** The sentence "TakTak is fully offline — it never uses the
  internet." appears, word for word (`src/lib/offline.ts`), in the onboarding window (as the
  tooltip of its badge "Fully offline · never uses the internet" and in its "Why?" disclosure)
  and at the top of the privacy box in Settings → About; the tray popover's footer shows the short form "Offline ·
  never uses the internet" (the full sentence as its tooltip). The README and the website use
  the same sentence. It must stay true: no network code (CSP `connect-src` limited to IPC, and
  the CI offline guard, `npm run no-network`).
- **Import Mechvibes pack** (Settings → Sounds → Your own packs, next to "Open packs folder"):
  macOS shows one button, "Import Mechvibes pack…" (`kind: "any"`); Windows and Linux show
  "Import Mechvibes pack…" (`"zip"`) and "Import pack folder…" (`"folder"`). The buttons are
  disabled while an import runs. Next to them, the one-line note "Imported packs stay on this
  Mac and are for your personal use only." ("this computer" on Windows and Linux).
  - `imported`: a result row "Imported “<name>”" ("Replaced …" when `replaced`) with
    "<n> keys mapped, <m> with their own release sound.", the format and source, the warnings
    behind a disclosure ("<k> notes"), "Use this pack" (enabled once the pack is listed in
    `packs`; it calls `set_pack`) and a dismiss button.
  - `alreadyImported`: "“<name>” was imported before" (the listed pack's name, else the source)
    with "Replace" (`overwrite_mechvibes_pack`) and "Cancel".
  - Rejections show in the error toast. A cancelled picker (`null`) changes nothing.
- **Personal packs.** A pack whose `license` is `LicenseRef-Personal` shows a "Personal" badge
  (lock icon; tooltip: imported for personal use, stays on this computer, not for sharing) in
  place of the SPDX badge on its card, and "Personal" instead of "User" in the popover's pack
  list. The loader's warning that such a pack is personal-use-only is not listed among the
  pack's `warnings` (the badge says it); other warnings are.
- **Linux `input` group** (`onboarding.inputGroupNeeded`), in the onboarding window's place of
  the permission step: why ("On Wayland, apps can’t hear keys typed into other apps, so TakTak
  reads the keyboard devices directly…"), the command `sudo usermod -aG input $USER` with a Copy
  button, "log out and back in", and the cost in a caution box ("…lets every program you run
  read every keystroke, passwords included. TakTak only uses which key went down or up, and
  never asks for root."), and that saying no only leaves key sounds off. Closing button "Done".
- **Restart texts per platform.** macOS keeps "macOS needs TakTak to restart before it can
  listen." (Input Monitoring is allowed). Windows and Linux (`relaunchSuggested` without
  `permissionRequired`): "TakTak’s key listener couldn’t start. Quitting and reopening TakTak
  usually fixes it; your settings are kept." with Quit & Reopen, in the onboarding window and
  the notice; no troubleshooting about the macOS list, and no "TakTak reminds you the next time
  it starts".

## Hotkey keys and keyboard layouts

An accelerator's key token is named after the US layout (`M`, `Semicolon`, …), but what it
means depends on how the global-shortcut plugin registers it:

| OS | The token registers | The recorder (`src/lib/accelerator.ts`) builds it from |
|---|---|---|
| macOS | a key **position** (virtual key code): `M` is the key right of N on a US keyboard, which types "," on French AZERTY | `KeyboardEvent.code` (the same position) |
| Windows | a **virtual-key code**, assigned by the layout: `M` is the key that types M | the press's virtual-key code (`keyCode`) for letters, digits and punctuation; `code` for other keys |
| Linux (X11) | the key that **types** the token's character | `keyCode` for letters; `code` for the rest |

So the default `CommandOrControl+Alt+Shift+M` is the M key on Windows and Linux, and the US M
position on macOS. On macOS the UI labels letter, digit and punctuation keys with the current
layout's character for that position when the webview exposes the Keyboard Map API
(`navigator.keyboard.getLayoutMap()`, read in `src/lib/keyboard.svelte.ts`). WKWebView does
not, so in the app the labels name the US key, the keycaps' tooltip says so, and Settings →
Shortcuts explains it. Showing the right character there needs the app to translate key codes
with the current input source (e.g. `UCKeyTranslate`) and pass the labels to the UI; that is
not part of this contract yet.

## Where things live

- Rust types: `src-tauri/src/state.rs`; commands: `src-tauri/src/commands.rs` (registered in
  `src-tauri/src/lib.rs`); windows: `src-tauri/src/windows.rs`; tray: `src-tauri/src/tray.rs`;
  single instance on macOS: `src-tauri/src/instance.rs`.
  The state, the control thread (engine, listener, packs) and `state-changed` broadcasting:
  `src-tauri/src/service.rs`, with the testable logic in `catalog.rs` (pack list, fallback,
  hot-reload reactions), `settings.rs` (persistence, clamping, volume curve), `input.rs` (gate,
  permission polling), `loader.rs` (pack decoding thread) and `hotkey.rs`.
- **(M4)** Pure logic is kept out of the platform code and unit-tested: rule evaluation, gate
  composition and the 5 s rule-block timer in `src-tauri/src/rules.rs`; the auto-mute state
  machine and `DeviceWatch` in `src-tauri/src/automute.rs`; `derive()` (auto-mute, rule block,
  `playing`, `onboarding.offer`) in `service.rs`; the onboarding decision (`offer_onboarding`,
  `open_at_startup`) and the `onboarding` window in `windows.rs`. The macOS observers
  (frontmost app and session via `NSWorkspace`, screen lock via the CF distributed center; on
  the main thread), running apps, icons, `Info.plist` reading and the app picker are in
  `src-tauri/src/apps.rs` (objc2 / objc2-app-kit / objc2-foundation / objc2-core-foundation,
  a stub on other OSes). Settings migration and `appRule` leniency are in `settings.rs`.
  `relaunch` is in `src-tauri/src/relaunch.rs`: the new process gets `--relaunch` (wait up to
  5 s for the old instance's lock instead of handing over) and, if the onboarding window was
  open, `--onboarding`; on Windows and Linux the running instance also holds
  `<app data dir>/running.lock` so the new one can wait for it.
- **(M5)** The import: `src-tauri/src/mechvibes.rs` (the one-import-at-a-time slot, the
  summary and error texts, the pending overwrite, the macOS `NSOpenPanel` that takes a folder or
  a `.zip`, and on Windows and Linux the `tauri-plugin-dialog` pickers, used from Rust only: no
  JS API and no capability), commands in `commands.rs`, over
  `taktak_core::pack::import::import_mechvibes`. The personal-license warning is dropped in
  `catalog::summarize`. UI: `src/views/settings/SoundsSection.svelte`, `src/lib/imports.ts`;
  the offline sentence in `src/lib/offline.ts`; the Linux and Windows texts in
  `src/lib/status.ts` and `onboarding.ts`; the tray texts in `tray.rs` (`status_on`).
- **(M4)** UI: Settings → Apps is `src/views/settings/AppsSection.svelte` (with
  `src/components/AddAppPopover.svelte`), the welcome window `src/views/OnboardingView.svelte`;
  the pure helpers are `src/lib/rules.ts`, `onboarding.ts` and `status.ts`. The popover's
  "Edit rules…" opens Settings at the Apps section through a short-lived `localStorage` note
  (`taktak.openSection`, `{ id, at }`, ignored after 10 s), since `open_settings` takes no
  arguments.
- Headless check: `taktak --selftest [--allow-no-audio] [--packs <dir>]` scans and loads every
  bundled pack, drives the engine and the real service (pack switches, a preview opening and
  closing the output, a hot-reloaded user pack that breaks, is named as broken on a fresh
  start and is deleted, settings persistence) without windows, tray or keyboard listener,
  prints one line per check and exits 0 when nothing failed. **(M5)** It also imports a
  synthetic Mechvibes pack into its user packs folder (`mechvibes::import`, without the picker)
  and checks that it is listed as a personal pack, then recognized as already imported and
  replaced on overwrite.
  `TAKTAK_NO_INPUT=1 taktak` runs the app without the keyboard listener (no permission prompt);
  `TAKTAK_LOG=debug` raises the stderr log level.
- TypeScript types: `src/lib/types.ts` (including a `Commands` map of every command's args and
  result); typed wrappers and `onStateChanged`: `src/lib/api.ts`.
- Browser development: `npm run dev` and open <http://localhost:1420>. Outside Tauri,
  `src/lib/api.ts` answers from an in-memory mock (`src/lib/mock.ts`) listing the bundled packs
  from `packs/*/pack.json`; `?window=tray` picks the popover view (**(M4)** `?window=onboarding`
  the onboarding view), and `?scenario=denied,fault,invalid,empty` (any combination) simulates
  missing permission, an audio fault that recovers, broken/user packs and no packs. **(M4)** More
  scenarios: `firstrun` (onboarding not done yet; by default the mock is a returning user),
  `relaunch` (permission granted per macOS but the listener refused: `relaunchSuggested`;
  `relaunch` fixes it after 1.5 s), `rules` (mode "never" listing Slack and zoom.us, with Slack
  in front), `switching` (the frontmost app changes every 3 s), `locked` (screen locked, unlocks
  after 4 s), `outputchange` (`muteOnOutputChange` on; 3 s in, the output moves to AirPods Pro)
  and `unsupported` (no per-app rules and no permission step, like Windows and Linux).
  **(M5)** `inputgroup` (Linux without the `input` group) and `listenerfail` (a Windows or Linux
  listener that failed to start; `relaunch` fixes it after 1.5 s). `unlisted` (like `denied`,
  but macOS adds nothing to the Input Monitoring list: `open_permission_settings` answers `false`
  until permission is granted, which happens 6 s after `reveal_app`, as if the user had added
  TakTak by hand). `denied` answers `true`. The mock's
  `import_mechvibes_pack` answers in turn: an import (the pack is listed 0.7 s later), the same
  pack again (`alreadyImported`; `overwrite_mechvibes_pack` replaces it), a pack with no usable
  sounds (rejected) and a cancelled picker (`null`). The mock
  lists ten realistic running apps (with SVG `data:` icons, one without an icon), and its
  `choose_app` returns Microsoft Word, Discord, Obsidian, then `null` (cancelled), in turn.
  `tauri build` output contains no mock code.
