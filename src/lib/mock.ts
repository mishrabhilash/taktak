// In-browser stand-in for the Rust app, used automatically outside Tauri (`npm run dev`).
// It lists the 8 bundled packs from packs/*/pack.json, applies every command to an in-memory
// AppState, emits `state-changed` like the app does, and plays pack previews from the dev
// server. Nothing is persisted.
//
// Scenarios, combinable: ?scenario=denied,fault,invalid,empty
//   denied        Input Monitoring not granted (granted 2 s after "open permission settings")
//   unlisted      like denied, but macOS adds nothing to the Input Monitoring list (as after
//                 `tccutil reset`): "open permission settings" answers false and nothing
//                 changes until the user adds TakTak by hand (granted 6 s after reveal_app)
//   fault         audio device fault that recovers after 4 s
//   invalid       a user pack with a warning, a broken user pack (selected, failed, so
//                 Tactile plays instead) and an invalid one
//   empty         no packs at all (the built-in click plays)
// Milestone 4:
//   firstrun      a fresh install: the onboarding is not done yet (by default the mock is a
//                 returning user)
//   relaunch      macOS reports Input Monitoring as granted but the listener is refused:
//                 relaunchSuggested; the `relaunch` command fixes it after 1.5 s
//   rules         mode "never" listing Slack and zoom.us, with Slack in front (silent)
//   switching     the frontmost app changes every 3 s
//   locked        the screen is locked (auto-muted) and unlocks after 4 s
//   outputchange  "mute when the output changes" is on; 3 s in, the output moves to AirPods Pro
//   unsupported   no per-app rules and no permission step (like Windows and Linux)
// Milestone 5:
//   inputgroup    Linux Wayland without the `input` group: the listener can't read the
//                 keyboard devices (inputGroupNeeded), no per-app rules
//   listenerfail  Windows or Linux whose key listener failed to start (relaunchSuggested without
//                 a permission step); `relaunch` fixes it after 1.5 s
// "Import Mechvibes pack…" answers in turn: an import (the pack is listed 0.7 s later), the
// same pack again (alreadyImported; overwrite_mechvibes_pack replaces it), a pack with no usable
// sounds (rejected), and a cancelled picker (null).
// Pick the view with ?window=tray, ?window=onboarding or ?window=settings (default).
// open_onboarding ("Show Welcome Guide" in Settings → About and General) opens the onboarding
// view in another tab, with the same scenarios (see `onboardingUrl`).

import { version } from '../../package.json';
import type { Backend } from './api';
import { ruleAppProblem, ruleBlocks, withApp, withoutApp } from './rules';
import type {
  AppInfo,
  AppRef,
  AppState,
  Command,
  CommandArgs,
  CommandResult,
  Commands,
  ImportSummary,
  LatencyReport,
  MechvibesImport,
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
  packId: 'tactile',
  masterVolume: 0.7,
  pressVolume: 1,
  releaseVolume: 1,
  variantMode: 'consistent',
  humanize: 0.25,
  muteHotkey: 'CommandOrControl+Alt+Shift+M',
  launchAtLogin: false,
  appRule: { mode: 'everywhere', apps: [] },
  muteOnOutputChange: false,
  onboardingDone: false,
};

/** The most ids one `get_app_icons` call answers. */
const MAX_ICON_IDS = 200;

const UNSUPPORTED = 'Per-app rules aren’t available on this system yet.';

interface FakeApp extends AppRef {
  /** Icon background; null = the app has no icon. */
  color: string | null;
}

/** What list_running_apps reports (already sorted by name, like the app). */
const RUNNING: FakeApp[] = [
  { id: 'com.apple.finder', name: 'Finder', color: '#1e88e5' },
  { id: 'com.google.Chrome', name: 'Google Chrome', color: '#34a853' },
  { id: 'net.kovidgoyal.kitty', name: 'kitty', color: null },
  { id: 'com.apple.Notes', name: 'Notes', color: '#f9c600' },
  { id: 'com.apple.Safari', name: 'Safari', color: '#0a84ff' },
  { id: 'com.tinyspeck.slackmacgap', name: 'Slack', color: '#4a154b' },
  { id: 'com.apple.Terminal', name: 'Terminal', color: '#2b2b2b' },
  { id: 'com.microsoft.VSCode', name: 'Visual Studio Code', color: '#007acc' },
  { id: 'com.apple.dt.Xcode', name: 'Xcode', color: '#147efb' },
  { id: 'us.zoom.xos', name: 'zoom.us', color: '#2d8cff' },
];

/** What choose_app returns, in turn (null = the user cancelled). */
const CHOOSABLE: (FakeApp | null)[] = [
  { id: 'com.microsoft.Word', name: 'Microsoft Word', color: '#2b579a' },
  { id: 'com.hnc.Discord', name: 'Discord', color: '#5865f2' },
  { id: 'md.obsidian', name: 'Obsidian', color: '#7c3aed' },
  null,
];

/** The apps that take turns in front with ?scenario=switching. */
const SWITCHING = [
  'com.microsoft.VSCode',
  'com.tinyspeck.slackmacgap',
  'com.apple.Safari',
  'com.apple.Terminal',
  'us.zoom.xos',
];

const KNOWN_APPS = [...RUNNING, ...CHOOSABLE.filter((a): a is FakeApp => a !== null)];

/** What the mock's Mechvibes import writes. */
const IMPORTED: ImportSummary = {
  id: 'mv-cream-linear',
  name: 'Cream Linear',
  source: 'cream-linear.zip',
  format: 'Mechvibes v2',
  keysMapped: 84,
  keysWithRelease: 12,
  soundsWritten: 96,
  replaced: false,
  warnings: ['Skipped key “3640”: unknown key code', 'Missing file: sounds/F13.wav'],
};

/** How the mock lists an imported pack (what the app's registry reports for it). */
const IMPORTED_PACK: PackSummary = {
  id: IMPORTED.id,
  name: IMPORTED.name,
  author: 'Unknown (imported from Mechvibes)',
  license: 'LicenseRef-Personal',
  description: 'Imported from the Mechvibes pack "Cream Linear" for personal use; not for redistribution.',
  attribution: null,
  origin: 'user',
  hasRelease: true,
  perKey: true,
  warnings: [],
};

/** The importer's rejection for a pack without usable sounds (mechvibes.rs). */
const NO_SOUNDS =
  'No usable sounds in this pack: every sound file it names is missing. Check that the folder or .zip still has the sound files its config.json names, in a format TakTak reads (WAV, Ogg Vorbis or MP3).';

function fakeApp(id: string): FakeApp {
  const app = KNOWN_APPS.find((a) => a.id === id);
  if (!app) throw new Error(`no fake app ${id}`);
  return app;
}

const ref = ({ id, name }: AppRef): AppRef => ({ id, name });

/** A 32 × 32 rounded tile with the app's initial, as a data: URL (the app sends PNGs). */
function fakeIcon(app: FakeApp): string | null {
  if (app.color === null) return null;
  const svg =
    '<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32" viewBox="0 0 32 32">' +
    `<rect x="2" y="2" width="28" height="28" rx="7" fill="${app.color}"/>` +
    '<text x="16" y="21.5" font-family="-apple-system, Helvetica, sans-serif" font-size="15" ' +
    `font-weight="600" text-anchor="middle" fill="#fff">${app.name.charAt(0).toUpperCase()}</text>` +
    '</svg>';
  return `data:image/svg+xml,${encodeURIComponent(svg)}`;
}

function appInfo(app: FakeApp): AppInfo {
  return { id: app.id, name: app.name, iconDataUrl: fakeIcon(app) };
}

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
    settings: structuredClone(DEFAULT_SETTINGS),
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
    frontmostApp: ref(fakeApp('com.microsoft.VSCode')),
    ruleBlocked: false,
    autoMute: null,
    rulesSupported: true,
    onboarding: {
      offer: false,
      permissionRequired: true,
      relaunchSuggested: false,
      inputGroupNeeded: false,
    },
  };
  // A returning user, unless ?scenario=firstrun.
  state.settings.onboardingDone = !scenarios.has('firstrun');
  if (scenarios.has('denied') || scenarios.has('unlisted')) state.permission = 'denied';
  if (scenarios.has('relaunch')) {
    state.permission = 'denied';
    state.onboarding.relaunchSuggested = true;
  }
  if (scenarios.has('rules')) {
    state.settings.appRule = {
      mode: 'never',
      apps: [ref(fakeApp('com.tinyspeck.slackmacgap')), ref(fakeApp('us.zoom.xos'))],
    };
    state.frontmostApp = ref(fakeApp('com.tinyspeck.slackmacgap'));
  }
  if (scenarios.has('outputchange')) state.settings.muteOnOutputChange = true;
  if (scenarios.has('unsupported')) {
    state.rulesSupported = false;
    state.frontmostApp = null;
    state.onboarding.permissionRequired = false;
  }
  if (scenarios.has('inputgroup') || scenarios.has('listenerfail')) {
    state.rulesSupported = false;
    state.frontmostApp = null;
    state.onboarding.permissionRequired = false;
    state.permission = 'denied';
    state.audio.device = 'Built-in Audio Analog Stereo';
    state.userPacksDir = '/home/you/.local/share/tech.taktak.app/packs';
    if (scenarios.has('inputgroup')) state.onboarding.inputGroupNeeded = true;
    else state.onboarding.relaunchSuggested = true;
  }
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
 * What the app's loader ends up with for the selected pack: the pack itself, else Tactile
 * (the default), else any pack, else the built-in click, with `activePackError` saying why (catalog.rs).
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

/** The page the mock's open_onboarding opens: this page's query with `window=onboarding`. */
export function onboardingUrl(search: string): string {
  const params = new URLSearchParams(search);
  params.set('window', 'onboarding');
  return `?${params.toString()}`;
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
  /** The two auto-mute reasons, tracked apart like the app does; `autoMute` shows the stronger. */
  const autoMute = { screenLocked: scenarios.has('locked'), outputChanged: false };
  let chosen = 0;
  /** Which import answer comes next (see the header comment). */
  let imports = 0;
  /** An "alreadyImported" answer waits for overwrite_mechvibes_pack. */
  let overwritePending = false;

  /** The app writes the pack; its registry lists it about 0.7 s later. */
  function imported(replaced: boolean): MechvibesImport {
    void wait(700).then(() =>
      change((s) => {
        s.packs = [...s.packs.filter((p) => p.id !== IMPORTED_PACK.id), IMPORTED_PACK].sort(byName);
      }),
    );
    return { outcome: 'imported', pack: { ...IMPORTED, replaced } };
  }

  /** The derived fields, as the app computes them (docs/ui-contract.md). */
  function snapshot(): AppState {
    state.autoMute = autoMute.screenLocked
      ? 'screenLocked'
      : autoMute.outputChanged
        ? 'outputChanged'
        : null;
    state.ruleBlocked = ruleBlocks(state.settings.appRule, state.frontmostApp, state.rulesSupported);
    state.onboarding.offer =
      !state.settings.onboardingDone ||
      (state.onboarding.permissionRequired && state.permission === 'denied');
    state.playing =
      state.settings.enabled &&
      !state.muted &&
      state.autoMute === null &&
      !state.ruleBlocked &&
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

  if (autoMute.screenLocked) {
    void wait(4000).then(() => {
      autoMute.screenLocked = false;
      emit();
    });
  }

  if (scenarios.has('outputchange')) {
    void wait(3000).then(() => {
      state.audio = {
        device: 'AirPods Pro',
        sampleRate: 48000,
        bufferFrames: 64,
        state: 'ok',
        message: null,
      };
      // Only while sounds are on and not muted by hand, like the app.
      if (state.settings.muteOnOutputChange && state.settings.enabled && !state.muted) {
        autoMute.outputChanged = true;
      }
      emit();
    });
  }

  if (scenarios.has('switching') && state.rulesSupported) {
    let turn = 0;
    const next = (): void => {
      void wait(3000).then(() => {
        turn = (turn + 1) % SWITCHING.length;
        state.frontmostApp = ref(fakeApp(SWITCHING[turn] ?? 'com.microsoft.VSCode'));
        emit();
        next();
      });
    };
    next();
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
    set_enabled: ({ enabled }) =>
      change((s) => {
        s.settings.enabled = enabled;
        if (enabled) autoMute.outputChanged = false;
      }),
    set_muted: ({ muted }) =>
      change((s) => {
        s.muted = muted;
        autoMute.outputChanged = false;
      }),
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
      if (!state.onboarding.permissionRequired) return true;
      if (scenarios.has('unlisted') && state.permission !== 'granted') return false;
      // With ?scenario=relaunch the listener stays refused until `relaunch`.
      if (state.permission !== 'granted' && !state.onboarding.relaunchSuggested) {
        void wait(2000).then(() => change((s) => (s.permission = 'granted')));
      }
      return true;
    },
    get_latency: () => latency(),
    quit: () => console.info('[mock] quit'),
    list_running_apps: () => (state.rulesSupported ? RUNNING.map(appInfo) : []),
    choose_app: () => {
      if (!state.rulesSupported) throw UNSUPPORTED;
      const app = CHOOSABLE[chosen % CHOOSABLE.length] ?? null;
      chosen += 1;
      return app ? appInfo(app) : null;
    },
    get_app_icons: ({ ids }) =>
      Object.fromEntries(
        ids.slice(0, MAX_ICON_IDS).map((id) => {
          const app = state.rulesSupported ? KNOWN_APPS.find((a) => a.id === id) : undefined;
          return [id, app ? fakeIcon(app) : null];
        }),
      ),
    set_app_rule_mode: ({ mode }) => change((s) => (s.settings.appRule.mode = mode)),
    add_rule_app: ({ id, name }) => {
      // The app's checks and storage rules (rules.ts mirrors them for the UI).
      const rule = state.settings.appRule;
      const problem = ruleAppProblem(rule, id);
      if (problem) throw problem;
      const next = withApp(rule, { id, name });
      if (next === rule) return snapshot();
      return change((s) => (s.settings.appRule = next));
    },
    remove_rule_app: ({ id }) => {
      const rule = state.settings.appRule;
      const next = withoutApp(rule, id);
      if (next === rule) return snapshot();
      return change((s) => (s.settings.appRule = next));
    },
    set_mute_on_output_change: ({ enabled }) =>
      change((s) => {
        s.settings.muteOnOutputChange = enabled;
        if (!enabled) autoMute.outputChanged = false;
      }),
    open_onboarding: () => {
      const url = onboardingUrl(search);
      console.info(`[mock] open the onboarding window (${url})`);
      // Like the app, one onboarding window: a second call focuses the same tab.
      if (typeof window !== 'undefined') window.open(url, 'taktak-onboarding');
    },
    finish_onboarding: () => {
      console.info('[mock] close the onboarding window');
      return change((s) => (s.settings.onboardingDone = true));
    },
    import_mechvibes_pack: ({ kind }) => {
      console.info(`[mock] pick a Mechvibes pack (${kind})`);
      const turn = imports % 4;
      imports += 1;
      overwritePending = false;
      if (turn === 0) return imported(false);
      if (turn === 1) {
        overwritePending = true;
        return { outcome: 'alreadyImported', id: IMPORTED.id, source: IMPORTED.source };
      }
      if (turn === 2) throw NO_SOUNDS;
      return null;
    },
    overwrite_mechvibes_pack: () => {
      if (!overwritePending) {
        throw 'There is no import waiting to be replaced. Choose the pack again with Import Mechvibes pack…';
      }
      overwritePending = false;
      return imported(true);
    },
    reveal_app: () => {
      console.info('[mock] reveal TakTak.app in Finder');
      // As if the user dragged it into the list and switched it on.
      if (scenarios.has('unlisted') && state.permission !== 'granted') {
        void wait(6000).then(() => change((s) => (s.permission = 'granted')));
      }
    },
    relaunch: () => {
      console.info('[mock] relaunch');
      if (state.onboarding.relaunchSuggested) {
        // As if the new instance were up: it can listen now.
        void wait(1500).then(() =>
          change((s) => {
            s.permission = 'granted';
            s.onboarding.relaunchSuggested = false;
          }),
        );
      }
    },
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
