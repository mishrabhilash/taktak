// Typed access to the Rust app (docs/ui-contract.md). Inside Tauri every call goes over IPC;
// in a plain browser (`npm run dev`) a mock backend with the bundled packs answers instead,
// so the UI can be built without the app. The mock is loaded lazily and never inside Tauri.

import { type InvokeArgs, invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type {
  AppInfo,
  AppRef,
  AppRuleMode,
  AppState,
  Command,
  CommandArgs,
  CommandResult,
  LatencyReport,
  MechvibesImport,
  PickKind,
  VariantMode,
  WindowLabel,
} from './types';

/** What answers the UI: the Tauri app, or the in-browser mock. */
export interface Backend {
  call<C extends Command>(command: C, args: CommandArgs<C>): Promise<CommandResult<C>>;
  onStateChanged(listener: (state: AppState) => void): Promise<() => void>;
}

/** True inside the Tauri webview, false in a plain browser. */
export const isTauri: boolean =
  typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

const tauriBackend: Backend = {
  call: (command, args) => invoke(command, args as InvokeArgs | undefined),
  onStateChanged: (listener) => listen<AppState>('state-changed', (e) => listener(e.payload)),
};

/** False in `tauri build` output, so the shipped app carries no mock code or mock data. */
const MOCK_AVAILABLE = !(import.meta.env.PROD && import.meta.env.TAURI_ENV_PLATFORM);

let backendPromise: Promise<Backend> | undefined;

/** The backend in use (created on first use). */
export function backend(): Promise<Backend> {
  backendPromise ??= isTauri
    ? Promise.resolve(tauriBackend)
    : MOCK_AVAILABLE
      ? import('./mock').then((m) => m.createMockBackend())
      : Promise.reject(new Error('TakTak: this page only works inside the TakTak app.'));
  return backendPromise;
}

async function call<C extends Command>(
  command: C,
  ...args: CommandArgs<C> extends void ? [] : [CommandArgs<C>]
): Promise<CommandResult<C>> {
  return (await backend()).call(command, args[0] as CommandArgs<C>);
}

/** Calls `listener` with every `state-changed` payload. Resolves to an unsubscribe function. */
export async function onStateChanged(listener: (state: AppState) => void): Promise<() => void> {
  return (await backend()).onStateChanged(listener);
}

/**
 * The label of the window this UI runs in. In a browser: `?window=tray` or `?window=onboarding`,
 * else "settings".
 */
export function windowLabel(): WindowLabel {
  const label = isTauri
    ? getCurrentWindow().label
    : new URLSearchParams(window.location.search).get('window');
  return label === 'tray' || label === 'onboarding' ? label : 'settings';
}

/** The user-facing message of a rejected command (commands reject with a string). */
export function errorMessage(error: unknown): string {
  if (typeof error === 'string') return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

// One wrapper per command, in contract order.

export const getState = (): Promise<AppState> => call('get_state');
export const setEnabled = (enabled: boolean): Promise<AppState> => call('set_enabled', { enabled });
export const setMuted = (muted: boolean): Promise<AppState> => call('set_muted', { muted });
/** Resolves at once; another `state-changed` follows when the pack has loaded. */
export const setPack = (id: string): Promise<AppState> => call('set_pack', { id });
export const setMasterVolume = (value: number): Promise<AppState> =>
  call('set_master_volume', { value });
export const setPressVolume = (value: number): Promise<AppState> =>
  call('set_press_volume', { value });
export const setReleaseVolume = (value: number): Promise<AppState> =>
  call('set_release_volume', { value });
export const setVariantMode = (mode: VariantMode): Promise<AppState> =>
  call('set_variant_mode', { mode });
export const setHumanize = (value: number): Promise<AppState> => call('set_humanize', { value });
/** Rejects with a message if the accelerator is invalid or taken by the system. */
export const setMuteHotkey = (accelerator: string | null): Promise<AppState> =>
  call('set_mute_hotkey', { accelerator });
export const setLaunchAtLogin = (enabled: boolean): Promise<AppState> =>
  call('set_launch_at_login', { enabled });
/** Plays the pack's preview clip once without changing the active pack. */
export const previewPack = (id: string): Promise<void> => call('preview_pack', { id });
export const stopPreview = (): Promise<void> => call('stop_preview');
export const openSettings = (): Promise<void> => call('open_settings');
/** Hides the tray popover (Escape); on macOS it also hands the keyboard back. */
export const hideTray = (): Promise<void> => call('hide_tray');
export const openUserPacksDir = (): Promise<void> => call('open_user_packs_dir');
/**
 * macOS: asks macOS to list TakTak under Input Monitoring (if it never decided), then opens that
 * pane. Resolves with whether TakTak is in the list now: false when macOS added nothing (the
 * user then adds it with +, see `revealApp`). Always true elsewhere.
 */
export const openPermissionSettings = (): Promise<boolean> => call('open_permission_settings');
/** null until 5 presses have been measured since the window opened. */
export const getLatency = (): Promise<LatencyReport | null> => call('get_latency');
export const quit = (): Promise<void> => call('quit');

// Milestone 4: per-app rules, auto-mute, onboarding.

/** Running regular apps (TakTak excluded), sorted by name, with cached icons. [] if unsupported. */
export const listRunningApps = (): Promise<AppInfo[]> => call('list_running_apps');
/** The native app picker; null when cancelled. Does not add the app (see `addRuleApp`). */
export const chooseApp = (): Promise<AppInfo | null> => call('choose_app');
/** Icons of installed apps by bundle id (for listed apps that are not running); null = none. */
export const getAppIcons = (ids: string[]): Promise<Record<string, string | null>> =>
  call('get_app_icons', { ids });
export const setAppRuleMode = (mode: AppRuleMode): Promise<AppState> =>
  call('set_app_rule_mode', { mode });
/** Appends the app to the rule list; already listed = no change. Rejects with a message. */
export const addRuleApp = (app: AppRef): Promise<AppState> =>
  call('add_rule_app', { id: app.id, name: app.name });
export const removeRuleApp = (id: string): Promise<AppState> => call('remove_rule_app', { id });
/** Turning it off also clears an "outputChanged" auto-mute. */
export const setMuteOnOutputChange = (enabled: boolean): Promise<AppState> =>
  call('set_mute_on_output_change', { enabled });
export const openOnboarding = (): Promise<void> => call('open_onboarding');
/** Marks the onboarding done and closes its window. */
export const finishOnboarding = (): Promise<AppState> => call('finish_onboarding');
/** Quits and starts TakTak again (the promise never settles in the app). */
export const relaunch = (): Promise<void> => call('relaunch');

// Milestone 5: importing Mechvibes packs.

/**
 * Opens the native picker for a Mechvibes pack folder or .zip and imports it into the user packs
 * folder (it is listed within about a second). null when the picker was cancelled. Rejects with
 * a message (not a pack, unsupported, no usable sounds, an import already running).
 */
export const importMechvibesPack = (kind: PickKind): Promise<MechvibesImport | null> =>
  call('import_mechvibes_pack', { kind });
/** Replaces the earlier import the last `importMechvibesPack` reported as "alreadyImported". */
export const overwriteMechvibesPack = (): Promise<MechvibesImport> =>
  call('overwrite_mechvibes_pack');

/** Shows TakTak itself (macOS: TakTak.app) in Finder, to drag it into the Input Monitoring list. */
export const revealApp = (): Promise<void> => call('reveal_app');
