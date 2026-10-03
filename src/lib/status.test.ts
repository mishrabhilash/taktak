import { describe, expect, it } from 'vitest';
import { detectPlatform, permissionName } from './platform';
import { BUILT_IN_SOUND, grantLabel, notices, playbackStatus, playingName } from './status';
import type { AppState, PackSummary } from './types';

function state(top: Partial<AppState> = {}, enabled = true): AppState {
  return {
    version: '0.1.0',
    settings: {
      enabled,
      packId: 'deep-thock',
      masterVolume: 0.7,
      pressVolume: 1,
      releaseVolume: 1,
      variantMode: 'consistent',
      humanize: 0.25,
      muteHotkey: null,
      launchAtLogin: false,
    },
    muted: false,
    playing: true,
    packs: [],
    invalidPacks: [],
    playingPackId: 'deep-thock',
    activePackError: null,
    muteHotkeyError: null,
    userPacksDir: null,
    permission: 'granted',
    audio: { device: 'Speakers', sampleRate: 48000, bufferFrames: 64, state: 'ok', message: null },
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
    expect(list[0]?.permissionAction).toBe(false);
  });

  it('is empty when all is well', () => {
    expect(notices(state(), 'mac')).toEqual([]);
  });

  it('offers the permission button when Input Monitoring is missing', () => {
    const [n] = notices(state({ permission: 'denied', playing: false }), 'mac');
    expect(n?.id).toBe('permission');
    expect(n?.permissionAction).toBe(true);
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
  const packs = [pack('deep-thock', 'Deep Thock'), pack('my-board', 'My Board')];

  it('names the pack that plays, which is the selected one when all is well', () => {
    expect(playingName(state({ packs }))).toBe('Deep Thock');
  });

  it('names the fallback pack, not the built-in click, when the selected pack is missing', () => {
    const s = state({
      packs,
      playingPackId: 'deep-thock',
      activePackError: 'The pack “gone” is not installed. Playing Deep Thock instead.',
    });
    s.settings.packId = 'gone';
    expect(playingName(s)).toBe('Deep Thock');
  });

  it('names the selected pack while its last working version keeps playing', () => {
    // A pack that broke on disk leaves the list but keeps playing: its id is all there is.
    const s = state({
      packs: [pack('deep-thock', 'Deep Thock')],
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
