import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { KeyLabels } from './accelerator';

/** Lets the layout map's promise settle. */
const flush = () => new Promise<void>((resolve) => setTimeout(resolve));

/** A fresh copy of the module (it starts watching only once) and the window's listeners. */
async function load() {
  vi.resetModules();
  const listeners = new Map<string, () => void>();
  vi.stubGlobal('window', {
    addEventListener: (type: string, listener: () => void) => listeners.set(type, listener),
  });
  const module = await import('./keyboard.svelte');
  return { ...module, listeners };
}

describe('watchKeyboardLayout', () => {
  let layout: KeyLabels;
  const keyboard = { getLayoutMap: vi.fn(() => Promise.resolve(layout)) };

  beforeEach(() => {
    layout = new Map([['KeyM', ',']]);
    keyboard.getLayoutMap.mockClear();
  });

  it('reads the layout on macOS, and again when the window regains focus', async () => {
    const { keyboardLayout, watchKeyboardLayout, listeners } = await load();
    expect(keyboardLayout.labels).toBeNull();
    watchKeyboardLayout('mac', keyboard, null);
    await flush();
    expect(keyboardLayout.labels?.get('KeyM')).toBe(',');
    layout = new Map([['KeyM', 'm']]);
    listeners.get('focus')?.();
    await flush();
    expect(keyboardLayout.labels?.get('KeyM')).toBe('m');
    // Watching starts once, however many components ask.
    watchKeyboardLayout('mac', keyboard, null);
    expect(keyboard.getLayoutMap).toHaveBeenCalledTimes(2);
  });

  it('forgets the layout when it can no longer be read', async () => {
    const { keyboardLayout, watchKeyboardLayout, listeners } = await load();
    watchKeyboardLayout('mac', keyboard, null);
    await flush();
    keyboard.getLayoutMap.mockImplementationOnce(() => Promise.reject(new Error('denied')));
    listeners.get('focus')?.();
    await flush();
    expect(keyboardLayout.labels).toBeNull();
  });

  it('does nothing on Windows and Linux, or without the Keyboard Map API', async () => {
    const { keyboardLayout, watchKeyboardLayout, listeners } = await load();
    watchKeyboardLayout('windows', keyboard, null);
    watchKeyboardLayout('linux', keyboard, null);
    watchKeyboardLayout('mac', undefined, null);
    watchKeyboardLayout('mac', {}, null);
    await flush();
    expect(keyboard.getLayoutMap).not.toHaveBeenCalled();
    expect(listeners.size).toBe(0);
    expect(keyboardLayout.labels).toBeNull();
  });
});

/** A fake app: `key_labels` answers `answer`; `emit` sends `key-labels-changed`. */
function fakeApp(answer: Record<string, string> | null | Error) {
  let listener: ((labels: Record<string, string> | null) => void) | undefined;
  return {
    source: {
      get: vi.fn(() =>
        answer instanceof Error ? Promise.reject(answer) : Promise.resolve(answer),
      ),
      onChange: vi.fn(async (l: (labels: Record<string, string> | null) => void) => {
        listener = l;
        return () => {};
      }),
    },
    emit: (labels: Record<string, string> | null) => listener?.(labels),
  };
}

describe('labels from the app (macOS)', () => {
  const keyboard = { getLayoutMap: vi.fn(() => Promise.resolve(new Map([['KeyM', 'x']]))) };

  beforeEach(() => keyboard.getLayoutMap.mockClear());

  it('uses the app’s labels and follows layout switches', async () => {
    const { keyboardLayout, watchKeyboardLayout } = await load();
    const app = fakeApp({ KeyM: ',', KeyQ: 'a' });
    watchKeyboardLayout('mac', keyboard, app.source);
    await flush();
    expect(keyboardLayout.labels?.get('KeyM')).toBe(',');
    expect(keyboard.getLayoutMap).not.toHaveBeenCalled();
    app.emit({ KeyM: 'm' });
    expect(keyboardLayout.labels?.get('KeyM')).toBe('m');
    // A layout the app cannot read: back to US names.
    app.emit(null);
    expect(keyboardLayout.labels).toBeNull();
  });

  it('falls back to the Keyboard Map API when the app cannot tell', async () => {
    const { keyboardLayout, watchKeyboardLayout } = await load();
    watchKeyboardLayout('mac', keyboard, fakeApp(null).source);
    await flush();
    await flush();
    expect(keyboardLayout.labels?.get('KeyM')).toBe('x');
    const again = await load();
    again.watchKeyboardLayout('mac', undefined, fakeApp(new Error('no backend')).source);
    await flush();
    expect(again.keyboardLayout.labels).toBeNull();
  });

  it('asks nothing on Windows and Linux', async () => {
    const { keyboardLayout, watchKeyboardLayout } = await load();
    const app = fakeApp({ KeyM: ',' });
    watchKeyboardLayout('windows', keyboard, app.source);
    watchKeyboardLayout('linux', keyboard, app.source);
    await flush();
    expect(app.source.get).not.toHaveBeenCalled();
    expect(keyboardLayout.labels).toBeNull();
  });

  it('keeps only usable labels', async () => {
    const { labelsFromRecord } = await load();
    expect(labelsFromRecord(null)).toBeNull();
    expect(labelsFromRecord({})).toBeNull();
    expect(labelsFromRecord('KeyM')).toBeNull();
    const labels = labelsFromRecord({ KeyM: ',', KeyA: '', KeyB: 7, KeyC: 'far too long' });
    expect(labels && [...labels]).toEqual([['KeyM', ',']]);
  });
});
