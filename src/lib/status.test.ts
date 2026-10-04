import { describe, expect, it } from 'vitest';
import { detectPlatform, permissionName } from './platform';
import {
  BUILT_IN_SOUND,
  actionLabel,
  effectiveMuted,
  grantLabel,
  notices,
  playbackStatus,
  playingName,
  silentIn,
} from './status';
import type { AppState, PackSummary } from './types';

function state(top: Partial<AppState> = {}, enabled = true): AppState {
  return {
    version: '0.1.0',
    settings: {
      enabled,
      packId: 'buckling-spring',
      masterVolume: 0.7,
      pressVolume: 1,
      releaseVolume: 1,
      variantMode: 'consistent',
      humanize: 0.25,
      muteHotkey: null,
      launchAtLogin: false,
      appRule: { mode: 'everywhere', apps: [] },
      muteOnOutputChange: false,
      onboardingDone: true,
    },
    muted: false,
    playing: true,
    packs: [],
    invalidPacks: [],
    playingPackId: 'buckling-spring',
    activePackError: null,
    muteHotkeyError: null,
    userPacksDir: null,
    permission: 'granted',
    audio: { device: 'Speakers', sampleRate: 48000, bufferFrames: 64, state: 'ok', message: null },
    frontmostApp: { id: 'com.apple.Safari', name: 'Safari' },
    ruleBlocked: false,
    autoMute: null,
    rulesSupported: true,
    onboarding: {
      offer: false,
      permissionRequired: true,
      relaunchSuggested: false,
      inputGroupNeeded: false,
    },
    ...top,
  };
}

const FAULT = {
  device: null,
  sampleRate: null,
  bufferFrames: null,
  state: 'fault' as const,
  message: 'Output device disconnected — reconnecting…',
};

describe('detectPlatform', () => {
  it('prefers Tauri’s platform, then the user agent', () => {
    expect(detectPlatform('darwin', 'Windows NT')).toBe('mac');
    expect(detectPlatform('windows')).toBe('windows');
    expect(detectPlatform('linux')).toBe('linux');
    expect(detectPlatform(undefined, 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)')).toBe('mac');
    expect(detectPlatform(undefined, 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)')).toBe('windows');
    expect(detectPlatform(undefined, 'Mozilla/5.0 (X11; Linux x86_64)')).toBe('linux');
    expect(detectPlatform('', '')).toBe('linux');
  });

  it('names the permission per platform', () => {
    expect(permissionName('mac')).toBe('Input Monitoring');
    expect(grantLabel('mac')).toBe('Grant Input Monitoring');
    expect(grantLabel('windows')).not.toContain('Input Monitoring');
  });
});

describe('playbackStatus', () => {
  it('explains why nothing plays, most basic reason first', () => {
    expect(playbackStatus(state(), 'mac')).toEqual({ label: 'Playing', tone: 'ok' });
    expect(playbackStatus(state({ playing: false }, false), 'mac').label).toBe('Sounds off');
    expect(playbackStatus(state({ playing: false, muted: true }), 'mac').label).toBe('Muted');
    expect(playbackStatus(state({ playing: false, permission: 'denied' }), 'mac')).toEqual({
      label: 'Needs Input Monitoring',
      tone: 'warning',
    });
    expect(playbackStatus(state({ playing: false, audio: FAULT }), 'mac').tone).toBe('error');
  });
});

describe('notices', () => {
  it('reports a mute shortcut that could not be registered at startup', () => {
    const list = notices(state({ muteHotkeyError: 'The shortcut ⌘⌥⇧M is already in use.' }), 'mac');
    expect(list.map((n) => n.id)).toEqual(['hotkey']);
    expect(list[0]?.message).toContain('already in use');
    expect(list[0]?.actions).toEqual([]);
  });

  it('is empty when all is well', () => {
    expect(notices(state(), 'mac')).toEqual([]);
  });

  it('offers the permission button when Input Monitoring is missing', () => {
    const [n] = notices(state({ permission: 'denied', playing: false }), 'mac');
    expect(n?.id).toBe('permission');
    expect(n?.actions).toEqual(['permission', 'guide']);
    expect(n?.title).toContain('Input Monitoring');
  });

  it('shows the audio fault message and the active pack error', () => {
    const list = notices(
      state({ playing: false, audio: FAULT, activePackError: 'Half Done could not be loaded' }),
      'mac',
    );
    expect(list.map((n) => n.id)).toEqual(['audio', 'pack']);
    expect(list[0]?.message).toBe(FAULT.message);
    expect(list[1]?.message).toBe('Half Done could not be loaded');
  });

  it('orders permission, audio, pack', () => {
    const list = notices(
      state({ permission: 'unknown', audio: FAULT, activePackError: 'x', playing: false }),
      'windows',
    );
    expect(list.map((n) => n.id)).toEqual(['permission', 'audio', 'pack']);
  });
});

describe('effectiveMuted and silentIn', () => {
  it('the mute switch shows the manual mute or an outputChanged auto-mute, not a lock', () => {
    expect(effectiveMuted(state())).toBe(false);
    expect(effectiveMuted(state({ muted: true }))).toBe(true);
    expect(effectiveMuted(state({ autoMute: 'outputChanged' }))).toBe(true);
    expect(effectiveMuted(state({ autoMute: 'screenLocked' }))).toBe(false);
  });

  it('names the app in front, or "this app"', () => {
    expect(silentIn(state())).toBe('Silent in Safari');
    expect(silentIn(state({ frontmostApp: null }))).toBe('Silent in this app');
  });
});

describe('playbackStatus (Milestone 4)', () => {
  const silent = { playing: false } as const;

  it('explains auto-mute and rules with the tray’s texts', () => {
    expect(playbackStatus(state({ ...silent, autoMute: 'screenLocked' }), 'mac')).toEqual({
      label: 'Muted — screen locked',
      tone: 'off',
    });
    expect(playbackStatus(state({ ...silent, autoMute: 'outputChanged' }), 'mac').label).toBe(
      'Muted — output device changed',
    );
    expect(playbackStatus(state({ ...silent, ruleBlocked: true }), 'mac').label).toBe(
      'Silent in Safari',
    );
    expect(
      playbackStatus(state({ ...silent, ruleBlocked: true, frontmostApp: null }), 'mac').label,
    ).toBe('Silent in this app');
  });

  it('follows the contract’s order: off, muted, locked, output, permission, fault, rules, starting', () => {
    const all: Partial<AppState> = {
      ...silent,
      muted: true,
      autoMute: 'screenLocked',
      permission: 'denied',
      audio: FAULT,
      ruleBlocked: true,
    };
    expect(playbackStatus(state(all, false), 'mac').label).toBe('Sounds off');
    expect(playbackStatus(state(all), 'mac').label).toBe('Muted');
    expect(playbackStatus(state({ ...all, muted: false }), 'mac').label).toBe(
      'Muted — screen locked',
    );
    expect(
      playbackStatus(state({ ...all, muted: false, autoMute: 'outputChanged' }), 'mac').label,
    ).toBe('Muted — output device changed');
    expect(playbackStatus(state({ ...all, muted: false, autoMute: null }), 'mac').label).toBe(
      'Needs Input Monitoring',
    );
    expect(
      playbackStatus(state({ ...all, muted: false, autoMute: null, permission: 'granted' }), 'mac')
        .label,
    ).toBe('No sound output');
    const starting = { ...FAULT, state: 'starting' as const, message: null };
    expect(
      playbackStatus(
        state({ ...all, muted: false, autoMute: null, permission: 'granted', audio: starting }),
        'mac',
      ).label,
    ).toBe('Silent in Safari');
    expect(
      playbackStatus(
        state({
          ...all,
          muted: false,
          autoMute: null,
          permission: 'granted',
          audio: starting,
          ruleBlocked: false,
        }),
        'mac',
      ).label,
    ).toBe('Starting audio…');
  });
});

describe('notices (Milestone 4)', () => {
  it('an output change offers Unmute and names the new device', () => {
    const s = state({
      playing: false,
      autoMute: 'outputChanged',
      audio: { device: 'AirPods Pro', sampleRate: 48000, bufferFrames: 64, state: 'ok', message: null },
    });
    const [n] = notices(s, 'mac');
    expect(n?.id).toBe('automute');
    expect(n?.actions).toEqual(['unmute']);
    expect(n?.message).toBe(
      'TakTak muted itself because the sound output changed to AirPods Pro. Unmute to keep typing sounds on this device.',
    );
    expect(n?.line).toContain('AirPods Pro');
    const nameless = notices(state({ autoMute: 'outputChanged', audio: { ...FAULT, state: 'ok' } }), 'mac');
    expect(nameless[0]?.message).not.toContain('null');
  });

  it('a screen lock is explained without a button (only unlocking clears it)', () => {
    const [n] = notices(state({ playing: false, autoMute: 'screenLocked' }), 'mac');
    expect(n?.id).toBe('automute');
    expect(n?.title).toBe('Muted — screen locked');
    expect(n?.actions).toEqual([]);
  });

  it('auto-mute and rules say nothing while sounds are off or muted by hand', () => {
    for (const top of [{ muted: true }, {}]) {
      const enabled = 'muted' in top;
      for (const extra of [{ autoMute: 'screenLocked' as const }, { ruleBlocked: true }]) {
        expect(notices(state({ playing: false, ...top, ...extra }, enabled), 'mac')).toEqual([]);
      }
    }
  });

  it('rules: an info notice with "Edit rules…", only when the rules are the reason', () => {
    const [n] = notices(state({ playing: false, ruleBlocked: true }), 'mac');
    expect(n).toMatchObject({ id: 'rules', tone: 'info', title: 'Silent in Safari', actions: ['rules'] });
    expect(n?.line).toBe('Silent in Safari because of your per-app rules.');
    expect(actionLabel('rules', 'mac')).toBe('Edit rules…');
    // Permission missing or the output gone: those notices explain it instead.
    const denied = notices(state({ playing: false, ruleBlocked: true, permission: 'denied' }), 'mac');
    expect(denied.map((x) => x.id)).toEqual(['permission']);
    const fault = notices(state({ playing: false, ruleBlocked: true, audio: FAULT }), 'mac');
    expect(fault.map((x) => x.id)).toEqual(['audio']);
    const locked = notices(state({ playing: false, ruleBlocked: true, autoMute: 'screenLocked' }), 'mac');
    expect(locked.map((x) => x.id)).toEqual(['automute']);
  });

  it('a refused listener leads with Quit & Reopen', () => {
    const [n] = notices(
      state({
        playing: false,
        permission: 'denied',
        onboarding: {
          offer: true,
          permissionRequired: true,
          relaunchSuggested: true,
          inputGroupNeeded: false,
        },
      }),
      'mac',
    );
    expect(n?.actions).toEqual(['relaunch', 'guide']);
    expect(n?.message).toContain('macOS needs TakTak to restart before it can listen.');
    expect(actionLabel('relaunch', 'mac')).toBe('Quit & Reopen');
  });

  it('says key sounds are unavailable where there is no listener and nothing to grant', () => {
    const s = state({
      playing: false,
      permission: 'unknown',
      onboarding: {
        offer: false,
        permissionRequired: false,
        relaunchSuggested: false,
        inputGroupNeeded: false,
      },
    });
    const [n] = notices(s, 'linux');
    expect(n?.id).toBe('permission');
    expect(n?.tone).toBe('info');
    expect(n?.actions).toEqual([]);
    expect(n?.title).toBe('Key sounds are turned off');
    expect(n?.message).toContain('without its key listener');
    expect(notices(s, 'mac')[0]?.title).toBe('Key sounds are turned off');
    expect(playbackStatus(s, 'windows')).toEqual({ label: 'Key sounds unavailable', tone: 'off' });
    // Still waiting where a permission would bring the listener.
    const waiting = state({ playing: false, permission: 'unknown' });
    expect(playbackStatus(waiting, 'mac').label).toBe('Waiting for Input Monitoring');
    // Muted by hand still comes first.
    expect(playbackStatus({ ...s, muted: true }, 'linux').label).toBe('Muted');
  });

  it('explains the Linux input group, with the command and its cost (M5)', () => {
    const s = state({
      playing: false,
      permission: 'denied',
      onboarding: {
        offer: false,
        permissionRequired: false,
        relaunchSuggested: false,
        inputGroupNeeded: true,
      },
    });
    expect(playbackStatus(s, 'linux')).toEqual({ label: 'Needs keyboard access', tone: 'warning' });
    const [n] = notices(s, 'linux');
    expect(n).toMatchObject({ id: 'permission', tone: 'warning', actions: ['guide'] });
    expect(n?.message).toContain('sudo usermod -aG input $USER');
    expect(n?.message).toContain('log out');
    expect(n?.message).toContain('every program you run read every keystroke');
    expect(n?.message).not.toContain('Input Monitoring');
  });

  it('a failed listener on Windows or Linux offers Quit & Reopen without macOS words (M5)', () => {
    const s = state({
      playing: false,
      permission: 'denied',
      onboarding: {
        offer: false,
        permissionRequired: false,
        relaunchSuggested: true,
        inputGroupNeeded: false,
      },
    });
    for (const p of ['windows', 'linux'] as const) {
      expect(playbackStatus(s, p)).toEqual({ label: 'Key listener stopped', tone: 'warning' });
      const [n] = notices(s, p);
      expect(n).toMatchObject({ title: 'TakTak needs to restart', actions: ['relaunch'] });
      expect(n?.message).not.toMatch(/macOS|Input Monitoring/);
      expect(n?.line).not.toMatch(/macOS|Input Monitoring/);
    }
  });

  it('orders auto-mute first and rules last', () => {
    const s = state({
      playing: false,
      autoMute: 'outputChanged',
      activePackError: 'x',
      muteHotkeyError: 'y',
    });
    expect(notices(s, 'mac').map((n) => n.id)).toEqual(['automute', 'pack', 'hotkey']);
    const r = state({ playing: false, ruleBlocked: true, activePackError: 'x' });
    expect(notices(r, 'mac').map((n) => n.id)).toEqual(['pack', 'rules']);
  });
});

function pack(id: string, name: string): PackSummary {
  return {
    id,
    name,
    author: 'TakTak',
    license: 'CC0-1.0',
    description: null,
    attribution: null,
    origin: 'bundled',
    hasRelease: true,
    perKey: false,
    warnings: [],
  };
}

describe('playingName', () => {
  const packs = [pack('buckling-spring', 'Buckling Spring'), pack('my-board', 'My Board')];

  it('names the pack that plays, which is the selected one when all is well', () => {
    expect(playingName(state({ packs }))).toBe('Buckling Spring');
  });

  it('names the fallback pack, not the built-in click, when the selected pack is missing', () => {
    const s = state({
      packs,
      playingPackId: 'buckling-spring',
      activePackError: 'The pack “gone” is not installed. Playing Buckling Spring instead.',
    });
    s.settings.packId = 'gone';
    expect(playingName(s)).toBe('Buckling Spring');
  });

  it('names the selected pack while its last working version keeps playing', () => {
    // A pack that broke on disk leaves the list but keeps playing: its id is all there is.
    const s = state({
      packs: [pack('buckling-spring', 'Buckling Spring')],
      playingPackId: 'my-board',
      activePackError: 'My Board has errors … Its last working version keeps playing.',
    });
    s.settings.packId = 'my-board';
    expect(playingName(s)).toBe('my-board');
  });

  it('says built-in click only when no pack plays', () => {
    expect(playingName(state({ packs, playingPackId: null }))).toBe(BUILT_IN_SOUND);
    expect(playingName(state({ packs: [], playingPackId: null }))).toBe('Built-in click');
  });
});
