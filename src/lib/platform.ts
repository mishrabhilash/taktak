// Which desktop the UI runs on. It only changes wording and shortcut symbols (⌘ vs Ctrl).

export type Platform = 'mac' | 'windows' | 'linux';

/**
 * The platform from Tauri's build-time `TAURI_ENV_PLATFORM` ("darwin", "windows", "linux"…),
 * else from the user agent (plain browser during development).
 */
export function detectPlatform(tauriPlatform?: string, userAgent = ''): Platform {
  const name = (tauriPlatform || userAgent).toLowerCase();
  if (/darwin|mac|iphone|ipad|ios/.test(name)) return 'mac';
  if (/win/.test(name)) return 'windows';
  return 'linux';
}

/** The platform this window runs on. */
export const platform: Platform = detectPlatform(
  import.meta.env.TAURI_ENV_PLATFORM,
  typeof navigator === 'undefined' ? '' : navigator.userAgent,
);

/** What the OS calls the permission TakTak needs to hear key presses. */
export function permissionName(p: Platform): string {
  return p === 'mac' ? 'Input Monitoring' : 'Keyboard access';
}
