// What the current keyboard layout prints on each key position, for labelling hotkeys. It only
// matters on macOS, where a hotkey is a key position named after the US layout (see
// accelerator.ts). Two sources, in order:
// - the app (`key_labels`): it translates the fixed hotkey key positions with the current
//   input source (UCKeyTranslate) and sends `key-labels-changed` when the user switches layout;
// - the webview's Keyboard Map API (Chromium has it; WKWebView does not), read when the window
//   opens and again when it regains focus. In practice this is the browser during development.
// Without either, labels name the US keys and say so. The map describes the keyboard, not what
// anyone typed; it is never stored or sent.

import type { KeyLabels } from './accelerator';
import { getKeyLabels, onKeyLabelsChanged } from './api';
import { type Platform, platform } from './platform';

interface LayoutSource {
  getLayoutMap?: () => Promise<KeyLabels>;
}

/** The app's labels: `key_labels` and its change event. */
export interface AppLabelSource {
  get(): Promise<Record<string, string> | null>;
  onChange(listener: (labels: Record<string, string> | null) => void): Promise<() => void>;
}

const appSource: AppLabelSource = { get: getKeyLabels, onChange: onKeyLabelsChanged };

/** The longest label kept: a character or two (a few code units for emoji and the like). */
const MAX_LABEL = 8;

/**
 * The app's answer as a label map, keeping only string labels of reasonable length; null when
 * there is no answer or nothing usable in it.
 */
export function labelsFromRecord(record: unknown): KeyLabels | null {
  if (typeof record !== 'object' || record === null) return null;
  const labels = new Map<string, string>();
  for (const [code, label] of Object.entries(record)) {
    if (typeof label === 'string' && label.length > 0 && label.length <= MAX_LABEL) {
      labels.set(code, label);
    }
  }
  return labels.size > 0 ? labels : null;
}

let labels = $state.raw<KeyLabels | null>(null);

export const keyboardLayout = {
  /** The layout's character per `KeyboardEvent.code`, or null when nobody can tell. */
  get labels(): KeyLabels | null {
    return labels;
  },
};

let watching = false;

/** Reads the layout through the Keyboard Map API, and again whenever the window regains focus. */
function watchLayoutMap(keyboard: LayoutSource | undefined): void {
  const getLayoutMap = keyboard?.getLayoutMap;
  if (typeof getLayoutMap !== 'function') return;
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

/**
 * Starts reading the layout on macOS: from the app (`app`), else from the Keyboard Map API
 * (`keyboard`). Safe to call again. Pass `app: null` to skip the app.
 */
export function watchKeyboardLayout(
  p: Platform = platform,
  keyboard: LayoutSource | undefined = typeof navigator === 'undefined'
    ? undefined
    : (navigator as Navigator & { keyboard?: LayoutSource }).keyboard,
  app: AppLabelSource | null = appSource,
): void {
  if (watching || p !== 'mac') return;
  if (!app && typeof keyboard?.getLayoutMap !== 'function') return;
  watching = true;
  if (!app) {
    watchLayoutMap(keyboard);
    return;
  }
  let fromApp = false;
  void app
    .onChange((record) => {
      fromApp = true;
      labels = labelsFromRecord(record);
    })
    .catch(() => undefined);
  app.get().then(
    (record) => {
      const map = labelsFromRecord(record);
      if (map) {
        fromApp = true;
        labels = map;
      } else if (!fromApp) {
        watchLayoutMap(keyboard);
      }
    },
    () => watchLayoutMap(keyboard),
  );
}
