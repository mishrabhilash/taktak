// What to tell the user about playback: a one-line status and the notices that need action.

import { type Platform, permissionName } from './platform';
import type { AppState } from './types';

export type StatusTone = 'ok' | 'off' | 'warning' | 'error';

export interface Status {
  label: string;
  tone: StatusTone;
}

/**
 * The mute the switches (popover) show and toggle, like the tray's Mute item: the manual mute,
 * or an "outputChanged" auto-mute. Turning it off sends `set_muted(false)`, which clears both.
 */
export function effectiveMuted(s: AppState): boolean {
  return s.muted || s.autoMute === 'outputChanged';
}

/** "Silent in Slack", or "Silent in this app" when TakTak can't tell which app is in front. */
export function silentIn(s: AppState): string {
  return `Silent in ${s.frontmostApp?.name ?? 'this app'}`;
}

/**
 * Why sounds are (not) playing, in a few words. First match wins, in the contract's order:
 * off → muted by hand → screen locked → output changed → no key listener → permission → audio
 * fault → rules → audio starting.
 */
export function playbackStatus(s: AppState, p: Platform): Status {
  if (s.playing) return { label: 'Playing', tone: 'ok' };
  if (!s.settings.enabled) return { label: 'Sounds off', tone: 'off' };
  if (s.muted) return { label: 'Muted', tone: 'off' };
  if (s.autoMute === 'screenLocked') return { label: 'Muted — screen locked', tone: 'off' };
  if (s.autoMute === 'outputChanged') return { label: 'Muted — output device changed', tone: 'off' };
  // No permission step (Windows and Linux for now, or the listener turned off): nothing to wait for.
  if (s.permission !== 'granted' && !s.onboarding.permissionRequired) {
    return { label: 'Key sounds unavailable', tone: 'off' };
  }
  if (s.permission === 'denied') return { label: `Needs ${permissionName(p)}`, tone: 'warning' };
  if (s.permission === 'unknown') return { label: `Waiting for ${permissionName(p)}`, tone: 'warning' };
  if (s.audio.state === 'fault') return { label: 'No sound output', tone: 'error' };
  if (s.ruleBlocked) return { label: silentIn(s), tone: 'off' };
  if (s.audio.state === 'starting') return { label: 'Starting audio…', tone: 'off' };
  return { label: 'Silent', tone: 'off' };
}

/** What plays when no pack is loaded. */
export const BUILT_IN_SOUND = 'Built-in click';

/**
 * The name of what plays (or would play, unmuted): the pack in `playingPackId`, which is a
 * fallback or the selected pack's last working version whenever `activePackError` is set. A
 * pack that broke on disk keeps playing although it left the list; it is named by its id then.
 */
export function playingName(s: AppState): string {
  const id = s.playingPackId;
  if (id === null) return BUILT_IN_SOUND;
  return s.packs.find((p) => p.id === id)?.name ?? id;
}

/**
 * A notice's buttons:
 * - `permission` opens System Settings → Input Monitoring;
 * - `guide` opens the onboarding window (the full permission guide);
 * - `relaunch` quits and reopens TakTak;
 * - `unmute` clears the manual mute and an "outputChanged" auto-mute;
 * - `rules` shows the per-app rules in Settings.
 */
export type NoticeAction = 'permission' | 'guide' | 'relaunch' | 'unmute' | 'rules';

export interface Notice {
  id: 'automute' | 'permission' | 'audio' | 'pack' | 'hotkey' | 'rules';
  tone: 'info' | 'warning' | 'error';
  title: string;
  message: string;
  /** The short form for the tray popover (one or two lines). */
  line: string;
  /** Its buttons, the main one first. */
  actions: NoticeAction[];
}

/** The label of the button that opens the permission settings. */
export function grantLabel(p: Platform): string {
  return p === 'mac' ? 'Grant Input Monitoring' : 'Open system settings';
}

/** A notice button's label. */
export function actionLabel(action: NoticeAction, p: Platform): string {
  switch (action) {
    case 'permission':
      return grantLabel(p);
    case 'guide':
      return 'Help';
    case 'relaunch':
      return 'Quit & Reopen';
    case 'unmute':
      return 'Unmute';
    case 'rules':
      return 'Edit rules…';
  }
}

/** Problems the user should know about, most important first. */
export function notices(s: AppState, p: Platform): Notice[] {
  const list: Notice[] = [];
  // Reasons that only matter while sounds are on and not muted by hand.
  const wanted = s.settings.enabled && !s.muted;
  if (wanted && s.autoMute === 'screenLocked') {
    list.push({
      id: 'automute',
      tone: 'info',
      title: 'Muted — screen locked',
      message: 'TakTak is always silent while the screen is locked. Sounds come back when you unlock.',
      line: 'Silent while the screen is locked.',
      actions: [],
    });
  } else if (wanted && s.autoMute === 'outputChanged') {
    const device = s.audio.device;
    list.push({
      id: 'automute',
      tone: 'info',
      title: 'Muted — output device changed',
      message: device
        ? `TakTak muted itself because the sound output changed to ${device}. Unmute to keep typing sounds on this device.`
        : 'TakTak muted itself because the sound output changed. Unmute to keep typing sounds on.',
      line: device ? `Muted: the output changed to ${device}.` : 'Muted: the sound output changed.',
      actions: ['unmute'],
    });
  }
  if (s.permission !== 'granted' && !s.onboarding.permissionRequired) {
    // No key listener and no permission that would bring one (as the onboarding's "unavailable").
    const message =
      p === 'mac'
        ? 'TakTak was started without its key listener, so typing makes no sound.'
        : `TakTak can’t hear key presses on ${p === 'windows' ? 'Windows' : 'Linux'} yet, so typing makes no sound. You can still try the sound packs in Settings.`;
    list.push({
      id: 'permission',
      tone: 'info',
      title: p === 'mac' ? 'Key sounds are turned off' : 'Key sounds aren’t available on this system yet',
      message,
      line: p === 'mac' ? 'Key sounds are turned off.' : 'Key sounds aren’t available on this system yet.',
      actions: [],
    });
  } else if (s.permission !== 'granted') {
    const name = permissionName(p);
    const guide: NoticeAction[] = s.onboarding.permissionRequired ? ['guide'] : [];
    if (s.onboarding.relaunchSuggested) {
      list.push({
        id: 'permission',
        tone: 'warning',
        title: 'TakTak needs to restart',
        message: `macOS needs TakTak to restart before it can listen. ${name} is allowed; your settings are kept.`,
        line: 'macOS needs TakTak to restart before it can listen.',
        actions: ['relaunch', ...guide],
      });
    } else {
      list.push({
        id: 'permission',
        tone: 'warning',
        title: s.permission === 'denied' ? `${name} is off` : `${name} not confirmed yet`,
        message:
          'TakTak needs it to notice when keys go down. It never sees what you type. Sounds start as soon as it is allowed.',
        line: `TakTak needs ${name} to play sounds.`,
        actions: ['permission', ...guide],
      });
    }
  }
  if (s.audio.state === 'fault') {
    const message = s.audio.message ?? 'The output device stopped working. Reconnecting…';
    list.push({
      id: 'audio',
      tone: 'error',
      title: 'No sound output',
      message,
      line: message,
      actions: [],
    });
  } else if (s.audio.state === 'starting' && s.audio.message) {
    list.push({
      id: 'audio',
      tone: 'info',
      title: 'Starting audio…',
      message: s.audio.message,
      line: s.audio.message,
      actions: [],
    });
  }
  if (s.activePackError) {
    list.push({
      id: 'pack',
      tone: 'warning',
      title: 'Pack not loaded',
      message: s.activePackError,
      line: s.activePackError,
      actions: [],
    });
  }
  if (s.muteHotkeyError) {
    list.push({
      id: 'hotkey',
      tone: 'warning',
      title: 'Mute shortcut not working',
      message: `${s.muteHotkeyError} Record a different shortcut in Settings → Shortcuts.`,
      line: 'The mute shortcut could not be set up.',
      actions: [],
    });
  }
  // Only when the rules are what keeps TakTak silent (the status line's order).
  if (
    wanted &&
    s.ruleBlocked &&
    s.autoMute === null &&
    s.permission === 'granted' &&
    s.audio.state !== 'fault'
  ) {
    const app = s.frontmostApp?.name;
    list.push({
      id: 'rules',
      tone: 'info',
      title: silentIn(s),
      message: app
        ? `Your per-app rules keep TakTak silent while ${app} is in front. Sounds come back in the apps your rules allow.`
        : 'TakTak can’t tell which app is in front, and your per-app rules only allow sounds in the apps you listed.',
      line: `${silentIn(s)} because of your per-app rules.`,
      actions: ['rules'],
    });
  }
  return list;
}
