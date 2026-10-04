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
  // No permission prompt to wait for (Windows, Linux, or the listener turned off).
  if (s.permission !== 'granted' && !s.onboarding.permissionRequired) {
    if (s.onboarding.inputGroupNeeded) return { label: 'Needs keyboard access', tone: 'warning' };
    if (s.onboarding.relaunchSuggested) return { label: 'Key listener stopped', tone: 'warning' };
    return { label: 'Key sounds unavailable', tone: 'off' };
  }
  if (s.permission === 'denied') return { label: `Needs ${permissionName(p)}`, tone: 'warning' };
  if (s.permission === 'unknown') return { label: `Waiting for ${permissionName(p)}`, tone: 'warning' };
  if (s.audio.state === 'fault') return { label: 'No sound output', tone: 'error' };
  if (s.ruleBlocked) return { label: silentIn(s), tone: 'off' };
  if (s.audio.state === 'starting') return { label: 'Starting audio…', tone: 'off' };
  return { label: 'Silent', tone: 'off' };
}

/** Where there is no key listener and nothing to grant (`TAKTAK_NO_INPUT=1`). */
export const UNAVAILABLE =
  'TakTak is running without its key listener, so typing makes no sound. You can still try the sound packs in Settings.';

/** Windows and Linux: the listener failed to start (`relaunchSuggested`, no permission step). */
export const RELAUNCH_OTHER =
  'TakTak’s key listener couldn’t start. Quitting and reopening TakTak usually fixes it; your settings are kept.';

/** Linux without the `input` group (`inputGroupNeeded`): why, and how. */
export const INPUT_GROUP_WHY =
  'On Wayland, apps can’t hear keys typed into other apps, so TakTak reads the keyboard devices directly. That needs your user in the input group: one command, then log out and back in.';

/** …and what it costs, said every time it is offered. */
export const INPUT_GROUP_COST =
  'Be aware that the input group lets every program you run read every keystroke, passwords included. TakTak only uses which key went down or up, and never asks for root.';

/** The command that joins the `input` group (takes effect at the next login). */
export const INPUT_GROUP_COMMAND = 'sudo usermod -aG input $USER';

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
    if (s.onboarding.inputGroupNeeded) {
      // Linux (Wayland, or TAKTAK_INPUT=evdev): the opt-in, with its cost; the guide has more.
      list.push({
        id: 'permission',
        tone: 'warning',
        title: 'Keyboard access needed',
        message: `${INPUT_GROUP_WHY} The command is “${INPUT_GROUP_COMMAND}”. ${INPUT_GROUP_COST}`,
        line: 'TakTak needs keyboard access (the input group) to play sounds.',
        actions: ['guide'],
      });
    } else if (s.onboarding.relaunchSuggested) {
      // Windows or Linux: the listener failed to start; there is no permission to grant.
      list.push({
        id: 'permission',
        tone: 'warning',
        title: 'TakTak needs to restart',
        message: RELAUNCH_OTHER,
        line: 'The key listener couldn’t start. Quit & Reopen usually fixes it.',
        actions: ['relaunch'],
      });
    } else {
      // No key listener and nothing that would bring one (as the onboarding's "unavailable").
      list.push({
        id: 'permission',
        tone: 'info',
        title: 'Key sounds are turned off',
        message: UNAVAILABLE,
        line: 'Key sounds are turned off.',
        actions: [],
      });
    }
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
