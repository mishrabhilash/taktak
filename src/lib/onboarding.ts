// What the onboarding window shows (docs/ui-contract.md § Onboarding, UI requirements): one line,
// one main button, a live status, and two small disclosures ("Why?", "Having trouble?") for
// everything else. Plain functions so every path can be tested.

import type { Permission } from './types';

/**
 * - `welcome`: no permission to grant here and TakTak can hear key presses: just the intro.
 * - `unavailable`: no permission to grant, but no key listener either (the listener turned off
 *   with `TAKTAK_NO_INPUT=1`), so typing makes no sound: say so instead of "all set".
 * - `inputGroup` (M5): Linux without the `input` group: the opt-in, its command and its
 *   security cost. Joining takes a log out, so nothing here waits for it.
 * - `ask`: Input Monitoring is missing and the user has not pressed "Allow Input Monitoring".
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
  /** When the user last pressed "Allow Input Monitoring" in this window (ms), or null. */
  openedAt: number | null;
  /**
   * What that press answered: whether TakTak is in the Input Monitoring list (false: macOS added
   * nothing, as after `tccutil reset`; the user adds it with +). null before the answer.
   */
  listed: boolean | null;
  now: number;
}

/** The window's one big button. */
export type OnboardingAction = 'allow' | 'reopen' | 'done';

export interface OnboardingStep {
  phase: OnboardingPhase;
  action: OnboardingAction;
  /** Show the small "Later" link (closes the window) next to the disclosures. */
  later: boolean;
  /** The "Why?" disclosure exists (there is something to explain: a permission or the group). */
  whyAvailable: boolean;
  /** The "Having trouble?" disclosure exists (macOS, while TakTak can't listen yet). */
  troubleshootAvailable: boolean;
  /** Open "Having trouble?" by itself. */
  troubleshoot: boolean;
  /** TakTak is not in the Input Monitoring list: the status says how to add it. */
  notListed: boolean;
}

/** The live status line under the button, or none. */
export interface OnboardingStatus {
  tone: 'ok' | 'waiting' | 'idle' | 'warn' | 'info';
  text: string;
}

/** How long after opening System Settings "Having trouble?" opens by itself. */
export const TROUBLESHOOT_AFTER_MS = 20_000;

/** The words, kept few. */
export const COPY = {
  title: 'Welcome to TakTak',
  lineMac: 'TakTak needs Input Monitoring to hear when keys go down. It never sees what you type.',
  lineOther: 'Mechanical keyboard sounds as you type, in every app.',
  lineInputGroup: 'On Wayland, TakTak needs your user in the input group to hear your keys.',
  allow: 'Allow Input Monitoring',
  reopen: 'Quit & Reopen',
  reopening: 'Reopening…',
  done: 'Done',
  later: 'Later',
  why: 'Why?',
  trouble: 'Having trouble?',
  allSet: 'You’re all set — start typing',
  ask: 'Then switch on TakTak in the list.',
  waiting: 'Waiting for you to switch on TakTak…',
  notListed: 'TakTak isn’t in the list? Click + below it and choose TakTak.',
  relaunchMac: 'Almost there — TakTak needs a quick restart.',
  relaunchOther: 'TakTak’s key listener couldn’t start. A restart usually fixes it.',
  unavailable: 'Key sounds are turned off.',
} as const;

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
  const action: OnboardingAction =
    phase === 'ask' || phase === 'waiting' ? 'allow' : phase === 'relaunch' ? 'reopen' : 'done';
  const notListed = phase === 'waiting' && f.listed === false;
  const waitedLong =
    phase === 'waiting' && f.openedAt !== null && f.now - f.openedAt >= TROUBLESHOOT_AFTER_MS;
  // The troubleshooting is about macOS's Input Monitoring list.
  const troubleshootAvailable = f.permissionRequired && action !== 'done';
  return {
    phase,
    action,
    later: action !== 'done',
    whyAvailable: f.permissionRequired || phase === 'inputGroup',
    troubleshootAvailable,
    troubleshoot: troubleshootAvailable && (phase === 'relaunch' || waitedLong || notListed),
    notListed,
  };
}

/** The window's one line under the title. */
export function onboardingLine(f: OnboardingFacts): string {
  if (f.permissionRequired) return COPY.lineMac;
  return onboardingPhase(f) === 'inputGroup' ? COPY.lineInputGroup : COPY.lineOther;
}

export function onboardingStatus(f: OnboardingFacts): OnboardingStatus | null {
  const step = onboardingStep(f);
  switch (step.phase) {
    case 'welcome':
    case 'granted':
      return { tone: 'ok', text: COPY.allSet };
    case 'unavailable':
      return { tone: 'info', text: COPY.unavailable };
    case 'ask':
      return { tone: 'idle', text: COPY.ask };
    case 'waiting':
      return step.notListed
        ? { tone: 'warn', text: COPY.notListed }
        : { tone: 'waiting', text: COPY.waiting };
    case 'relaunch':
      return {
        tone: 'warn',
        text: f.permissionRequired ? COPY.relaunchMac : COPY.relaunchOther,
      };
    case 'inputGroup':
      return null;
  }
}

/**
 * Milliseconds until "Having trouble?" opens by itself (for a one-shot timer), or null when
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
