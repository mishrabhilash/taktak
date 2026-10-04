import { describe, expect, it, vi } from 'vitest';
import { GRACE_MS, HOLD_MS, Optimistic } from './optimistic';
import type { AppState } from './types';

function state(patch: Partial<AppState['settings']> = {}, top: Partial<AppState> = {}): AppState {
  return {
    version: '0.1.0',
    settings: {
      enabled: true,
      packId: 'buckling-spring',
      masterVolume: 0.7,
      pressVolume: 1,
      releaseVolume: 1,
      variantMode: 'consistent',
      humanize: 0.25,
      muteHotkey: 'CommandOrControl+Alt+Shift+M',
      launchAtLogin: false,
      appRule: { mode: 'everywhere', apps: [] },
      muteOnOutputChange: false,
      onboardingDone: true,
      ...patch,
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

function setup() {
  let now = 1000;
  const onChange = vi.fn();
  const o = new Optimistic(onChange, () => now);
  return {
    o,
    onChange,
    advance(ms: number) {
      now += ms;
    },
  };
}

describe('Optimistic', () => {
  it('is null until the first state, then shows it', () => {
    const { o, onChange } = setup();
    expect(o.view).toBeNull();
    o.event(state());
    expect(o.view?.settings.masterVolume).toBe(0.7);
    expect(onChange).toHaveBeenCalled();
  });

  it('shows a sent value at once and keeps it while the command is in flight', () => {
    const { o } = setup();
    o.event(state());
    o.sent('masterVolume', 0.3);
    expect(o.view?.settings.masterVolume).toBe(0.3);
    expect(o.pending('masterVolume')).toBe(true);
    // An event from elsewhere does not pull an in-flight value back.
    o.event(state({ masterVolume: 0.7, humanize: 0.5 }));
    expect(o.view?.settings.masterVolume).toBe(0.3);
    expect(o.view?.settings.humanize).toBe(0.5);
  });

  it('drops the value once the app agrees', () => {
    const { o } = setup();
    o.event(state());
    o.sent('masterVolume', 0.3);
    const at = o.stamp;
    o.reply(state({ masterVolume: 0.3 }), at);
    o.settled('masterVolume', true);
    expect(o.pending('masterVolume')).toBe(false);
    expect(o.view?.settings.masterVolume).toBe(0.3);
  });

  it('shows the app value again at once when the command fails', () => {
    const { o } = setup();
    o.event(state());
    o.sent('packId', 'nope');
    expect(o.view?.settings.packId).toBe('nope');
    o.settled('packId', false);
    expect(o.view?.settings.packId).toBe('buckling-spring');
  });

  it('a slider is not pulled back by replies to its older values', () => {
    const { o } = setup();
    o.event(state());
    o.sent('masterVolume', 0.4);
    const first = o.stamp;
    o.sent('masterVolume', 0.5);
    o.reply(state({ masterVolume: 0.4 }), first);
    o.settled('masterVolume', true);
    expect(o.view?.settings.masterVolume).toBe(0.5);
    o.reply(state({ masterVolume: 0.5 }), o.stamp);
    o.settled('masterVolume', true);
    expect(o.view?.settings.masterVolume).toBe(0.5);
    expect(o.pending('masterVolume')).toBe(false);
  });

  it('drops a reply that an event overtook', () => {
    const { o } = setup();
    o.event(state());
    const at = o.stamp;
    o.event(state({ packId: 'typewriter' }));
    o.reply(state({ packId: 'buckling-spring' }), at);
    expect(o.view?.settings.packId).toBe('typewriter');
  });

  it('gives up on a confirmed value after the grace period if the app disagrees', () => {
    vi.useFakeTimers();
    try {
      const { o, advance } = setup();
      o.event(state());
      o.sent('enabled', false);
      o.settled('enabled', true);
      expect(o.view?.settings.enabled).toBe(false);
      advance(GRACE_MS + 1);
      vi.advanceTimersByTime(GRACE_MS + 2);
      expect(o.view?.settings.enabled).toBe(true);
    } finally {
      vi.useRealTimers();
    }
  });

  it('a shown-but-never-sent value expires after the hold time', () => {
    vi.useFakeTimers();
    try {
      const { o, advance } = setup();
      o.event(state());
      o.show('humanize', 0.9);
      expect(o.view?.settings.humanize).toBe(0.9);
      advance(HOLD_MS + 1);
      vi.advanceTimersByTime(HOLD_MS + 2);
      expect(o.view?.settings.humanize).toBe(0.25);
      o.dispose();
    } finally {
      vi.useRealTimers();
    }
  });

  it('recomputes playing for an optimistic mute or on/off', () => {
    const { o } = setup();
    o.event(state());
    o.sent('muted', true);
    expect(o.view?.muted).toBe(true);
    expect(o.view?.playing).toBe(false);
    o.settled('muted', false);
    expect(o.view?.playing).toBe(true);
    o.event(state({}, { permission: 'denied', playing: false }));
    o.sent('enabled', true);
    expect(o.view?.playing).toBe(false);
  });

  it('an optimistic unmute or sounds-on clears an outputChanged auto-mute, as the app does', () => {
    const { o } = setup();
    o.event(state({}, { autoMute: 'outputChanged', playing: false }));
    o.sent('muted', false);
    expect(o.view?.autoMute).toBeNull();
    expect(o.view?.playing).toBe(true);
    o.settled('muted', false);
    expect(o.view?.autoMute).toBe('outputChanged');
    o.sent('enabled', true);
    expect(o.view?.autoMute).toBeNull();
    o.settled('enabled', false);
    // Turning sounds off leaves it alone.
    o.sent('enabled', false);
    expect(o.view?.autoMute).toBe('outputChanged');
    expect(o.view?.playing).toBe(false);
  });

  it('a screen lock or a rule block keeps an optimistic unmute silent', () => {
    const { o } = setup();
    o.event(state({}, { muted: true, autoMute: 'screenLocked', playing: false }));
    o.sent('muted', false);
    expect(o.view?.autoMute).toBe('screenLocked');
    expect(o.view?.playing).toBe(false);
    o.settled('muted', false);
    o.event(state({}, { muted: true, ruleBlocked: true, playing: false }));
    o.sent('muted', false);
    expect(o.view?.playing).toBe(false);
  });

  it('a rule edit re-evaluates ruleBlocked and playing at once, and waits for the app to agree', () => {
    const { o } = setup();
    const slack = { id: 'com.tinyspeck.slackmacgap', name: 'Slack' };
    o.event(state({}, { frontmostApp: slack }));
    const never = { mode: 'never' as const, apps: [slack] };
    o.sent('appRule', never);
    expect(o.view?.settings.appRule).toEqual(never);
    expect(o.view?.ruleBlocked).toBe(true);
    expect(o.view?.playing).toBe(false);
    o.settled('appRule', true);
    // The app's state, an equal copy: the optimistic value goes.
    o.event(
      state(
        { appRule: { mode: 'never', apps: [{ ...slack }] } },
        { frontmostApp: slack, ruleBlocked: true, playing: false },
      ),
    );
    expect(o.pending('appRule')).toBe(false);
    // Removing it again: playing comes back before the reply.
    o.sent('appRule', { mode: 'never', apps: [] });
    expect(o.view?.ruleBlocked).toBe(false);
    expect(o.view?.playing).toBe(true);
    o.settled('appRule', false);
    expect(o.view?.ruleBlocked).toBe(true);
  });

  it('two quick rule edits are not pulled back by the first reply', () => {
    const { o } = setup();
    const a = { id: 'a.app', name: 'A' };
    const b = { id: 'b.app', name: 'B' };
    o.event(state({ appRule: { mode: 'never', apps: [a, b] } }));
    o.sent('appRule', { mode: 'never', apps: [b] });
    o.sent('appRule', { mode: 'never', apps: [] });
    o.reply(state({ appRule: { mode: 'never', apps: [b] } }), o.stamp);
    o.settled('appRule', true);
    expect(o.view?.settings.appRule.apps).toEqual([]);
  });

  it('turning "mute when the output changes" off clears that auto-mute at once', () => {
    const { o } = setup();
    o.event(state({ muteOnOutputChange: true }, { autoMute: 'outputChanged', playing: false }));
    o.sent('muteOnOutputChange', false);
    expect(o.view?.autoMute).toBeNull();
    expect(o.view?.playing).toBe(true);
    o.settled('muteOnOutputChange', false);
    o.sent('muteOnOutputChange', true);
    expect(o.view?.autoMute).toBe('outputChanged');
    expect(o.view?.playing).toBe(false);
  });
});
