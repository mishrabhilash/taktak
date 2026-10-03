// The current keyboard layout's character for each key position, where the webview can tell
// (the Keyboard Map API: Chromium and WebView2 have it, WKWebView and WebKitGTK do not). It only
// labels shortcut keys on macOS, where a hotkey is a key position (see accelerator.ts), so it is
// read on macOS only: when the window opens, and again when it regains focus, in case the user
// switched layouts. It describes the keyboard, not what anyone typed; it is never stored or sent.

import type { KeyLabels } from './accelerator';
import { type Platform, platform } from './platform';

interface LayoutSource {
  getLayoutMap?: () => Promise<KeyLabels>;
}

let labels = $state.raw<KeyLabels | null>(null);

export const keyboardLayout = {
  /** The layout's character per `KeyboardEvent.code`, or null when the webview cannot tell. */
  get labels(): KeyLabels | null {
    return labels;
  },
};

let watching = false;

/** Starts reading the layout (macOS with the Keyboard Map API only). Safe to call again. */
export function watchKeyboardLayout(
  p: Platform = platform,
  keyboard: LayoutSource | undefined = typeof navigator === 'undefined'
    ? undefined
    : (navigator as Navigator & { keyboard?: LayoutSource }).keyboard,
): void {
  const getLayoutMap = keyboard?.getLayoutMap;
  if (watching || p !== 'mac' || typeof getLayoutMap !== 'function') return;
  watching = true;
  const read = (): void => {
    getLayoutMap.call(keyboard).then(
      (map) => {
        labels = new Map(map);
      },
      () => {
        labels = null;
      },
    );
  };
  read();
  window.addEventListener('focus', read);
}
