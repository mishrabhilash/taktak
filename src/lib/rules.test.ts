import { describe, expect, it } from 'vitest';
import {
  MAX_RULE_APPS,
  OWN_APP_ID,
  RULE_MODES,
  frontmostSummary,
  isListed,
  ruleAppProblem,
  ruleBlocks,
  searchApps,
  withApp,
  withMode,
  withoutApp,
} from './rules';
import type { AppInfo, AppRule, AppState } from './types';

const slack = { id: 'com.tinyspeck.slackmacgap', name: 'Slack' };
const safari = { id: 'com.apple.Safari', name: 'Safari' };
const zoom = { id: 'us.zoom.xos', name: 'zoom.us' };
const rule = (mode: AppRule['mode'], apps = [slack]): AppRule => ({ mode, apps });

describe('ruleBlocks', () => {
  it('follows the mode for listed, unlisted and unknown apps', () => {
    expect(ruleBlocks(rule('everywhere'), slack, true)).toBe(false);
    expect(ruleBlocks(rule('everywhere'), null, true)).toBe(false);
    expect(ruleBlocks(rule('only'), slack, true)).toBe(false);
    expect(ruleBlocks(rule('only'), safari, true)).toBe(true);
    expect(ruleBlocks(rule('only'), null, true)).toBe(true);
    expect(ruleBlocks(rule('never'), slack, true)).toBe(true);
    expect(ruleBlocks(rule('never'), safari, true)).toBe(false);
    expect(ruleBlocks(rule('never'), null, true)).toBe(false);
  });

  it('an empty "only" list is silent everywhere', () => {
    expect(ruleBlocks({ mode: 'only', apps: [] }, safari, true)).toBe(true);
  });

  it('never blocks where rules are unsupported', () => {
    for (const mode of ['everywhere', 'only', 'never'] as const) {
      expect(ruleBlocks(rule(mode), slack, false)).toBe(false);
      expect(ruleBlocks(rule(mode), null, false)).toBe(false);
    }
  });
});

describe('editing the list', () => {
  it('appends in order, trimmed, with the id standing in for an empty name', () => {
    let r = rule('never', []);
    r = withApp(r, slack);
    r = withApp(r, { id: '  com.apple.Safari ', name: '  Safari  ' });
    r = withApp(r, { id: 'org.example.nameless', name: '   ' });
    expect(r.apps).toEqual([slack, safari, { id: 'org.example.nameless', name: 'org.example.nameless' }]);
    expect(r.mode).toBe('never');
  });

  it('an app that is already listed changes nothing and keeps its entry', () => {
    const r = rule('only', [slack]);
    expect(withApp(r, { id: slack.id, name: 'Renamed' })).toBe(r);
    expect(withApp(r, { id: ` ${slack.id}`, name: 'Renamed' })).toBe(r);
  });

  it('removes by id; an unlisted id changes nothing', () => {
    const r = rule('never', [slack, safari, zoom]);
    expect(withoutApp(r, safari.id).apps).toEqual([slack, zoom]);
    expect(withoutApp(r, 'com.example.none')).toBe(r);
  });

  it('a mode change keeps the list', () => {
    const r = rule('only', [slack, zoom]);
    expect(withMode(r, 'never')).toEqual({ mode: 'never', apps: [slack, zoom] });
    expect(withMode(r, 'only')).toBe(r);
  });

  it('does not change its input', () => {
    const r = rule('only', [slack]);
    withApp(r, safari);
    withoutApp(r, slack.id);
    withMode(r, 'never');
    expect(r).toEqual(rule('only', [slack]));
  });

  it('rejects like add_rule_app, with its messages', () => {
    const r = rule('never', []);
    for (const bad of ['', '   ', 'has space', 'tab\there', 'ctrl\u0007', 'x'.repeat(256)]) {
      expect(ruleAppProblem(r, bad)).toBe('That is not an app TakTak can recognize.');
      expect(withApp(r, { id: bad, name: 'Bad' })).toBe(r);
    }
    expect(ruleAppProblem(r, 'x'.repeat(255))).toBeNull();
    expect(ruleAppProblem(r, OWN_APP_ID)).toMatch(/TakTak itself can’t be listed/);
    expect(withApp(r, { id: OWN_APP_ID, name: 'TakTak' })).toBe(r);
    expect(ruleAppProblem(r, slack.id)).toBeNull();
  });

  it('caps the list at 200, but re-adding a listed app is fine', () => {
    let r = rule('never', []);
    for (let i = 0; i < MAX_RULE_APPS; i += 1) r = withApp(r, { id: `com.example.app${i}`, name: `App ${i}` });
    expect(r.apps).toHaveLength(200);
    expect(ruleAppProblem(r, 'com.example.more')).toBe('You can list up to 200 apps.');
    expect(withApp(r, { id: 'com.example.more', name: 'More' })).toBe(r);
    expect(ruleAppProblem(r, 'com.example.app0')).toBeNull();
    expect(isListed(r, 'com.example.app199')).toBe(true);
  });
});

describe('RULE_MODES', () => {
  it('offers the three modes in order, each with a one-line hint', () => {
    expect(RULE_MODES.map((m) => m.mode)).toEqual(['everywhere', 'only', 'never']);
    expect(RULE_MODES.map((m) => m.label)).toEqual([
      'Everywhere',
      'Only in these apps',
      'Never in these apps',
    ]);
    expect(RULE_MODES.every((m) => m.hint.length > 0 && !m.hint.includes('\n'))).toBe(true);
  });
});

describe('searchApps', () => {
  const apps: AppInfo[] = [
    { id: 'com.apple.finder', name: 'Finder', iconDataUrl: null },
    { id: 'com.google.Chrome', name: 'Google Chrome', iconDataUrl: null },
    { id: 'com.microsoft.VSCode', name: 'Visual Studio Code', iconDataUrl: null },
    { id: 'com.example.cafe', name: 'Café Notes', iconDataUrl: null },
  ];
  const names = (q: string) => searchApps(apps, q).map((a) => a.name);

  it('matches names and bundle ids, ignoring case and accents, every word', () => {
    expect(names('')).toEqual(apps.map((a) => a.name));
    expect(names('  ')).toHaveLength(4);
    expect(names('CHROME')).toEqual(['Google Chrome']);
    expect(names('vscode')).toEqual(['Visual Studio Code']);
    expect(names('studio visual')).toEqual(['Visual Studio Code']);
    expect(names('cafe')).toEqual(['Café Notes']);
    expect(names('café')).toEqual(['Café Notes']);
    expect(names('com.apple')).toEqual(['Finder']);
    expect(names('zzz')).toEqual([]);
  });

  it('returns a new array', () => {
    expect(searchApps(apps, '')).not.toBe(apps);
  });
});

function appState(top: Partial<AppState>, appRule: AppRule): AppState {
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
      muteHotkey: null,
      launchAtLogin: false,
      appRule,
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
    frontmostApp: safari,
    ruleBlocked: false,
    autoMute: null,
    rulesSupported: true,
    onboarding: { offer: false, permissionRequired: true, relaunchSuggested: false },
    ...top,
  };
}

describe('frontmostSummary', () => {
  it('says why TakTak is silent in the app in front', () => {
    const s = appState({ frontmostApp: { id: 'com.apple.Terminal', name: 'Terminal' }, ruleBlocked: true }, rule('only'));
    expect(frontmostSummary(s)).toEqual({
      text: 'TakTak is silent in Terminal because of your rules.',
      silent: true,
      canAdd: true,
    });
  });

  it('says it plays, and why, per mode', () => {
    expect(frontmostSummary(appState({}, rule('everywhere')))?.text).toBe('TakTak plays in Safari.');
    expect(frontmostSummary(appState({}, rule('never')))?.text).toBe(
      'TakTak plays in Safari: it isn’t on your list.',
    );
    expect(frontmostSummary(appState({}, rule('only', [safari])))?.text).toBe(
      'TakTak plays in Safari: it’s on your list.',
    );
  });

  it('offers to add only an app that is known, not listed and allowed', () => {
    expect(frontmostSummary(appState({}, rule('never')))?.canAdd).toBe(true);
    expect(frontmostSummary(appState({}, rule('never', [safari])))?.canAdd).toBe(false);
    expect(frontmostSummary(appState({ frontmostApp: null }, rule('never')))?.canAdd).toBe(false);
    expect(
      frontmostSummary(appState({ frontmostApp: { id: OWN_APP_ID, name: 'TakTak' } }, rule('never')))
        ?.canAdd,
    ).toBe(false);
  });

  it('handles an unknown app in front', () => {
    const silent = frontmostSummary(appState({ frontmostApp: null, ruleBlocked: true }, rule('only')));
    expect(silent?.text).toBe('TakTak can’t tell which app is in front, so your rules keep it silent.');
    expect(silent?.silent).toBe(true);
    expect(frontmostSummary(appState({ frontmostApp: null }, rule('never')))?.text).toBe(
      'TakTak can’t tell which app is in front right now.',
    );
  });

  it('is null where rules are unsupported', () => {
    expect(frontmostSummary(appState({ rulesSupported: false, frontmostApp: null }, rule('only')))).toBeNull();
  });
});
