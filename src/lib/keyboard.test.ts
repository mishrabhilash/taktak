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
    watchKeyboardLayout('mac', keyboard);
    await flush();
    expect(keyboardLayout.labels?.get('KeyM')).toBe(',');
    layout = new Map([['KeyM', 'm']]);
    listeners.get('focus')?.();
    await flush();
    expect(keyboardLayout.labels?.get('KeyM')).toBe('m');
    // Watching starts once, however many components ask.
    watchKeyboardLayout('mac', keyboard);
    expect(keyboard.getLayoutMap).toHaveBeenCalledTimes(2);
  });

  it('forgets the layout when it can no longer be read', async () => {
    const { keyboardLayout, watchKeyboardLayout, listeners } = await load();
    watchKeyboardLayout('mac', keyboard);
    await flush();
    keyboard.getLayoutMap.mockImplementationOnce(() => Promise.reject(new Error('denied')));
    listeners.get('focus')?.();
    await flush();
    expect(keyboardLayout.labels).toBeNull();
  });

  it('does nothing on Windows and Linux, or without the Keyboard Map API', async () => {
    const { keyboardLayout, watchKeyboardLayout, listeners } = await load();
    watchKeyboardLayout('windows', keyboard);
    watchKeyboardLayout('linux', keyboard);
    watchKeyboardLayout('mac', undefined);
    watchKeyboardLayout('mac', {});
    await flush();
    expect(keyboard.getLayoutMap).not.toHaveBeenCalled();
    expect(listeners.size).toBe(0);
    expect(keyboardLayout.labels).toBeNull();
  });
});
