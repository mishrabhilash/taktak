import { describe, expect, it } from 'vitest';
import { NOTE_TTL_MS, type NoteStore, requestSection, takeRequestedSection } from './section';

function memory(): NoteStore & { data: Map<string, string> } {
  const data = new Map<string, string>();
  return {
    data,
    getItem: (k) => data.get(k) ?? null,
    setItem: (k, v) => void data.set(k, v),
    removeItem: (k) => void data.delete(k),
  };
}

describe('section requests', () => {
  it('hands a section over once', () => {
    const store = memory();
    requestSection('apps', store, 1000);
    expect(takeRequestedSection(store, 1500)).toBe('apps');
    expect(takeRequestedSection(store, 1600)).toBeNull();
    expect(store.data.size).toBe(0);
  });

  it('ignores a stale or broken note, and removes it', () => {
    const store = memory();
    requestSection('apps', store, 1000);
    expect(takeRequestedSection(store, 1000 + NOTE_TTL_MS)).toBeNull();
    expect(store.data.size).toBe(0);
    store.setItem('taktak.openSection', 'not json');
    expect(takeRequestedSection(store, 0)).toBeNull();
    store.setItem('taktak.openSection', JSON.stringify({ id: 3, at: 0 }));
    expect(takeRequestedSection(store, 0)).toBeNull();
    expect(store.data.size).toBe(0);
  });

  it('survives storage that throws or is missing', () => {
    const broken: NoteStore = {
      getItem: () => {
        throw new Error('denied');
      },
      setItem: () => {
        throw new Error('denied');
      },
      removeItem: () => undefined,
    };
    expect(() => requestSection('apps', broken)).not.toThrow();
    expect(takeRequestedSection(broken)).toBeNull();
    expect(takeRequestedSection(null)).toBeNull();
  });
});
