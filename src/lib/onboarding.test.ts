import { describe, expect, it } from 'vitest';
import {
  type OnboardingFacts,
  TCC_RESET_COMMAND,
  TROUBLESHOOT_AFTER_MS,
  justGranted,
  onboardingPhase,
  onboardingStep,
  troubleshootDelay,
} from './onboarding';

function facts(patch: Partial<OnboardingFacts> = {}): OnboardingFacts {
  return {
    permission: 'denied',
    permissionRequired: true,
    relaunchSuggested: false,
    openedAt: null,
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

  it('never says "all set" where there is no key listener (Windows, Linux, listener off)', () => {
    expect(onboardingPhase(facts({ permissionRequired: false, permission: 'unknown' }))).toBe(
      'unavailable',
    );
    expect(onboardingPhase(facts({ permissionRequired: false }))).toBe('unavailable');
    expect(
      onboardingPhase(facts({ permissionRequired: false, permission: 'unknown', openedAt: 1 })),
    ).toBe('unavailable');
  });
});

describe('onboardingStep', () => {
  it('"Later" while permission is missing, "Done" (main action) once there is nothing to do', () => {
    expect(onboardingStep(facts())).toMatchObject({ closeLabel: 'Later', closePrimary: false });
    expect(onboardingStep(facts({ relaunchSuggested: true })).closeLabel).toBe('Later');
    expect(onboardingStep(facts({ permission: 'granted' }))).toMatchObject({
      closeLabel: 'Done',
      closePrimary: true,
    });
    expect(onboardingStep(facts({ permissionRequired: false }))).toMatchObject({
      phase: 'unavailable',
      closeLabel: 'Done',
      closePrimary: true,
      troubleshootAvailable: false,
    });
    const listening = onboardingStep(facts({ permissionRequired: false, permission: 'granted' }));
    expect(listening).toMatchObject({ phase: 'welcome', closeLabel: 'Done', closePrimary: true });
  });

  it('opens the troubleshooting with the relaunch, or 20 s after opening System Settings', () => {
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
    // Granted meanwhile: nothing to troubleshoot.
    const granted = onboardingStep(
      facts({ openedAt: opened, now: opened + 60_000, permission: 'granted' }),
    );
    expect(granted.troubleshoot).toBe(false);
    expect(granted.troubleshootAvailable).toBe(false);
  });

  it('offers troubleshooting only while there is something to fix', () => {
    expect(onboardingStep(facts()).troubleshootAvailable).toBe(true);
    expect(onboardingStep(facts({ relaunchSuggested: true })).troubleshootAvailable).toBe(true);
    expect(onboardingStep(facts({ permissionRequired: false })).troubleshootAvailable).toBe(false);
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
