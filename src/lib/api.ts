// Typed access to the Rust app (docs/ui-contract.md). Inside Tauri every call goes over IPC;
// in a plain browser (`npm run dev`) a mock backend with the bundled packs answers instead,
// so the UI can be built without the app. The mock is loaded lazily and never inside Tauri.

import { type InvokeArgs, invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type {
  AppState,
  Command,
  CommandArgs,
  CommandResult,
  LatencyReport,
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

/** The label of the window this UI runs in. In a browser: `?window=tray`, else "settings". */
export function windowLabel(): WindowLabel {
  const label = isTauri
    ? getCurrentWindow().label
    : new URLSearchParams(window.location.search).get('window');
  return label === 'tray' ? 'tray' : 'settings';
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
export const openPermissionSettings = (): Promise<void> => call('open_permission_settings');
/** null until 5 presses have been measured since the window opened. */
export const getLatency = (): Promise<LatencyReport | null> => call('get_latency');
export const quit = (): Promise<void> => call('quit');
