// The latest AppState, shared by every component in this window. `state-changed` keeps it
// current (the event reaches every window: tray popover and settings); a control the user
// changes shows its new value at once (see optimistic.ts) and the app's state wins after.

import { errorMessage, getState, onStateChanged } from './api';
import { Optimistic, type OptimisticKey, type OptimisticValue } from './optimistic';
import { throttle } from './throttle';
import type { AppState } from './types';

/** Slider sends per second, at most ~30. */
const SEND_INTERVAL_MS = 33;
/** How long an error message stays up unless dismissed. */
const ERROR_MS = 8000;

class AppStore {
  #version = $state(0);
  /** The optimistic layer; `state` is its view. */
  readonly core = new Optimistic(() => {
    this.#version += 1;
  });
  /** The last command's rejection message, shown until dismissed or replaced. */
  error = $state<string | null>(null);

  /** What the UI renders: null until the first state arrives. */
  get state(): AppState | null {
    void this.#version;
    return this.core.view;
  }
}

export const app = new AppStore();

let errorTimer: ReturnType<typeof setTimeout> | undefined;

/** Shows a user-facing error for a while. */
export function showError(error: unknown): void {
  clearTimeout(errorTimer);
  app.error = errorMessage(error);
  errorTimer = setTimeout(dismissError, ERROR_MS);
}

export function dismissError(): void {
  clearTimeout(errorTimer);
  app.error = null;
}

let connected = false;

/** Subscribes to `state-changed` and loads the initial state. Safe to call more than once. */
export async function connect(): Promise<void> {
  if (connected) return;
  connected = true;
  try {
    // Subscribe first; a reply overtaken by an event is dropped (Optimistic.reply).
    await onStateChanged((state) => app.core.event(state));
    await refresh();
  } catch (e) {
    showError(e);
  }
}

/** Fetches the state again (after a failed command, to undo its optimistic value). */
export async function refresh(): Promise<void> {
  const at = app.core.stamp;
  app.core.reply(await getState(), at);
}

/** Runs a command and shows its rejection message, if any. Resolves to undefined on error. */
export async function run<T>(command: Promise<T>): Promise<T | undefined> {
  try {
    return await command;
  } catch (e) {
    showError(e);
    return undefined;
  }
}

/**
 * Sends a command that replies with the new state and shows that state at once (unless an
 * event overtook it), so the UI never flashes the old value. Rejects like the command.
 */
export async function apply(command: () => Promise<AppState>): Promise<AppState> {
  const at = app.core.stamp;
  const reply = await command();
  app.core.reply(reply, at);
  return reply;
}

/**
 * Shows `value` for `key` at once and sends it. On rejection the app's value comes back and
 * the message is shown. Resolves to whether the command succeeded.
 */
export async function set<K extends OptimisticKey>(
  key: K,
  value: OptimisticValue<K>,
  send: (value: OptimisticValue<K>) => Promise<AppState>,
): Promise<boolean> {
  app.core.sent(key, value);
  const at = app.core.stamp;
  try {
    const reply = await send(value);
    app.core.reply(reply, at);
    app.core.settled(key, true);
    return true;
  } catch (e) {
    app.core.settled(key, false);
    showError(e);
    await refresh().catch(() => undefined);
    return false;
  }
}

export type LevelKey = 'masterVolume' | 'pressVolume' | 'releaseVolume' | 'humanize';

/** A slider's link to the app: every move shows at once; sends are throttled. */
export interface LevelControl {
  /** While dragging. */
  input(value: number): void;
  /** On release (or a keyboard step): the final value is always sent. */
  commit(value: number): void;
}

export function levelControl(
  key: LevelKey,
  send: (value: number) => Promise<AppState>,
): LevelControl {
  const throttled = throttle<number>((value) => void set(key, value, send), SEND_INTERVAL_MS);
  return {
    input(value) {
      app.core.show(key, value);
      throttled.call(value);
    },
    commit(value) {
      app.core.show(key, value);
      throttled.call(value);
      throttled.flush();
    },
  };
}
