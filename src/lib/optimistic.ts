// Optimistic UI over the app's state. The latest `AppState` from the app (a `state-changed`
// event, or a command reply that nothing newer has overtaken) is the truth; on top of it, a
// control the user just changed shows the new value at once, until the app confirms it, the
// command fails, or a short grace period passes. A slider that sends many values in a row is
// never pulled back by the replies to its older values.
//
// Plain TypeScript (no runes) so it can be unit-tested; `store.svelte.ts` makes it reactive.

import { ruleBlocks } from './rules';
import type { AppState, Settings } from './types';

/** The fields a control can change optimistically. */
export type OptimisticKey =
  | 'enabled'
  | 'packId'
  | 'masterVolume'
  | 'pressVolume'
  | 'releaseVolume'
  | 'variantMode'
  | 'humanize'
  | 'launchAtLogin'
  | 'muteOnOutputChange'
  | 'appRule'
  | 'muted';

export type OptimisticValue<K extends OptimisticKey> = K extends keyof Settings
  ? Settings[K]
  : boolean;

/** How long a confirmed-by-reply value may wait for the app's state to agree. */
export const GRACE_MS = 800;
/** How long a shown-but-not-yet-sent value is kept if it is never sent. */
export const HOLD_MS = 2000;

interface Entry {
  value: unknown;
  /** Commands carrying this field that have not settled yet. */
  inflight: number;
  /** Once nothing is in flight: drop the entry at this time even if the app disagrees. */
  until: number;
}

/** The value of `key` in `state`. */
export function fieldOf(state: AppState, key: OptimisticKey): unknown {
  return key === 'muted' ? state.muted : state.settings[key];
}

function same(a: unknown, b: unknown): boolean {
  if (typeof a === 'number' && typeof b === 'number') return Math.abs(a - b) < 1e-9;
  // `appRule`: small plain data, compared by value.
  if (typeof a === 'object' && a !== null && typeof b === 'object' && b !== null) {
    return JSON.stringify(a) === JSON.stringify(b);
  }
  return a === b;
}

/**
 * `state` with `values` applied, and what follows from them like the app computes it: any mute
 * change, turning sounds on and turning `muteOnOutputChange` off clear an "outputChanged"
 * auto-mute; a rule edit re-evaluates `ruleBlocked`; `playing` follows.
 */
export function withValues(
  state: AppState,
  values: ReadonlyMap<OptimisticKey, unknown>,
): AppState {
  if (values.size === 0) return state;
  const settings = { ...state.settings } as Record<string, unknown>;
  let muted = state.muted;
  for (const [key, value] of values) {
    if (key === 'muted') muted = value as boolean;
    else settings[key] = value;
  }
  const next: AppState = { ...state, settings: settings as unknown as Settings, muted };
  if (
    state.autoMute === 'outputChanged' &&
    (values.has('muted') ||
      values.get('enabled') === true ||
      values.get('muteOnOutputChange') === false)
  ) {
    next.autoMute = null;
  }
  if (values.has('appRule')) {
    next.ruleBlocked = ruleBlocks(next.settings.appRule, state.frontmostApp, state.rulesSupported);
  }
  if (
    values.has('enabled') ||
    values.has('muted') ||
    values.has('appRule') ||
    values.has('muteOnOutputChange')
  ) {
    next.playing =
      next.settings.enabled &&
      !muted &&
      next.autoMute === null &&
      !next.ruleBlocked &&
      state.permission === 'granted' &&
      state.audio.state === 'ok';
  }
  return next;
}

export class Optimistic {
  #server: AppState | null = null;
  #view: AppState | null = null;
  #entries = new Map<OptimisticKey, Entry>();
  #stamp = 0;
  #timer: ReturnType<typeof setTimeout> | null = null;
  readonly #onChange: () => void;
  readonly #now: () => number;

  /** `onChange` runs whenever `view` changes. */
  constructor(onChange: () => void, now: () => number = Date.now) {
    this.#onChange = onChange;
    this.#now = now;
  }

  /** What the UI shows: the app's state with pending values applied (null until loaded). */
  get view(): AppState | null {
    return this.#view;
  }

  /** The app's state as last received. */
  get server(): AppState | null {
    return this.#server;
  }

  /** Counts accepted states; pass it to `reply` to drop replies that were overtaken. */
  get stamp(): number {
    return this.#stamp;
  }

  /** True while `key` shows a value the app has not confirmed yet. */
  pending(key: OptimisticKey): boolean {
    return this.#entries.has(key);
  }

  /** A `state-changed` payload: always the newest truth. */
  event(state: AppState): void {
    this.#accept(state);
  }

  /**
   * A command reply or `get_state` result, requested when `stamp` was `sentAt`. Dropped if
   * any state has arrived since: that one is at least as new.
   */
  reply(state: AppState, sentAt: number): void {
    if (sentAt === this.#stamp) this.#accept(state);
  }

  /** Shows `value` at once (e.g. while a slider moves), before it is sent. */
  show<K extends OptimisticKey>(key: K, value: OptimisticValue<K>): void {
    const entry = this.#entries.get(key);
    if (entry) {
      entry.value = value;
      if (entry.inflight === 0) entry.until = this.#now() + HOLD_MS;
    } else {
      this.#entries.set(key, { value, inflight: 0, until: this.#now() + HOLD_MS });
    }
    this.#update();
  }

  /** A command carrying `value` for `key` was sent. */
  sent<K extends OptimisticKey>(key: K, value: OptimisticValue<K>): void {
    const entry = this.#entries.get(key);
    if (entry) {
      entry.value = value;
      entry.inflight += 1;
    } else {
      this.#entries.set(key, { value, inflight: 1, until: Infinity });
    }
    this.#update();
  }

  /**
   * A command for `key` settled. When nothing else is in flight: on failure the app's value
   * shows again at once; on success the value stays until the app agrees or `GRACE_MS` pass.
   */
  settled(key: OptimisticKey, ok: boolean): void {
    const entry = this.#entries.get(key);
    if (!entry) return;
    entry.inflight = Math.max(0, entry.inflight - 1);
    if (entry.inflight === 0) {
      if (ok) entry.until = this.#now() + GRACE_MS;
      else this.#entries.delete(key);
    }
    this.#update();
  }

  /** Stops the expiry timer (when the window goes away). */
  dispose(): void {
    if (this.#timer !== null) clearTimeout(this.#timer);
    this.#timer = null;
  }

  #accept(state: AppState): void {
    this.#stamp += 1;
    this.#server = state;
    this.#update();
  }

  #update(): void {
    const now = this.#now();
    const server = this.#server;
    let next = Infinity;
    for (const [key, entry] of this.#entries) {
      if (entry.inflight > 0) continue;
      if (now >= entry.until || (server && same(fieldOf(server, key), entry.value))) {
        this.#entries.delete(key);
      } else {
        next = Math.min(next, entry.until);
      }
    }
    this.#schedule(next - now);
    const values = new Map<OptimisticKey, unknown>();
    for (const [key, entry] of this.#entries) values.set(key, entry.value);
    this.#view = server ? withValues(server, values) : null;
    this.#onChange();
  }

  #schedule(delay: number): void {
    if (this.#timer !== null) clearTimeout(this.#timer);
    this.#timer = null;
    if (Number.isFinite(delay)) {
      this.#timer = setTimeout(() => this.#update(), Math.max(0, delay) + 1);
    }
  }
}
