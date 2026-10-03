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

export interface AppState {
  version: string;
  settings: Settings;
  /** Hotkey/tray mute (separate from enabled). */
  muted: boolean;
  /** enabled && !muted && permission granted && audio ok. */
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

/** Window labels; the UI picks its view from the label of the window it runs in. */
export type WindowLabel = 'tray' | 'settings';

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
  open_permission_settings: { args: void; result: void };
  get_latency: { args: void; result: LatencyReport | null };
  quit: { args: void; result: void };
}

export type Command = keyof Commands;
export type CommandArgs<C extends Command> = Commands[C]['args'];
export type CommandResult<C extends Command> = Commands[C]['result'];

/** Events the app emits, with their payloads. */
export interface Events {
  'state-changed': AppState;
}
