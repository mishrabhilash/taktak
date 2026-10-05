// The app ↔ UI contract, exactly as docs/ui-contract.md. The Rust side mirrors these in
// src-tauri/src/state.rs (serde camelCase). Change both, and the doc, together.

export type VariantMode = 'consistent' | 'random';

export interface Settings {
  /** Master on/off (tray toggle). */
  enabled: boolean;
  /** Active pack id. */
  packId: string;
  /** 0..1, slider position; engine gain = masterVolume^2. */
  masterVolume: number;
  /** 0..1, linear. */
  pressVolume: number;
  /** 0..1, linear. */
  releaseVolume: number;
  /** Default "consistent". */
  variantMode: VariantMode;
  /** 0..1, default 0.25. */
  humanize: number;
  /** Tauri accelerator, e.g. "CommandOrControl+Alt+Shift+M"; null = none. */
  muteHotkey: string | null;
  launchAtLogin: boolean;
  /** (M4) Per-app rules; default `{ mode: "everywhere", apps: [] }`. */
  appRule: AppRule;
  /** (M4) Auto-mute when the default output device changes; default false. */
  muteOnOutputChange: boolean;
  /**
   * Close the sound output after this many minutes without a key press (battery: an open output
   * keeps the audio device awake); the next key press reopens it and plays. 0 = never; default 5,
   * at most 1440.
   */
  idleSleepMinutes: number;
  /** (M4) The onboarding window was closed at least once; default false. */
  onboardingDone: boolean;
}

/**
 * (M4) How per-app rules use the list: "everywhere" ignores it (default), "only" plays sounds
 * only while a listed app is frontmost, "never" is silent while a listed app is frontmost.
 */
export type AppRuleMode = 'everywhere' | 'only' | 'never';

/** (M4) An app as TakTak identifies it. */
export interface AppRef {
  /** macOS bundle identifier, e.g. "com.apple.Safari". */
  id: string;
  /** Display name, e.g. "Safari". */
  name: string;
}

/**
 * (M4) One listed app. An object rather than a bare id so per-app overrides (e.g. a pack or a
 * volume) can be added later as optional fields, absent = follow the global setting, without
 * migrating settings.json. Readers ignore fields they do not know.
 */
export interface AppRuleEntry {
  /** macOS bundle identifier; unique within the list. */
  id: string;
  /** Display name when it was added (never empty: the id stands in). */
  name: string;
}

/** (M4) Per-app rules: one list for both "only" and "never", in append order, at most 200. */
export interface AppRule {
  mode: AppRuleMode;
  apps: AppRuleEntry[];
}

/** (M4) A pickable app (`list_running_apps`, `choose_app`). */
export interface AppInfo {
  /** Bundle identifier. */
  id: string;
  /** Localized display name. */
  name: string;
  /** "data:image/png;base64,…", 32 × 32 px (16 pt @2x); null = no icon (show a generic one). */
  iconDataUrl: string | null;
}

export type PackOrigin = 'bundled' | 'user';

export interface PackSummary {
  id: string;
  name: string;
  author: string;
  /** SPDX. */
  license: string;
  description: string | null;
  attribution: string | null;
  origin: PackOrigin;
  /** Any release sounds at all. */
  hasRelease: boolean;
  /** Has per-key entries. */
  perKey: boolean;
  /** Human-readable, already escaped. */
  warnings: string[];
}

export interface InvalidPack {
  /** Path shown to the user. */
  location: string;
  /** Formatted Problem lines. */
  problems: string[];
}

export type Permission = 'granted' | 'denied' | 'unknown';

export type AudioState = 'ok' | 'starting' | 'fault';

export interface AudioStatus {
  device: string | null;
  sampleRate: number | null;
  bufferFrames: number | null;
  state: AudioState;
  /** User-facing, e.g. "Output device disconnected — reconnecting…". */
  message: string | null;
}

/**
 * (M4) Why TakTak muted itself. "screenLocked": the screen is locked or this user session is
 * inactive; clears by itself on unlock. "outputChanged": the default output device changed while
 * `muteOnOutputChange` was on; stays until the user unmutes (or turns sounds or the setting off).
 */
export type AutoMute = 'screenLocked' | 'outputChanged';

/** (M4) What the onboarding window needs to know. */
export interface OnboardingStatus {
  /** Offer the onboarding: `!settings.onboardingDone`, or permission required and denied (live). */
  offer: boolean;
  /** The platform needs a permission the user grants (macOS with the key listener on). */
  permissionRequired: boolean;
  /**
   * The permission looks granted (macOS: Input Monitoring; Windows and Linux X11 have none to
   * grant) but the key listener cannot start: suggest Quit & Reopen.
   */
  relaunchSuggested: boolean;
  /**
   * (M5) Linux: the key listener must read the keyboard devices (Wayland, or TAKTAK_INPUT=evdev)
   * and cannot, because the user is not in the `input` group. Explain the opt-in and its cost.
   */
  inputGroupNeeded: boolean;
}

export interface AppState {
  version: string;
  settings: Settings;
  /** The manual mute: hotkey, tray, set_muted (separate from enabled). Auto-mute never changes it. */
  muted: boolean;
  /**
   * A key press makes a sound now: enabled && !muted && autoMute === null && !ruleBlocked &&
   * permission granted && audio ok.
   */
  playing: boolean;
  /** Sorted by name. */
  packs: PackSummary[];
  invalidPacks: InvalidPack[];
  /**
   * The pack whose sounds are loaded now: the selected one, a fallback, or the selected pack's
   * last working version. null = the built-in click (also before the first pack has loaded).
   */
  playingPackId: string | null;
  /** Set when the selected pack is not the one playing; says why and what plays instead. */
  activePackError: string | null;
  /** Set when the saved mute hotkey could not be registered at startup; cleared by a successful set_mute_hotkey. */
  muteHotkeyError: string | null;
  userPacksDir: string | null;
  permission: Permission;
  audio: AudioStatus;
  /**
   * The output is paused: no key went down for `settings.idleSleepMinutes`. The next key press
   * reopens it and plays (a few tens of ms late). `playing` is unaffected.
   */
  audioAsleep: boolean;
  /**
   * (M4) The app in front now, TakTak's own windows excluded; null when unknown, without a
   * bundle id, or when rules are unsupported. Current value only: never collect it into a history.
   */
  frontmostApp: AppRef | null;
  /** (M4) `settings.appRule` silences `frontmostApp` right now. */
  ruleBlocked: boolean;
  /** (M4) Why TakTak muted itself; null = not auto-muted. */
  autoMute: AutoMute | null;
  /** (M4) Per-app rules work on this platform (macOS); elsewhere they are kept but ignored. */
  rulesSupported: boolean;
  /** (M4) */
  onboarding: OnboardingStatus;
}

/** Timings only, never key identities. */
export interface LatencyReport {
  count: number;
  totalP50Ms: number;
  totalP95Ms: number;
  totalMaxMs: number;
  inputP50Ms: number;
  queueP50Ms: number;
  outputMs: number;
}

/**
 * (M5) Which picker `import_mechvibes_pack` opens. macOS shows one panel for a folder or a .zip
 * whatever the kind; Windows and Linux pickers take one or the other ("any" = the .zip picker).
 */
export type PickKind = 'any' | 'folder' | 'zip';

/** (M5) What a Mechvibes import wrote. Every string is pack content, already escaped. */
export interface ImportSummary {
  /** The new pack's id ("mv-…"); it shows up in `AppState.packs` within about a second. */
  id: string;
  name: string;
  /** The source folder or .zip file name. */
  source: string;
  /** The Mechvibes flavour, e.g. "Mechvibes v2". */
  format: string;
  /** Keys with a sound of their own. */
  keysMapped: number;
  /** How many of them also have their own release sound. */
  keysWithRelease: number;
  /** Sound files written (the preview not counted). */
  soundsWritten: number;
  /** An earlier import of the same pack was replaced. */
  replaced: boolean;
  /** Skipped keys, missing or unreadable files, other notes; at most 30 lines. */
  warnings: string[];
}

/**
 * (M5) The result of `import_mechvibes_pack` / `overwrite_mechvibes_pack`. "alreadyImported":
 * this pack (same id, same source name) was imported before and nothing changed; offer to
 * replace it with `overwrite_mechvibes_pack`.
 */
export type MechvibesImport =
  | { outcome: 'imported'; pack: ImportSummary }
  | { outcome: 'alreadyImported'; id: string; source: string };

/** Window labels; the UI picks its view from the label of the window it runs in. */
export type WindowLabel = 'tray' | 'settings' | 'onboarding';

/** Every command: its `invoke` arguments (`void` = none) and what it resolves with. */
export interface Commands {
  get_state: { args: void; result: AppState };
  set_enabled: { args: { enabled: boolean }; result: AppState };
  set_muted: { args: { muted: boolean }; result: AppState };
  set_pack: { args: { id: string }; result: AppState };
  set_master_volume: { args: { value: number }; result: AppState };
  set_press_volume: { args: { value: number }; result: AppState };
  set_release_volume: { args: { value: number }; result: AppState };
  set_variant_mode: { args: { mode: VariantMode }; result: AppState };
  set_humanize: { args: { value: number }; result: AppState };
  set_mute_hotkey: { args: { accelerator: string | null }; result: AppState };
  set_launch_at_login: { args: { enabled: boolean }; result: AppState };
  preview_pack: { args: { id: string }; result: void };
  stop_preview: { args: void; result: void };
  open_settings: { args: void; result: void };
  hide_tray: { args: void; result: void };
  open_user_packs_dir: { args: void; result: void };
  /** Resolves with whether TakTak is in the Input Monitoring list (true where there is none). */
  open_permission_settings: { args: void; result: boolean };
  get_latency: { args: void; result: LatencyReport | null };
  quit: { args: void; result: void };
  // Milestone 4
  list_running_apps: { args: void; result: AppInfo[] };
  choose_app: { args: void; result: AppInfo | null };
  get_app_icons: { args: { ids: string[] }; result: Record<string, string | null> };
  set_app_rule_mode: { args: { mode: AppRuleMode }; result: AppState };
  add_rule_app: { args: { id: string; name: string }; result: AppState };
  remove_rule_app: { args: { id: string }; result: AppState };
  set_mute_on_output_change: { args: { enabled: boolean }; result: AppState };
  set_idle_sleep_minutes: { args: { minutes: number }; result: AppState };
  /**
   * macOS: the current layout's label per `KeyboardEvent.code` for the letter, digit and
   * punctuation key positions a hotkey can name; null elsewhere or when it cannot be read.
   */
  key_labels: { args: void; result: Record<string, string> | null };
  open_onboarding: { args: void; result: void };
  finish_onboarding: { args: void; result: AppState };
  relaunch: { args: void; result: void };
  // Milestone 5
  import_mechvibes_pack: { args: { kind: PickKind }; result: MechvibesImport | null };
  overwrite_mechvibes_pack: { args: void; result: MechvibesImport };
  reveal_app: { args: void; result: void };
}

export type Command = keyof Commands;
export type CommandArgs<C extends Command> = Commands[C]['args'];
export type CommandResult<C extends Command> = Commands[C]['result'];

/** Events the app emits, with their payloads. */
export interface Events {
  'state-changed': AppState;
  /** macOS: the user switched keyboard layout; the new `key_labels` answer. */
  'key-labels-changed': Record<string, string> | null;
}
