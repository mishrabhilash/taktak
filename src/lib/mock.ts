// In-browser stand-in for the Rust app, used automatically outside Tauri (`npm run dev`).
// It lists the 12 bundled packs from packs/*/pack.json, applies every command to an in-memory
// AppState, emits `state-changed` like the app does, and plays pack previews from the dev
// server. Nothing is persisted.
//
// Scenarios, combinable: ?scenario=denied,fault,invalid,empty
//   denied   Input Monitoring not granted (granted 2 s after "open permission settings")
//   fault    audio device fault that recovers after 4 s
//   invalid  a user pack with a warning, a broken user pack (selected, failed, so Deep Thock
//            plays instead) and an invalid one
//   empty    no packs at all (the built-in click plays)
// Pick the view with ?window=tray or ?window=settings (default).

import { version } from '../../package.json';
import type { Backend } from './api';
import type {
  AppState,
  Command,
  CommandArgs,
  CommandResult,
  Commands,
  LatencyReport,
  PackSummary,
  Settings,
} from './types';

interface SoundSet {
  press?: string[];
  release?: string[];
}

interface Manifest {
  id: string;
  name: string;
  author: string;
  license: string;
  description?: string;
  attribution?: string;
  groups?: Record<string, SoundSet>;
  keys?: Record<string, SoundSet>;
}

const manifests = import.meta.glob<Manifest>('../../packs/*/pack.json', {
  eager: true,
  import: 'default',
});

const DEFAULT_SETTINGS: Settings = {
  enabled: true,
  packId: 'deep-thock',
  masterVolume: 0.7,
  pressVolume: 1,
  releaseVolume: 1,
  variantMode: 'consistent',
  humanize: 0.25,
  muteHotkey: 'CommandOrControl+Alt+Shift+M',
  launchAtLogin: false,
};

const MODIFIERS = new Set([
  'commandorcontrol',
  'cmdorctrl',
  'command',
  'cmd',
  'control',
  'ctrl',
  'alt',
  'option',
  'shift',
  'super',
  'meta',
]);

/** F1–F24, which work as a hotkey on their own (lower case). */
const FUNCTION_KEY = /^f([1-9]|1\d|2[0-4])$/;

/** Shortcuts the mock pretends the system already owns. */
const TAKEN: Record<string, string> = {
  'commandorcontrol+space': 'Spotlight',
  'commandorcontrol+tab': 'the app switcher',
  'commandorcontrol+q': 'Quit',
};

function summary(m: Manifest): PackSummary {
  const sets = [...Object.values(m.groups ?? {}), ...Object.values(m.keys ?? {})];
  return {
    id: m.id,
    name: m.name,
    author: m.author,
    license: m.license,
    description: m.description ?? null,
    attribution: m.attribution ?? null,
    origin: 'bundled',
    hasRelease: sets.some((s) => (s.release?.length ?? 0) > 0),
    perKey: Object.keys(m.keys ?? {}).length > 0,
    warnings: [],
  };
}

const byName = (a: PackSummary, b: PackSummary) => a.name.localeCompare(b.name);
const unit = (v: number) => (Number.isFinite(v) ? Math.min(1, Math.max(0, v)) : 0);
const wait = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

function initialState(scenarios: Set<string>): AppState {
  const state: AppState = {
    version,
    settings: { ...DEFAULT_SETTINGS },
    muted: false,
    playing: false,
    packs: Object.values(manifests).map(summary).sort(byName),
    invalidPacks: [],
    playingPackId: DEFAULT_SETTINGS.packId,
    activePackError: null,
    muteHotkeyError: null,
    userPacksDir: '/Users/you/Library/Application Support/tech.taktak.app/packs',
    permission: 'granted',
    audio: {
      device: 'MacBook Pro Speakers',
      sampleRate: 48000,
      bufferFrames: 64,
      state: 'ok',
      message: null,
    },
  };
  if (scenarios.has('denied')) state.permission = 'denied';
  if (scenarios.has('fault')) {
    state.audio = {
      device: null,
      sampleRate: null,
      bufferFrames: null,
      state: 'fault',
      message: 'Output device disconnected — reconnecting…',
    };
  }
  if (scenarios.has('invalid')) {
    state.packs.push(
      {
        id: 'my-board',
        name: 'My Board',
        author: 'You',
        license: 'CC0-1.0',
        description: 'Recorded with pack-maker on a 65% board.',
        attribution: null,
        origin: 'user',
        hasRelease: true,
        perKey: false,
        warnings: ['sounds/space-press.wav: longer than 1 s; only the first 2 s are used'],
      },
      {
        id: 'half-done',
        name: 'Half Done',
        author: 'You',
        license: 'CC-BY-4.0',
        description: null,
        attribution: 'Recorded by you',
        origin: 'user',
        hasRelease: false,
        perKey: false,
        warnings: [],
      },
    );
    state.packs.sort(byName);
    state.settings.packId = 'half-done';
    loadSelected(state);
    state.invalidPacks = [
      {
        location: '/Users/you/Library/Application Support/tech.taktak.app/packs/broken.zip',
        problems: [
          'error: pack.json: missing field `license`',
          'error: sounds/a.wav: file not found',
        ],
      },
    ];
  }
  if (scenarios.has('empty')) {
    state.packs = [];
    loadSelected(state);
  }
  return state;
}

/** The pack the mock pretends cannot be decoded (scenario "invalid"). */
const BROKEN = 'half-done';

/**
 * What the app's loader ends up with for the selected pack: the pack itself, else Deep Thock,
 * else any pack, else the built-in click, with `activePackError` saying why (catalog.rs).
 */
function loadSelected(s: AppState): void {
  const selected = s.settings.packId;
  const loadable = s.packs.filter((p) => p.id !== BROKEN);
  const playing =
    loadable.find((p) => p.id === selected) ??
    loadable.find((p) => p.id === DEFAULT_SETTINGS.packId) ??
    loadable[0] ??
    null;
  s.playingPackId = playing?.id ?? null;
  const instead = playing?.name ?? 'the built-in click';
  const failed = s.packs.find((p) => p.id === selected);
  if (playing?.id === selected) s.activePackError = null;
  else if (failed) {
    s.activePackError = `${failed.name} could not be loaded: sounds/press-03.wav: unsupported format. Playing ${instead} instead.`;
  } else if (s.packs.length === 0) {
    s.activePackError = `No sound packs found. Playing ${instead} instead.`;
  } else {
    s.activePackError = `The pack “${selected}” is not installed. Playing ${instead} instead.`;
  }
}

/**
 * A mock backend for one page; see the header comment for the scenarios. `search` is the page's
 * query string (tests pass their own).
 */
export function createMockBackend(
  search: string = typeof window === 'undefined' ? '' : window.location.search,
): Backend {
  const scenarios = new Set(
    (new URLSearchParams(search).get('scenario') ?? '')
      .split(',')
      .map((s) => s.trim())
      .filter(Boolean),
  );
  const state = initialState(scenarios);
  const listeners = new Set<(state: AppState) => void>();
  let audio: HTMLAudioElement | null = null;
  const openedAt = performance.now();

  function snapshot(): AppState {
    state.playing =
      state.settings.enabled &&
      !state.muted &&
      state.permission === 'granted' &&
      state.audio.state === 'ok';
    return structuredClone(state);
  }

  function emit(): AppState {
    const snap = snapshot();
    // Like Tauri events: delivered after the command resolves.
    setTimeout(() => listeners.forEach((l) => l(structuredClone(snap))));
    return snap;
  }

  function change(apply: (s: AppState) => void): AppState {
    apply(state);
    return emit();
  }

  if (state.audio.state === 'fault') {
    void wait(4000).then(() => {
      state.audio = {
        device: 'External Headphones',
        sampleRate: 44100,
        bufferFrames: 64,
        state: 'ok',
        message: null,
      };
      emit();
    });
  }

  function latency(): LatencyReport | null {
    // Pretend the user types about three keys a second while the window is open.
    const count = Math.floor((performance.now() - openedAt) / 333);
    if (count < 5) return null;
    const jitter = Math.sin(count) * 0.3;
    return {
      count,
      totalP50Ms: 5.9 + jitter,
      totalP95Ms: 6.7 + jitter,
      totalMaxMs: 10.0 + 2 * jitter,
      inputP50Ms: 0.25,
      queueP50Ms: 0.8 + jitter / 2,
      outputMs: 4.6,
    };
  }

  /** The rules of hotkey.rs: one key; F1–F24 alone, anything else with Ctrl, Alt or Cmd/Win. */
  function validateHotkey(accelerator: string): void {
    const parts = accelerator.split('+').map((p) => p.trim().toLowerCase());
    const mods = parts.filter((p) => MODIFIERS.has(p));
    const keys = parts.filter((p) => p && !MODIFIERS.has(p));
    if (keys.length !== 1) throw 'A shortcut needs modifiers and exactly one other key.';
    if (!FUNCTION_KEY.test(keys[0] ?? '') && mods.every((m) => m === 'shift')) {
      throw 'Add ⌘ Command, ⌥ Option or ⌃ Control to the hotkey, so it never takes over a key you type.';
    }
    const owner = TAKEN[parts.join('+')];
    if (owner) throw `That shortcut is already used by ${owner}.`;
  }

  const handlers: { [C in Command]: (args: Commands[C]['args']) => Commands[C]['result'] } = {
    get_state: () => snapshot(),
    set_enabled: ({ enabled }) => change((s) => (s.settings.enabled = enabled)),
    set_muted: ({ muted }) => change((s) => (s.muted = muted)),
    set_pack: ({ id }) => {
      if (!state.packs.some((p) => p.id === id)) throw `There is no pack “${id}”.`;
      const result = change((s) => {
        s.settings.packId = id;
        s.activePackError = null;
      });
      // The real app loads off the main thread and emits again when the bank is swapped in.
      void wait(150).then(() => change(loadSelected));
      return result;
    },
    set_master_volume: ({ value }) => change((s) => (s.settings.masterVolume = unit(value))),
    set_press_volume: ({ value }) => change((s) => (s.settings.pressVolume = unit(value))),
    set_release_volume: ({ value }) => change((s) => (s.settings.releaseVolume = unit(value))),
    set_variant_mode: ({ mode }) => change((s) => (s.settings.variantMode = mode)),
    set_humanize: ({ value }) => change((s) => (s.settings.humanize = unit(value))),
    set_mute_hotkey: ({ accelerator }) => {
      const wanted = accelerator?.trim() || null;
      if (wanted !== null) validateHotkey(wanted);
      return change((s) => (s.settings.muteHotkey = wanted));
    },
    set_launch_at_login: ({ enabled }) => change((s) => (s.settings.launchAtLogin = enabled)),
    preview_pack: ({ id }) => {
      audio?.pause();
      audio = new Audio(`/packs/${encodeURIComponent(id)}/preview.wav`);
      audio.volume = state.settings.masterVolume ** 2;
      audio.play().catch(() => console.info(`[mock] no preview clip for ${id}`));
    },
    stop_preview: () => {
      audio?.pause();
      audio = null;
    },
    open_settings: () => console.info('[mock] open_settings'),
    hide_tray: () => console.info('[mock] hide_tray'),
    open_user_packs_dir: () => console.info(`[mock] reveal ${state.userPacksDir}`),
    open_permission_settings: () => {
      console.info('[mock] open Privacy & Security → Input Monitoring');
      if (state.permission !== 'granted') {
        void wait(2000).then(() => change((s) => (s.permission = 'granted')));
      }
    },
    get_latency: () => latency(),
    quit: () => console.info('[mock] quit'),
  };

  return {
    async call<C extends Command>(command: C, args: CommandArgs<C>): Promise<CommandResult<C>> {
      await wait(10);
      const handler = handlers[command] as (args: CommandArgs<C>) => CommandResult<C>;
      return handler(args);
    },
    async onStateChanged(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
}
