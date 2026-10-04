import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createMockBackend, onboardingUrl } from './mock';
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
      // A returning user (see scenario "firstrun").
      onboardingDone: true,
    });
    expect(s.playing).toBe(true);
    expect(s.frontmostApp).toEqual({ id: 'com.microsoft.VSCode', name: 'Visual Studio Code' });
    expect(s.ruleBlocked).toBe(false);
    expect(s.autoMute).toBeNull();
    expect(s.rulesSupported).toBe(true);
    expect(s.onboarding).toEqual({
      offer: false,
      permissionRequired: true,
      relaunchSuggested: false,
      inputGroupNeeded: false,
    });
    expect(s.permission).toBe('granted');
    expect(s.audio.state).toBe('ok');
    expect(s.packs).toHaveLength(8);
    const names = s.packs.map((p) => p.name);
    expect(names).toEqual([...names].sort((a, b) => a.localeCompare(b)));
    expect(s.packs.every((p) => p.origin === 'bundled' && p.warnings.length === 0)).toBe(true);
    expect(s.packs.some((p) => p.perKey)).toBe(false);
    expect(s.packs.some((p) => p.id === 'tactile')).toBe(true);
    // The packs earlier versions bundled are gone (Buckling Spring is withheld for now).
    expect(
      s.packs.some((p) => ['deep-thock', 'crisp-clack', 'blue-click', 'buckling-spring'].includes(p.id)),
    ).toBe(false);
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
    expect(reply.playingPackId).toBe('tactile');
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
    await call(backend.call('preview_pack', { id: 'typewriter' }));
    expect(created).toEqual(['/packs/typewriter/preview.wav']);
    expect(play).toHaveBeenCalledOnce();
    await call(backend.call('stop_preview', undefined));
    expect(pause).toHaveBeenCalledOnce();
    const s = await call(backend.call('get_state', undefined));
    expect(s.settings.packId).toBe('tactile');
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
    expect(await call(backend.call('open_permission_settings', undefined))).toBe(true);
    await settle(2100);
    expect(events.at(-1)?.permission).toBe('granted');
    expect(events.at(-1)?.playing).toBe(true);
  });

  it('scenario "unlisted": macOS adds nothing to the list until TakTak is added by hand', async () => {
    const { backend, events, call } = setup('?scenario=unlisted');
    const s = await call(backend.call('get_state', undefined));
    expect(s.permission).toBe('denied');
    expect(await call(backend.call('open_permission_settings', undefined))).toBe(false);
    await settle(2100);
    expect(events.at(-1)?.permission ?? 'denied').toBe('denied');
    await call(backend.call('reveal_app', undefined));
    await settle(6100);
    expect(events.at(-1)?.permission).toBe('granted');
    // Listed and granted now: the button answers true.
    expect(await call(backend.call('open_permission_settings', undefined))).toBe(true);
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
    expect(s.playingPackId).toBe('tactile');
    expect(s.activePackError).toMatch(/^Half Done could not be loaded: .* Playing Tactile instead\.$/);
    expect(s.invalidPacks).toHaveLength(1);
    expect(s.invalidPacks[0]?.problems.length).toBeGreaterThan(0);
    const fixed = await call(backend.call('set_pack', { id: 'my-board' }));
    expect(fixed.activePackError).toBeNull();
    await settle(200);
    expect(events.at(-1)?.playingPackId).toBe('my-board');
    expect(events.at(-1)?.activePackError).toBeNull();
    // Choosing the broken pack again fails again, and Tactile plays instead.
    await call(backend.call('set_pack', { id: 'half-done' }));
    await settle(200);
    expect(events.at(-1)?.playingPackId).toBe('tactile');
    expect(events.at(-1)?.activePackError).toMatch(/Playing Tactile instead/);
  });

  it('scenario "empty": no packs, the built-in click plays', async () => {
    const { backend, call } = setup('?scenario=empty,denied');
    const s = await call(backend.call('get_state', undefined));
    expect(s.packs).toEqual([]);
    expect(s.playingPackId).toBeNull();
    expect(s.activePackError).toBe('No sound packs found. Playing the built-in click instead.');
    expect(s.permission).toBe('denied');
  });

  it('scenario "firstrun": contract defaults, and the onboarding is offered until finished', async () => {
    const { backend, events, call } = setup('?scenario=firstrun');
    const s = await call(backend.call('get_state', undefined));
    expect(s.settings.onboardingDone).toBe(false);
    expect(s.settings.appRule).toEqual({ mode: 'everywhere', apps: [] });
    expect(s.settings.muteOnOutputChange).toBe(false);
    expect(s.onboarding.offer).toBe(true);
    // Permission is fine here, so the offer is only about the first launch.
    expect(s.playing).toBe(true);
    await expect(call(backend.call('open_onboarding', undefined))).resolves.toBeUndefined();
    const done = await call(backend.call('finish_onboarding', undefined));
    expect(done.settings.onboardingDone).toBe(true);
    expect(done.onboarding.offer).toBe(false);
    expect(events.at(-1)).toEqual(done);
  });

  it('offers the onboarding while permission is missing, done or not', async () => {
    const { backend, events, call } = setup('?scenario=denied');
    const s = await call(backend.call('get_state', undefined));
    expect(s.settings.onboardingDone).toBe(true);
    expect(s.onboarding.offer).toBe(true);
    expect(s.onboarding.relaunchSuggested).toBe(false);
    await call(backend.call('open_permission_settings', undefined));
    await settle(2100);
    expect(events.at(-1)?.permission).toBe('granted');
    expect(events.at(-1)?.onboarding.offer).toBe(false);
  });

  it('scenario "relaunch": only a relaunch lets the listener start', async () => {
    const { backend, events, call } = setup('?scenario=relaunch');
    const s = await call(backend.call('get_state', undefined));
    expect(s.permission).toBe('denied');
    expect(s.onboarding.relaunchSuggested).toBe(true);
    expect(s.onboarding.offer).toBe(true);
    await call(backend.call('open_permission_settings', undefined));
    await settle(2500);
    expect((await call(backend.call('get_state', undefined))).permission).toBe('denied');
    await expect(call(backend.call('relaunch', undefined))).resolves.toBeUndefined();
    await settle(1600);
    const after = events.at(-1);
    expect(after?.permission).toBe('granted');
    expect(after?.onboarding.relaunchSuggested).toBe(false);
    expect(after?.playing).toBe(true);
  });

  it('lists running apps by name, with icons where they have one', async () => {
    const { backend, call } = setup();
    const apps = await call(backend.call('list_running_apps', undefined));
    expect(apps.length).toBe(10);
    const names = apps.map((a) => a.name);
    expect(names).toEqual([...names].sort((a, b) => a.localeCompare(b)));
    expect(apps.some((a) => a.id === 'tech.taktak.app')).toBe(false);
    expect(new Set(apps.map((a) => a.id)).size).toBe(apps.length);
    expect(apps.filter((a) => a.iconDataUrl === null).map((a) => a.name)).toEqual(['kitty']);
    expect(apps.every((a) => a.iconDataUrl === null || a.iconDataUrl.startsWith('data:image/'))).toBe(
      true,
    );
  });

  it('choose_app returns Word, Discord, Obsidian, then null (cancelled), in turn', async () => {
    const { backend, call } = setup();
    const picks = [];
    for (let i = 0; i < 5; i += 1) picks.push(await call(backend.call('choose_app', undefined)));
    expect(picks.map((p) => p?.name ?? null)).toEqual([
      'Microsoft Word',
      'Discord',
      'Obsidian',
      null,
      'Microsoft Word',
    ]);
    expect(picks[0]?.id).toBe('com.microsoft.Word');
    expect(picks[0]?.iconDataUrl).toMatch(/^data:image\//);
  });

  it('get_app_icons answers every requested id, null for unknown apps', async () => {
    const { backend, call } = setup();
    const icons = await call(
      backend.call('get_app_icons', { ids: ['com.apple.Safari', 'md.obsidian', 'net.kovidgoyal.kitty', 'x.y'] }),
    );
    expect(Object.keys(icons).sort()).toEqual(
      ['com.apple.Safari', 'md.obsidian', 'net.kovidgoyal.kitty', 'x.y'].sort(),
    );
    expect(icons['com.apple.Safari']).toMatch(/^data:image\//);
    expect(icons['md.obsidian']).toMatch(/^data:image\//);
    expect(icons['net.kovidgoyal.kitty']).toBeNull();
    expect(icons['x.y']).toBeNull();
  });

  it('adds and removes rule apps with the app’s checks', async () => {
    const { backend, events, call } = setup();
    let s = await call(backend.call('add_rule_app', { id: ' com.tinyspeck.slackmacgap ', name: ' Slack ' }));
    expect(s.settings.appRule.apps).toEqual([{ id: 'com.tinyspeck.slackmacgap', name: 'Slack' }]);
    s = await call(backend.call('add_rule_app', { id: 'us.zoom.xos', name: '' }));
    expect(s.settings.appRule.apps.at(-1)).toEqual({ id: 'us.zoom.xos', name: 'us.zoom.xos' });
    // Already listed: nothing changes, no error, no event.
    const before = events.length;
    s = await call(backend.call('add_rule_app', { id: 'com.tinyspeck.slackmacgap', name: 'Other' }));
    expect(s.settings.appRule.apps).toHaveLength(2);
    expect(s.settings.appRule.apps[0]?.name).toBe('Slack');
    expect(events.length).toBe(before);
    for (const id of ['', '   ', 'has space', 'tab\there', 'x'.repeat(256)]) {
      await expect(call(backend.call('add_rule_app', { id, name: 'X' }))).rejects.toMatch(
        /not an app TakTak can recognize/,
      );
    }
    await expect(
      call(backend.call('add_rule_app', { id: 'tech.taktak.app', name: 'TakTak' })),
    ).rejects.toMatch(/TakTak itself/);
    s = await call(backend.call('remove_rule_app', { id: 'com.tinyspeck.slackmacgap' }));
    expect(s.settings.appRule.apps.map((a) => a.id)).toEqual(['us.zoom.xos']);
    s = await call(backend.call('remove_rule_app', { id: 'not.listed' }));
    expect(s.settings.appRule.apps).toHaveLength(1);
  });

  it('caps the rule list at 200 apps', async () => {
    const { backend, call } = setup();
    for (let i = 0; i < 200; i += 1) {
      await call(backend.call('add_rule_app', { id: `com.example.app${i}`, name: `App ${i}` }));
    }
    await expect(
      call(backend.call('add_rule_app', { id: 'com.example.one-more', name: 'One more' })),
    ).rejects.toMatch(/up to 200 apps/);
    // Re-adding a listed app is still fine.
    const s = await call(backend.call('add_rule_app', { id: 'com.example.app0', name: 'App 0' }));
    expect(s.settings.appRule.apps).toHaveLength(200);
  });

  it('scenario "rules": silent in a listed app; the mode decides what the list means', async () => {
    const { backend, call } = setup('?scenario=rules');
    let s = await call(backend.call('get_state', undefined));
    expect(s.settings.appRule.mode).toBe('never');
    expect(s.frontmostApp?.name).toBe('Slack');
    expect(s.ruleBlocked).toBe(true);
    expect(s.playing).toBe(false);
    s = await call(backend.call('set_app_rule_mode', { mode: 'only' }));
    expect(s.ruleBlocked).toBe(false);
    expect(s.playing).toBe(true);
    expect(s.settings.appRule.apps).toHaveLength(2);
    s = await call(backend.call('remove_rule_app', { id: 'com.tinyspeck.slackmacgap' }));
    expect(s.ruleBlocked).toBe(true);
    s = await call(backend.call('set_app_rule_mode', { mode: 'everywhere' }));
    expect(s.ruleBlocked).toBe(false);
    expect(s.playing).toBe(true);
  });

  it('scenario "switching": the frontmost app changes every 3 s, and the rule follows', async () => {
    const { backend, events, call } = setup('?scenario=switching');
    await call(backend.call('add_rule_app', { id: 'com.tinyspeck.slackmacgap', name: 'Slack' }));
    await call(backend.call('set_app_rule_mode', { mode: 'never' }));
    await settle(3000);
    expect(events.at(-1)?.frontmostApp?.name).toBe('Slack');
    expect(events.at(-1)?.ruleBlocked).toBe(true);
    await settle(3000);
    expect(events.at(-1)?.frontmostApp?.name).toBe('Safari');
    expect(events.at(-1)?.ruleBlocked).toBe(false);
    expect(events.at(-1)?.playing).toBe(true);
  });

  it('scenario "locked": auto-muted until the unlock, which no mute command clears', async () => {
    const { backend, events, call } = setup('?scenario=locked');
    let s = await call(backend.call('get_state', undefined));
    expect(s.autoMute).toBe('screenLocked');
    expect(s.muted).toBe(false);
    expect(s.playing).toBe(false);
    s = await call(backend.call('set_muted', { muted: false }));
    expect(s.autoMute).toBe('screenLocked');
    await settle(4000);
    expect(events.at(-1)?.autoMute).toBeNull();
    expect(events.at(-1)?.playing).toBe(true);
  });

  it('scenario "outputchange": mutes on a new output until the user unmutes', async () => {
    const { backend, events, call } = setup('?scenario=outputchange');
    let s = await call(backend.call('get_state', undefined));
    expect(s.settings.muteOnOutputChange).toBe(true);
    expect(s.autoMute).toBeNull();
    await settle(3000);
    s = events.at(-1) as AppState;
    expect(s.audio.device).toBe('AirPods Pro');
    expect(s.autoMute).toBe('outputChanged');
    expect(s.muted).toBe(false);
    expect(s.playing).toBe(false);
    // The manual mute takes over...
    s = await call(backend.call('set_muted', { muted: true }));
    expect(s.autoMute).toBeNull();
    expect(s.muted).toBe(true);
    // ...and unmuting plays again.
    s = await call(backend.call('set_muted', { muted: false }));
    expect(s.playing).toBe(true);
  });

  it('an output change while muted by hand or with the setting off does not auto-mute', async () => {
    const muted = setup('?scenario=outputchange');
    await muted.call(muted.backend.call('set_muted', { muted: true }));
    await settle(3000);
    expect(muted.events.at(-1)?.autoMute).toBeNull();
    const off = setup('?scenario=outputchange');
    await off.call(off.backend.call('set_mute_on_output_change', { enabled: false }));
    await settle(3000);
    expect(off.events.at(-1)?.audio.device).toBe('AirPods Pro');
    expect(off.events.at(-1)?.autoMute).toBeNull();
  });

  it('turning sounds on or the setting off clears an outputChanged auto-mute', async () => {
    const a = setup('?scenario=outputchange');
    await settle(3100);
    expect(a.events.at(-1)?.autoMute).toBe('outputChanged');
    const offAgain = await a.call(a.backend.call('set_enabled', { enabled: false }));
    expect(offAgain.autoMute).toBe('outputChanged');
    expect((await a.call(a.backend.call('set_enabled', { enabled: true }))).autoMute).toBeNull();
    const b = setup('?scenario=outputchange');
    await settle(3100);
    const s = await b.call(b.backend.call('set_mute_on_output_change', { enabled: false }));
    expect(s.autoMute).toBeNull();
    expect(s.playing).toBe(true);
  });

  it('scenario "unsupported": no rules, no app list, no permission step', async () => {
    const { backend, call } = setup('?scenario=unsupported');
    let s = await call(backend.call('get_state', undefined));
    expect(s.rulesSupported).toBe(false);
    expect(s.frontmostApp).toBeNull();
    expect(s.onboarding.permissionRequired).toBe(false);
    expect(await call(backend.call('list_running_apps', undefined))).toEqual([]);
    await expect(call(backend.call('choose_app', undefined))).rejects.toMatch(/aren’t available/);
    expect(await call(backend.call('get_app_icons', { ids: ['com.apple.Safari'] }))).toEqual({
      'com.apple.Safari': null,
    });
    // Rules are kept but ignored.
    s = await call(backend.call('set_app_rule_mode', { mode: 'only' }));
    expect(s.settings.appRule.mode).toBe('only');
    expect(s.ruleBlocked).toBe(false);
    expect(s.playing).toBe(true);
  });

  it('scenario "inputgroup": Linux without the input group; no onboarding nag (M5)', async () => {
    const { backend, call } = setup('?scenario=inputgroup');
    const s = await call(backend.call('get_state', undefined));
    expect(s.permission).toBe('denied');
    expect(s.onboarding).toEqual({
      offer: false,
      permissionRequired: false,
      relaunchSuggested: false,
      inputGroupNeeded: true,
    });
    expect(s.playing).toBe(false);
    expect(s.rulesSupported).toBe(false);
  });

  it('scenario "listenerfail": a relaunch fixes a listener that failed to start (M5)', async () => {
    const { backend, events, call } = setup('?scenario=listenerfail');
    const s = await call(backend.call('get_state', undefined));
    expect(s.onboarding).toMatchObject({ permissionRequired: false, relaunchSuggested: true });
    await call(backend.call('relaunch', undefined));
    await settle(1600);
    expect(events.at(-1)?.permission).toBe('granted');
  });

  it('imports a Mechvibes pack, then finds it imported, then fails, then is cancelled (M5)', async () => {
    const { backend, events, call } = setup();
    const first = await call(backend.call('import_mechvibes_pack', { kind: 'any' }));
    expect(first).toMatchObject({ outcome: 'imported', pack: { id: 'mv-cream-linear', replaced: false } });
    await settle(800);
    const listed = events.at(-1)?.packs.find((p) => p.id === 'mv-cream-linear');
    expect(listed).toMatchObject({ license: 'LicenseRef-Personal', origin: 'user' });

    const again = await call(backend.call('import_mechvibes_pack', { kind: 'zip' }));
    expect(again).toEqual({ outcome: 'alreadyImported', id: 'mv-cream-linear', source: 'cream-linear.zip' });
    const replaced = await call(backend.call('overwrite_mechvibes_pack', undefined));
    expect(replaced).toMatchObject({ outcome: 'imported', pack: { replaced: true } });
    await settle(800);
    // Still listed once.
    expect(events.at(-1)?.packs.filter((p) => p.id === 'mv-cream-linear')).toHaveLength(1);
    await expect(call(backend.call('overwrite_mechvibes_pack', undefined))).rejects.toMatch(
      /no import waiting/,
    );

    await expect(call(backend.call('import_mechvibes_pack', { kind: 'folder' }))).rejects.toMatch(
      /No usable sounds/,
    );
    expect(await call(backend.call('import_mechvibes_pack', { kind: 'any' }))).toBeNull();
  });

  it('hands out copies, so callers cannot change its state', async () => {
    const { backend, call } = setup();
    const s = await call(backend.call('get_state', undefined));
    s.settings.masterVolume = 0.1;
    s.packs.length = 0;
    const again = await call(backend.call('get_state', undefined));
    expect(again.settings.masterVolume).toBe(0.7);
    expect(again.packs).toHaveLength(8);
  });

  it('stops emitting to a listener after unsubscribe', async () => {
    const backend = createMockBackend('');
    const events: AppState[] = [];
    const unlisten = await backend.onStateChanged((s) => events.push(s));
    unlisten();
    await Promise.all([backend.call('set_enabled', { enabled: false }), settle()]);
    expect(events).toEqual([]);
  });

  it('opens the onboarding view with the same scenarios', () => {
    expect(onboardingUrl('')).toBe('?window=onboarding');
    expect(onboardingUrl('?window=settings&scenario=denied,fault')).toBe(
      '?window=onboarding&scenario=denied%2Cfault',
    );
  });
});
