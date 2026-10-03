// What to tell the user about playback: a one-line status and the notices that need action.

import { type Platform, permissionName } from './platform';
import type { AppState } from './types';

export type StatusTone = 'ok' | 'off' | 'warning' | 'error';

export interface Status {
  label: string;
  tone: StatusTone;
}

/** Why sounds are (not) playing, in a few words. */
export function playbackStatus(s: AppState, p: Platform): Status {
  if (s.playing) return { label: 'Playing', tone: 'ok' };
  if (!s.settings.enabled) return { label: 'Sounds off', tone: 'off' };
  if (s.muted) return { label: 'Muted', tone: 'off' };
  if (s.permission === 'denied') return { label: `Needs ${permissionName(p)}`, tone: 'warning' };
  if (s.permission === 'unknown') return { label: `Waiting for ${permissionName(p)}`, tone: 'warning' };
  if (s.audio.state === 'fault') return { label: 'No sound output', tone: 'error' };
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

export interface Notice {
  id: 'permission' | 'audio' | 'pack' | 'hotkey';
  tone: 'info' | 'warning' | 'error';
  title: string;
  message: string;
  /** The short form for the tray popover (one or two lines). */
  line: string;
  /** The notice offers the "grant permission" button. */
  permissionAction: boolean;
}

/** The label of the button that opens the permission settings. */
export function grantLabel(p: Platform): string {
  return p === 'mac' ? 'Grant Input Monitoring' : 'Open system settings';
}

/** Problems the user should know about, most important first. */
export function notices(s: AppState, p: Platform): Notice[] {
  const list: Notice[] = [];
  if (s.permission !== 'granted') {
    const name = permissionName(p);
    list.push({
      id: 'permission',
      tone: 'warning',
      title: s.permission === 'denied' ? `${name} is off` : `${name} not confirmed yet`,
      message:
        'TakTak needs it to notice when keys go down. It never sees what you type. Sounds start as soon as it is allowed.',
      line: `TakTak needs ${name} to play sounds.`,
      permissionAction: true,
    });
  }
  if (s.audio.state === 'fault') {
    const message = s.audio.message ?? 'The output device stopped working. Reconnecting…';
    list.push({
      id: 'audio',
      tone: 'error',
      title: 'No sound output',
      message,
      line: message,
      permissionAction: false,
    });
  } else if (s.audio.state === 'starting' && s.audio.message) {
    list.push({
      id: 'audio',
      tone: 'info',
      title: 'Starting audio…',
      message: s.audio.message,
      line: s.audio.message,
      permissionAction: false,
    });
  }
  if (s.activePackError) {
    list.push({
      id: 'pack',
      tone: 'warning',
      title: 'Pack not loaded',
      message: s.activePackError,
      line: s.activePackError,
      permissionAction: false,
    });
  }
  if (s.muteHotkeyError) {
    list.push({
      id: 'hotkey',
      tone: 'warning',
      title: 'Mute shortcut not working',
      message: `${s.muteHotkeyError} Record a different shortcut in Settings → Shortcuts.`,
      line: 'The mute shortcut could not be set up.',
      permissionAction: false,
    });
  }
  return list;
}
