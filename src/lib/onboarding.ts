// What the onboarding window shows (docs/ui-contract.md § Onboarding, UI requirements): the
// permission step and its live status, when to lead with "Quit & Reopen", when to open the
// troubleshooting, and the closing button. Plain functions so every path can be tested.

import type { Permission } from './types';

/**
 * - `welcome`: no permission to grant here and TakTak can hear key presses: just the intro.
 * - `unavailable`: no permission to grant, but no key listener either (the listener turned off
 *   with `TAKTAK_NO_INPUT=1`), so typing makes no sound: say so instead of "all set".
 * - `inputGroup` (M5): Linux without the `input` group: explain the opt-in, its command and its
 *   security cost. Joining takes a log out, so nothing here waits for it.
 * - `ask`: Input Monitoring is missing and System Settings has not been opened from this window.
 * - `waiting`: missing, and the user went to System Settings; the 2 s poll flips it live.
 * - `relaunch`: the permission looks granted (or there is none to grant: Windows, Linux X11) but
 *   the listener can't start: "Quit & Reopen" leads.
 * - `granted`: TakTak can hear key presses.
 */
export type OnboardingPhase =
  | 'welcome'
  | 'unavailable'
  | 'inputGroup'
  | 'ask'
  | 'waiting'
  | 'relaunch'
  | 'granted';

export interface OnboardingFacts {
  permission: Permission;
  permissionRequired: boolean;
  relaunchSuggested: boolean;
  inputGroupNeeded: boolean;
  /** When the user last pressed "Open Input Monitoring Settings" in this window (ms), or null. */
  openedAt: number | null;
  now: number;
}

export interface OnboardingStep {
  phase: OnboardingPhase;
  /** Open the troubleshooting by itself (the user can always open it). */
  troubleshoot: boolean;
  /** Show the troubleshooting at all (never once TakTak can listen, or with no permission step). */
  troubleshootAvailable: boolean;
  /** The closing button: "Done" once there is nothing left to do, else "Later". */
  closeLabel: 'Done' | 'Later';
  /** The closing button is the main action. */
  closePrimary: boolean;
}

/** How long after opening System Settings the troubleshooting opens by itself. */
export const TROUBLESHOOT_AFTER_MS = 20_000;

export function onboardingPhase(f: OnboardingFacts): OnboardingPhase {
  // Without a permission step, `permission` is "granted" only while a key listener runs; it stays
  // "unknown" where there is none.
  if (!f.permissionRequired) {
    if (f.permission === 'granted') return 'welcome';
    if (f.inputGroupNeeded) return 'inputGroup';
    return f.relaunchSuggested ? 'relaunch' : 'unavailable';
  }
  if (f.permission === 'granted') return 'granted';
  if (f.relaunchSuggested) return 'relaunch';
  return f.openedAt === null ? 'ask' : 'waiting';
}

export function onboardingStep(f: OnboardingFacts): OnboardingStep {
  const phase = onboardingPhase(f);
  // Nothing left to do here: also the input group, which only a log out completes.
  const done =
    phase === 'welcome' || phase === 'unavailable' || phase === 'inputGroup' || phase === 'granted';
  const waitedLong =
    phase === 'waiting' && f.openedAt !== null && f.now - f.openedAt >= TROUBLESHOOT_AFTER_MS;
  return {
    phase,
    // The troubleshooting is about macOS's Input Monitoring list.
    troubleshoot: f.permissionRequired && (phase === 'relaunch' || waitedLong),
    troubleshootAvailable: !done && f.permissionRequired,
    closeLabel: done ? 'Done' : 'Later',
    closePrimary: done,
  };
}

/**
 * Milliseconds until the troubleshooting opens by itself (for a one-shot timer), or null when
 * nothing is pending: it is open already, or the user has not gone to System Settings yet.
 */
export function troubleshootDelay(f: OnboardingFacts): number | null {
  if (onboardingPhase(f) !== 'waiting' || f.openedAt === null) return null;
  const left = f.openedAt + TROUBLESHOOT_AFTER_MS - f.now;
  return left > 0 ? left : null;
}

/** Permission came through while the window was open: time for a short celebration. */
export function justGranted(previous: OnboardingPhase | null, next: OnboardingPhase): boolean {
  return (
    (previous === 'ask' || previous === 'waiting' || previous === 'relaunch') && next === 'granted'
  );
}

/** The Terminal command that clears a stale Input Monitoring entry. */
export const TCC_RESET_COMMAND = 'tccutil reset ListenEvent tech.taktak.app';
