# TakTak app ↔ UI contract (Milestone 3)

The Tauri app crate (`src-tauri`, Rust) owns all state and talks to `taktak-core`. The web
UI (`src/`, Svelte 5 + TypeScript) is a thin view: it renders `AppState`, calls commands, and
re-renders on the `state-changed` event. The UI never reads files, never touches audio, and
never sees what the user types.

## Windows

| Label | Purpose | Lifecycle |
|---|---|---|
| `tray` | Small popover anchored to the menu-bar/tray icon (left click). | Created on open. Hidden on blur, on a second tray click and on Escape (`hide_tray`). Destroyed once it has stayed hidden for 60 s, and 1 s after Settings opens; the next click creates it again (~0.2 s). |
| `settings` | Full settings window, title "TakTak Settings". | Created on demand; destroyed on close (saves memory). |

Each webview keeps WebKit helper processes alive (WebContent alone is 20–40 MB), which is why
neither window outlives its use. The UI must not rely on the popover's page staying loaded
between opens.

There is no main window and, on macOS, no Dock icon (accessory app). Both windows load the
same bundle; the UI picks the view from `window.__TAURI_INTERNALS__.metadata.currentWindow.label`
(via `getCurrentWindow().label`).

Opening TakTak again while it runs shows (or focuses) the settings window: on macOS from
Finder, Spotlight, Launchpad or `open` (the running app gets a reopen event), and on every OS
when a second process starts (`open -n`, running the binary again), which hands over and
exits. On macOS, hiding the popover with a tray click or Escape also hands the keyboard back to
the app the user was typing in (TakTak hides itself unless Settings is showing).

The tray icon's **right-click** opens a native menu: `Sounds On` (check), `Pack ▸` (radio
list), `Mute` (check, shows the hotkey), `Settings…`, `Quit TakTak`. Tooltip: `TakTak`.

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
  muted: boolean;            // hotkey/tray mute (separate from enabled)
  playing: boolean;          // enabled && !muted && permission granted && audio ok
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
}

interface LatencyReport {     // timings only, never key identities
  count: number;
  totalP50Ms: number; totalP95Ms: number; totalMaxMs: number;
  inputP50Ms: number; queueP50Ms: number; outputMs: number;
}
```

## Commands (`invoke`)

All return `Promise<AppState>` unless noted, and reject with a user-facing string on error.

| Command | Args | Notes |
|---|---|---|
| `get_state` | — | |
| `set_enabled` | `{ enabled: boolean }` | |
| `set_muted` | `{ muted: boolean }` | Same as the hotkey. |
| `set_pack` | `{ id: string }` | Loads off the main thread; state updates again via event when loaded. Persisted. |
| `set_master_volume` | `{ value: number }` | Clamped 0..1. Persisted (debounced). |
| `set_press_volume` / `set_release_volume` | `{ value: number }` | Clamped 0..1. |
| `set_variant_mode` | `{ mode: VariantMode }` | |
| `set_humanize` | `{ value: number }` | Clamped 0..1. |
| `set_mute_hotkey` | `{ accelerator: string \| null }` | Validates and registers; rejects with a message if invalid or already taken by the system. `null` or `""` removes the hotkey. Besides unparseable accelerators it rejects media/volume keys and any key other than F1–F24 without Ctrl, Alt/Option or Cmd/Win (Shift alone would swallow typing); F1–F24 work alone or with Shift only. On rejection the previous hotkey stays registered. Success clears `muteHotkeyError`. See "Hotkey keys and keyboard layouts" below. |
| `set_launch_at_login` | `{ enabled: boolean }` | |
| `preview_pack` | `{ id: string }` | Plays that pack's preview clip once (does not change the active pack), opening the output for it if it is closed. Returns `void` once the clip plays. Rejects with a user-facing message when it cannot: no output device, the pack's sounds cannot be decoded, or it has nothing to preview. A clip still decoding after 5 s resolves and plays when ready. A preview replaced by another or stopped resolves too. |
| `stop_preview` | — | Returns `void`. |
| `open_settings` | — | Shows/creates the settings window. Returns `void`. |
| `hide_tray` | — | Hides the tray popover (use it for Escape instead of hiding the window from JS): on macOS it also hands the keyboard back to the app the user was in. Returns `void`. |
| `open_user_packs_dir` | — | Creates it if needed and reveals it in Finder/Explorer. Returns `void`. |
| `open_permission_settings` | — | macOS: opens Privacy & Security → Input Monitoring. Returns `void`. |
| `get_latency` | — | Returns `LatencyReport \| null` (null until 5 presses measured since the window opened). |
| `quit` | — | Exits the app. Returns `void` (in practice the promise never settles). |

## Events

| Event | Payload | When |
|---|---|---|
| `state-changed` | `AppState` | After any change: commands, tray menu actions, hotkey, pack hot-reload, permission granted, audio fault/recovery, pack load finished. |

## Behaviour rules

- Settings persist to `<app config dir>/settings.json` (atomic write, debounced ~300 ms).
  Missing/corrupt file → defaults (`deep-thock`, master 0.7, press/release 1.0, consistent,
  humanize 0.25, hotkey `CommandOrControl+Alt+Shift+M`, launch at login off, enabled on).
- If the saved pack no longer exists, fall back to `deep-thock`, then to any pack, then to the
  built-in click; set `activePackError` when a selected pack fails to load. `settings.packId`
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
  (`enabled && !muted`, permission granted) or a preview plays. An open stream keeps the
  audio device awake, which costs `coreaudiod` 5–9 % of a core even in silence. While it is
  closed, the playing pack stays decoded, `audio` shows the default device (state `ok`, or
  `fault` when there is none), and turning sounds on or unmuting opens it again in ~0.1 s.
- Missing Input Monitoring permission: `permission = "denied"`, `playing = false`; poll and
  start listening automatically once granted (full onboarding UI is Milestone 4). A key
  listener that fails to start although permission is granted (macOS sometimes refuses the tap
  right after the user grants it) also shows `"denied"`, and is retried every 30 s.
  `"unknown"` means the platform has no key listener (Windows and Linux for now), or the app runs
  with `TAKTAK_NO_INPUT=1`.
- Mute hotkey: a saved `muteHotkey` that cannot be registered at startup stays in the settings
  but sets `muteHotkeyError` (see `AppState`).
- Privacy: no network access (CSP `default-src 'self'`; no remote URLs), no logging of keys,
  the hotkey recorder only reads key events inside the settings window while recording.

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
- Headless check: `taktak --selftest [--allow-no-audio] [--packs <dir>]` scans and loads every
  bundled pack, drives the engine and the real service (pack switches, a preview opening and
  closing the output, a hot-reloaded user pack that breaks, is named as broken on a fresh
  start and is deleted, settings persistence) without windows, tray or keyboard listener,
  prints one line per check and exits 0 when nothing failed.
  `TAKTAK_NO_INPUT=1 taktak` runs the app without the keyboard listener (no permission prompt);
  `TAKTAK_LOG=debug` raises the stderr log level.
- TypeScript types: `src/lib/types.ts` (including a `Commands` map of every command's args and
  result); typed wrappers and `onStateChanged`: `src/lib/api.ts`.
- Browser development: `npm run dev` and open <http://localhost:1420>. Outside Tauri,
  `src/lib/api.ts` answers from an in-memory mock (`src/lib/mock.ts`) listing the bundled packs
  from `packs/*/pack.json`; `?window=tray` picks the popover view, and
  `?scenario=denied,fault,invalid,empty` (any combination) simulates missing permission, an
  audio fault that recovers, broken/user packs and no packs. `tauri build` output contains no
  mock code.
