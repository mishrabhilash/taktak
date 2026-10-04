import { describe, expect, it } from 'vitest';
import {
  COPY,
  type OnboardingFacts,
  TCC_RESET_COMMAND,
  TROUBLESHOOT_AFTER_MS,
  justGranted,
  onboardingLine,
  onboardingPhase,
  onboardingStatus,
  onboardingStep,
  troubleshootDelay,
} from './onboarding';

function facts(patch: Partial<OnboardingFacts> = {}): OnboardingFacts {
  return {
    permission: 'denied',
    permissionRequired: true,
    relaunchSuggested: false,
    inputGroupNeeded: false,
    openedAt: null,
    listed: null,
    now: 100_000,
    ...patch,
  };
}

describe('onboardingPhase', () => {
  it('walks ask → waiting → granted', () => {
    expect(onboardingPhase(facts())).toBe('ask');
    expect(onboardingPhase(facts({ permission: 'unknown' }))).toBe('ask');
    expect(onboardingPhase(facts({ openedAt: 99_000 }))).toBe('waiting');
    expect(onboardingPhase(facts({ openedAt: 99_000, permission: 'granted' }))).toBe('granted');
    expect(onboardingPhase(facts({ permission: 'granted' }))).toBe('granted');
  });

  it('leads with the relaunch when macOS says granted but the listener is refused', () => {
    expect(onboardingPhase(facts({ relaunchSuggested: true }))).toBe('relaunch');
    expect(onboardingPhase(facts({ relaunchSuggested: true, openedAt: 1 }))).toBe('relaunch');
  });

  it('is just a welcome where no permission is required and a key listener runs', () => {
    expect(onboardingPhase(facts({ permissionRequired: false, permission: 'granted' }))).toBe(
      'welcome',
    );
  });

  it('explains the input group on Linux, and offers a restart where the listener failed (M5)', () => {
    const linux = facts({ permissionRequired: false, inputGroupNeeded: true });
    expect(onboardingPhase(linux)).toBe('inputGroup');
    expect(onboardingStep(linux)).toMatchObject({
      action: 'done',
      later: false,
      whyAvailable: true,
      troubleshoot: false,
      troubleshootAvailable: false,
    });
    const failed = facts({ permissionRequired: false, relaunchSuggested: true });
    expect(onboardingPhase(failed)).toBe('relaunch');
    expect(onboardingStep(failed)).toMatchObject({
      action: 'reopen',
      later: true,
      whyAvailable: false,
      troubleshoot: false,
      troubleshootAvailable: false,
    });
    // Listening after all: just the welcome.
    expect(onboardingPhase({ ...linux, permission: 'granted' })).toBe('welcome');
  });

  it('never says "all set" where there is no key listener (listener off)', () => {
    expect(onboardingPhase(facts({ permissionRequired: false, permission: 'unknown' }))).toBe(
      'unavailable',
    );
    expect(onboardingPhase(facts({ permissionRequired: false }))).toBe('unavailable');
    expect(
      onboardingPhase(facts({ permissionRequired: false, permission: 'unknown', openedAt: 1 })),
    ).toBe('unavailable');
    expect(onboardingStatus(facts({ permissionRequired: false }))?.text).not.toBe(COPY.allSet);
  });
});

describe('onboardingStep', () => {
  it('one big button: Allow, then Done; Quit & Reopen when macOS wants a restart', () => {
    expect(onboardingStep(facts())).toMatchObject({ action: 'allow', later: true });
    expect(onboardingStep(facts({ openedAt: 1 }))).toMatchObject({ action: 'allow', later: true });
    expect(onboardingStep(facts({ relaunchSuggested: true }))).toMatchObject({
      action: 'reopen',
      later: true,
    });
    expect(onboardingStep(facts({ permission: 'granted' }))).toMatchObject({
      action: 'done',
      later: false,
    });
    expect(onboardingStep(facts({ permissionRequired: false }))).toMatchObject({
      phase: 'unavailable',
      action: 'done',
      later: false,
      troubleshootAvailable: false,
      whyAvailable: false,
    });
    const listening = onboardingStep(facts({ permissionRequired: false, permission: 'granted' }));
    expect(listening).toMatchObject({ phase: 'welcome', action: 'done', whyAvailable: false });
  });

  it('reopened later with permission granted: only the success state, nothing to ask', () => {
    // Opened again from Settings → About or the tray: no "Allow", no "Later", no troubleshooting.
    const reopened = facts({ permission: 'granted' });
    expect(onboardingStep(reopened)).toMatchObject({
      phase: 'granted',
      action: 'done',
      later: false,
      troubleshootAvailable: false,
      troubleshoot: false,
    });
    expect(onboardingStatus(reopened)).toEqual({ tone: 'ok', text: COPY.allSet });
    expect(COPY.allSet).toBe('You’re all set — start typing');
    expect(COPY.done).toBe('Done');
    expect(troubleshootDelay(reopened)).toBeNull();
    // Already granted when the window opens: no celebration either.
    expect(justGranted(null, 'granted')).toBe(false);
  });

  it('keeps "Why?" for the permission, collapsed until asked', () => {
    expect(onboardingStep(facts()).whyAvailable).toBe(true);
    expect(onboardingStep(facts({ permission: 'granted' })).whyAvailable).toBe(true);
  });

  it('opens "Having trouble?" with the relaunch, 20 s after opening System Settings, or when TakTak is not listed', () => {
    expect(onboardingStep(facts()).troubleshoot).toBe(false);
    expect(onboardingStep(facts({ relaunchSuggested: true })).troubleshoot).toBe(true);
    const opened = 50_000;
    expect(onboardingStep(facts({ openedAt: opened, now: opened })).troubleshoot).toBe(false);
    expect(
      onboardingStep(facts({ openedAt: opened, now: opened + TROUBLESHOOT_AFTER_MS - 1 }))
        .troubleshoot,
    ).toBe(false);
    expect(
      onboardingStep(facts({ openedAt: opened, now: opened + TROUBLESHOOT_AFTER_MS })).troubleshoot,
    ).toBe(true);
    // macOS added nothing to the list: straight away.
    const unlisted = onboardingStep(facts({ openedAt: opened, now: opened, listed: false }));
    expect(unlisted).toMatchObject({ troubleshoot: true, notListed: true });
    expect(
      onboardingStep(facts({ openedAt: opened, now: opened, listed: true })).troubleshoot,
    ).toBe(false);
    // Granted meanwhile: nothing to troubleshoot.
    const granted = onboardingStep(
      facts({ openedAt: opened, now: opened + 60_000, permission: 'granted', listed: false }),
    );
    expect(granted).toMatchObject({
      troubleshoot: false,
      troubleshootAvailable: false,
      notListed: false,
    });
  });

  it('offers troubleshooting only while there is something to fix', () => {
    expect(onboardingStep(facts()).troubleshootAvailable).toBe(true);
    expect(onboardingStep(facts({ relaunchSuggested: true })).troubleshootAvailable).toBe(true);
    expect(onboardingStep(facts({ permissionRequired: false })).troubleshootAvailable).toBe(false);
  });
});

describe('onboardingStatus', () => {
  it('turns into "You’re all set" live, with no restart', () => {
    expect(onboardingStatus(facts())).toEqual({ tone: 'idle', text: COPY.ask });
    expect(onboardingStatus(facts({ openedAt: 1 }))).toEqual({
      tone: 'waiting',
      text: COPY.waiting,
    });
    expect(onboardingStatus(facts({ openedAt: 1, permission: 'granted' }))).toEqual({
      tone: 'ok',
      text: COPY.allSet,
    });
    expect(
      onboardingStatus(facts({ permissionRequired: false, permission: 'granted' }))?.text,
    ).toBe(COPY.allSet);
  });

  it('says how to add TakTak when macOS did not list it', () => {
    expect(onboardingStatus(facts({ openedAt: 1, listed: false }))).toEqual({
      tone: 'warn',
      text: COPY.notListed,
    });
    expect(COPY.notListed).toMatch(/\+/);
  });

  it('asks for a restart in words that fit the platform', () => {
    expect(onboardingStatus(facts({ relaunchSuggested: true }))?.text).toBe(COPY.relaunchMac);
    const other = onboardingStatus(facts({ permissionRequired: false, relaunchSuggested: true }));
    expect(other?.text).toBe(COPY.relaunchOther);
    expect(other?.text).not.toMatch(/macOS|Input Monitoring/);
    expect(onboardingStatus(facts({ permissionRequired: false, inputGroupNeeded: true }))).toBe(
      null,
    );
  });
});

describe('onboardingLine', () => {
  it('is one short line, and only macOS mentions Input Monitoring', () => {
    expect(onboardingLine(facts())).toBe(
      'TakTak needs Input Monitoring to hear when keys go down. It never sees what you type.',
    );
    expect(onboardingLine(facts({ permission: 'granted' }))).toBe(COPY.lineMac);
    const others = [
      facts({ permissionRequired: false, permission: 'granted' }),
      facts({ permissionRequired: false, relaunchSuggested: true }),
      facts({ permissionRequired: false, inputGroupNeeded: true }),
    ].map(onboardingLine);
    for (const line of others) expect(line).not.toMatch(/Input Monitoring|macOS/);
    expect(others[2]).toBe(COPY.lineInputGroup);
  });

  it('keeps every line and status short', () => {
    for (const text of Object.values(COPY)) expect(text.length).toBeLessThanOrEqual(90);
  });
});

describe('troubleshootDelay', () => {
  it('counts down only while waiting, for a one-shot timer', () => {
    expect(troubleshootDelay(facts())).toBeNull();
    expect(troubleshootDelay(facts({ openedAt: 100_000 }))).toBe(TROUBLESHOOT_AFTER_MS);
    expect(troubleshootDelay(facts({ openedAt: 95_000 }))).toBe(TROUBLESHOOT_AFTER_MS - 5_000);
    expect(troubleshootDelay(facts({ openedAt: 100_000 - TROUBLESHOOT_AFTER_MS }))).toBeNull();
    expect(troubleshootDelay(facts({ openedAt: 100_000, permission: 'granted' }))).toBeNull();
    expect(troubleshootDelay(facts({ openedAt: 100_000, relaunchSuggested: true }))).toBeNull();
  });
});

describe('justGranted', () => {
  it('celebrates only a change to granted seen while the window is open', () => {
    expect(justGranted('waiting', 'granted')).toBe(true);
    expect(justGranted('ask', 'granted')).toBe(true);
    expect(justGranted('relaunch', 'granted')).toBe(true);
    expect(justGranted(null, 'granted')).toBe(false);
    expect(justGranted('granted', 'granted')).toBe(false);
    expect(justGranted('welcome', 'granted')).toBe(false);
    expect(justGranted('unavailable', 'granted')).toBe(false);
    expect(justGranted('ask', 'waiting')).toBe(false);
  });
});

it('the reset command names TakTak’s Input Monitoring entry only', () => {
  expect(TCC_RESET_COMMAND).toBe('tccutil reset ListenEvent tech.taktak.app');
});
