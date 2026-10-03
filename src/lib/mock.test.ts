import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createMockBackend } from './mock';
import type { AppState } from './types';

/** Lets the mock's 10 ms call delay and its event timers run. */
async function settle(ms = 20): Promise<void> {
  await vi.advanceTimersByTimeAsync(ms);
}

function setup(search = '') {
  const backend = createMockBackend(search);
  const events: AppState[] = [];
  void backend.onStateChanged((s) => events.push(s));
  async function call<T>(promise: Promise<T>): Promise<T> {
    const [result] = await Promise.all([promise, settle()]);
    return result;
  }
  return { backend, events, call };
}

describe('mock backend', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it('starts with the contract defaults and the bundled packs, sorted by name', async () => {
    const { backend, call } = setup();
    const s = await call(backend.call('get_state', undefined));
    expect(s.settings).toEqual({
      enabled: true,
      packId: 'deep-thock',
      masterVolume: 0.7,
      pressVolume: 1,
      releaseVolume: 1,
      variantMode: 'consistent',
      humanize: 0.25,
      muteHotkey: 'CommandOrControl+Alt+Shift+M',
      launchAtLogin: false,
    });
    expect(s.playing).toBe(true);
    expect(s.permission).toBe('granted');
    expect(s.audio.state).toBe('ok');
    expect(s.packs).toHaveLength(12);
    const names = s.packs.map((p) => p.name);
    expect(names).toEqual([...names].sort((a, b) => a.localeCompare(b)));
    expect(s.packs.every((p) => p.origin === 'bundled' && p.warnings.length === 0)).toBe(true);
    expect(
      s.packs
        .filter((p) => p.perKey)
        .map((p) => p.id)
        .sort(),
    ).toEqual(['blue-click', 'buckling-spring', 'crisp-clack', 'deep-thock']);
    expect(s.packs.some((p) => p.id === 'deep-thock')).toBe(true);
  });

  it('applies a command, replies with the new state and emits it after the reply', async () => {
    const { backend, events, call } = setup();
    const reply = await call(backend.call('set_enabled', { enabled: false }));
    expect(reply.settings.enabled).toBe(false);
    expect(reply.playing).toBe(false);
    expect(events.at(-1)).toEqual(reply);
  });

  it('clamps levels to 0..1', async () => {
    const { backend, call } = setup();
    expect((await call(backend.call('set_master_volume', { value: 1.7 }))).settings.masterVolume).toBe(1);
    expect((await call(backend.call('set_press_volume', { value: -2 }))).settings.pressVolume).toBe(0);
    expect((await call(backend.call('set_release_volume', { value: 0.4 }))).settings.releaseVolume).toBe(0.4);
    expect((await call(backend.call('set_humanize', { value: Number.NaN }))).settings.humanize).toBe(0);
  });

  it('mute is separate from enabled', async () => {
    const { backend, call } = setup();
    const s = await call(backend.call('set_muted', { muted: true }));
    expect(s.muted).toBe(true);
    expect(s.settings.enabled).toBe(true);
    expect(s.playing).toBe(false);
  });

  it('set_pack replies at once and emits again when the pack has loaded', async () => {
    const { backend, events, call } = setup();
    const reply = await call(backend.call('set_pack', { id: 'typewriter' }));
    expect(reply.settings.packId).toBe('typewriter');
    // Like the app: the old pack plays until the new one is swapped in.
    expect(reply.playingPackId).toBe('deep-thock');
    const before = events.length;
    await settle(200);
    expect(events.length).toBe(before + 1);
    expect(events.at(-1)?.settings.packId).toBe('typewriter');
    expect(events.at(-1)?.playingPackId).toBe('typewriter');
  });

  it('rejects an unknown pack with a message', async () => {
    const { backend, call } = setup();
    await expect(call(backend.call('set_pack', { id: 'nope' }))).rejects.toMatch(/nope/);
  });

  it('validates the mute hotkey like the app: modifiers first, one key, not taken', async () => {
    const { backend, call } = setup();
    await expect(call(backend.call('set_mute_hotkey', { accelerator: 'M' }))).rejects.toMatch(
      /takes over/,
    );
    await expect(
      call(backend.call('set_mute_hotkey', { accelerator: 'Shift+M' })),
    ).rejects.toMatch(/takes over/);
    for (const accelerator of ['F13', 'Shift+F24']) {
      const s = await call(backend.call('set_mute_hotkey', { accelerator }));
      expect(s.settings.muteHotkey).toBe(accelerator);
    }
    await expect(
      call(backend.call('set_mute_hotkey', { accelerator: 'Alt+M+K' })),
    ).rejects.toMatch(/one other key/);
    await expect(
      call(backend.call('set_mute_hotkey', { accelerator: 'CommandOrControl+Space' })),
    ).rejects.toMatch(/Spotlight/);
    const ok = await call(backend.call('set_mute_hotkey', { accelerator: 'Alt+Shift+K' }));
    expect(ok.settings.muteHotkey).toBe('Alt+Shift+K');
    const cleared = await call(backend.call('set_mute_hotkey', { accelerator: null }));
    expect(cleared.settings.muteHotkey).toBeNull();
    await call(backend.call('set_mute_hotkey', { accelerator: 'Alt+K' }));
    const empty = await call(backend.call('set_mute_hotkey', { accelerator: '' }));
    expect(empty.settings.muteHotkey).toBeNull();
  });

  it('stores the variant mode and launch at login', async () => {
    const { backend, call } = setup();
    expect((await call(backend.call('set_variant_mode', { mode: 'random' }))).settings.variantMode).toBe('random');
    expect((await call(backend.call('set_launch_at_login', { enabled: true }))).settings.launchAtLogin).toBe(true);
  });

  it('plays and stops previews without changing the active pack', async () => {
    const play = vi.fn(() => Promise.resolve());
    const pause = vi.fn();
    const created: string[] = [];
    vi.stubGlobal(
      'Audio',
      class {
        volume = 1;
        constructor(src: string) {
          created.push(src);
        }
        play = play;
        pause = pause;
      },
    );
    const { backend, call } = setup();
    await call(backend.call('preview_pack', { id: 'blue-click' }));
    expect(created).toEqual(['/packs/blue-click/preview.wav']);
    expect(play).toHaveBeenCalledOnce();
    await call(backend.call('stop_preview', undefined));
    expect(pause).toHaveBeenCalledOnce();
    const s = await call(backend.call('get_state', undefined));
    expect(s.settings.packId).toBe('deep-thock');
  });

  it('get_latency is null until 5 presses, then reports timings only', async () => {
    const { backend, call } = setup();
    expect(await call(backend.call('get_latency', undefined))).toBeNull();
    await settle(3000);
    const report = await call(backend.call('get_latency', undefined));
    expect(report).not.toBeNull();
    expect(Object.keys(report ?? {}).sort()).toEqual(
      [
        'count',
        'inputP50Ms',
        'outputMs',
        'queueP50Ms',
        'totalMaxMs',
        'totalP50Ms',
        'totalP95Ms',
      ].sort(),
    );
    expect(report?.count).toBeGreaterThanOrEqual(5);
  });

  it('scenario "denied": silent until the permission is granted', async () => {
    const { backend, events, call } = setup('?scenario=denied');
    const s = await call(backend.call('get_state', undefined));
    expect(s.permission).toBe('denied');
    expect(s.playing).toBe(false);
    await call(backend.call('open_permission_settings', undefined));
    await settle(2100);
    expect(events.at(-1)?.permission).toBe('granted');
    expect(events.at(-1)?.playing).toBe(true);
  });

  it('scenario "fault": an audio fault that recovers on another device', async () => {
    const { backend, events, call } = setup('?scenario=fault');
    const s = await call(backend.call('get_state', undefined));
    expect(s.audio.state).toBe('fault');
    expect(s.audio.message).toBeTruthy();
    expect(s.playing).toBe(false);
    await settle(4100);
    expect(events.at(-1)?.audio.state).toBe('ok');
    expect(events.at(-1)?.playing).toBe(true);
  });

  it('scenario "invalid": user packs, a failed active pack and an invalid pack', async () => {
    const { backend, events, call } = setup('?scenario=invalid');
    const s = await call(backend.call('get_state', undefined));
    expect(s.packs.filter((p) => p.origin === 'user')).toHaveLength(2);
    expect(s.settings.packId).toBe('half-done');
    // The selected pack failed, so the fallback plays and the error names it.
    expect(s.playingPackId).toBe('deep-thock');
    expect(s.activePackError).toMatch(/^Half Done could not be loaded: .* Playing Deep Thock instead\.$/);
    expect(s.invalidPacks).toHaveLength(1);
    expect(s.invalidPacks[0]?.problems.length).toBeGreaterThan(0);
    const fixed = await call(backend.call('set_pack', { id: 'my-board' }));
    expect(fixed.activePackError).toBeNull();
    await settle(200);
    expect(events.at(-1)?.playingPackId).toBe('my-board');
    expect(events.at(-1)?.activePackError).toBeNull();
    // Choosing the broken pack again fails again, and Deep Thock plays instead.
    await call(backend.call('set_pack', { id: 'half-done' }));
    await settle(200);
    expect(events.at(-1)?.playingPackId).toBe('deep-thock');
    expect(events.at(-1)?.activePackError).toMatch(/Playing Deep Thock instead/);
  });

  it('scenario "empty": no packs, the built-in click plays', async () => {
    const { backend, call } = setup('?scenario=empty,denied');
    const s = await call(backend.call('get_state', undefined));
    expect(s.packs).toEqual([]);
    expect(s.playingPackId).toBeNull();
    expect(s.activePackError).toBe('No sound packs found. Playing the built-in click instead.');
    expect(s.permission).toBe('denied');
  });

  it('hands out copies, so callers cannot change its state', async () => {
    const { backend, call } = setup();
    const s = await call(backend.call('get_state', undefined));
    s.settings.masterVolume = 0.1;
    s.packs.length = 0;
    const again = await call(backend.call('get_state', undefined));
    expect(again.settings.masterVolume).toBe(0.7);
    expect(again.packs).toHaveLength(12);
  });

  it('stops emitting to a listener after unsubscribe', async () => {
    const backend = createMockBackend('');
    const events: AppState[] = [];
    const unlisten = await backend.onStateChanged((s) => events.push(s));
    unlisten();
    await Promise.all([backend.call('set_enabled', { enabled: false }), settle()]);
    expect(events).toEqual([]);
  });
});
