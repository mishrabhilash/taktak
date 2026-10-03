// The typed wrappers outside Tauri: they reach the in-browser mock with the contract's
// command names and argument shapes.

import { describe, expect, it } from 'vitest';
import * as api from './api';
import type { AppState } from './types';

describe('api (mock backend)', () => {
  it('is not inside Tauri here', () => {
    expect(api.isTauri).toBe(false);
  });

  it('uses one backend for the whole page', async () => {
    expect(await api.backend()).toBe(await api.backend());
  });

  it('round-trips every state command through the wrappers', async () => {
    const events: AppState[] = [];
    const unlisten = await api.onStateChanged((s) => events.push(s));
    try {
      const initial = await api.getState();
      expect(initial.settings.packId).toBe('deep-thock');

      expect((await api.setEnabled(false)).settings.enabled).toBe(false);
      expect((await api.setEnabled(true)).settings.enabled).toBe(true);
      expect((await api.setMuted(true)).muted).toBe(true);
      expect((await api.setMuted(false)).muted).toBe(false);
      expect((await api.setPack('typewriter')).settings.packId).toBe('typewriter');
      expect((await api.setMasterVolume(0.33)).settings.masterVolume).toBe(0.33);
      expect((await api.setPressVolume(2)).settings.pressVolume).toBe(1);
      expect((await api.setReleaseVolume(0.5)).settings.releaseVolume).toBe(0.5);
      expect((await api.setVariantMode('random')).settings.variantMode).toBe('random');
      expect((await api.setHumanize(0.8)).settings.humanize).toBe(0.8);
      expect((await api.setMuteHotkey('Alt+Shift+K')).settings.muteHotkey).toBe('Alt+Shift+K');
      expect((await api.setMuteHotkey(null)).settings.muteHotkey).toBeNull();
      expect((await api.setLaunchAtLogin(true)).settings.launchAtLogin).toBe(true);

      // Events follow the replies.
      await new Promise((resolve) => setTimeout(resolve, 0));
      expect(events.length).toBeGreaterThanOrEqual(13);
      expect(events.at(-1)?.settings.launchAtLogin).toBe(true);
    } finally {
      unlisten();
    }
  });

  it('void commands resolve to undefined', async () => {
    await expect(api.stopPreview()).resolves.toBeUndefined();
    await expect(api.openSettings()).resolves.toBeUndefined();
    await expect(api.hideTray()).resolves.toBeUndefined();
    await expect(api.openUserPacksDir()).resolves.toBeUndefined();
    await expect(api.openPermissionSettings()).resolves.toBeUndefined();
    await expect(api.quit()).resolves.toBeUndefined();
  });

  it('get_latency starts out null', async () => {
    await expect(api.getLatency()).resolves.toBeNull();
  });

  it('a rejection carries a user-facing message', async () => {
    const error: unknown = await api.setMuteHotkey('M').catch((e: unknown) => e);
    expect(api.errorMessage(error)).toMatch(/takes over a key you type/);
  });

  it('errorMessage handles strings, errors and anything else', () => {
    expect(api.errorMessage('Shortcut already in use')).toBe('Shortcut already in use');
    expect(api.errorMessage(new Error('boom'))).toBe('boom');
    expect(api.errorMessage(42)).toBe('42');
  });
});
