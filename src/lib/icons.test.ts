import { describe, expect, it, vi } from 'vitest';
import * as api from './api';
import {
  MAX_ICON_IDS,
  frontmostIcon,
  iconOf,
  loadIcons,
  missingIconIds,
  rememberIcons,
} from './icons.svelte';

describe('icon cache', () => {
  it('asks only for unknown ids, once each, at most 200', () => {
    const known = new Set(['a']);
    expect(missingIconIds(['a', 'b', 'b', 'c'], (id) => known.has(id))).toEqual(['b', 'c']);
    const many = Array.from({ length: 300 }, (_, i) => `id${i}`);
    const missing = missingIconIds(many, () => false);
    expect(missing).toHaveLength(MAX_ICON_IDS);
    expect(missing[0]).toBe('id0');
  });

  it('keeps icons from app lists, null for none, and loads the rest from the mock', async () => {
    rememberIcons([
      { id: 'com.example.a', name: 'A', iconDataUrl: 'data:image/png;base64,AAAA' },
      { id: 'com.example.b', name: 'B', iconDataUrl: null },
    ]);
    expect(iconOf('com.example.a')).toBe('data:image/png;base64,AAAA');
    expect(iconOf('com.example.b')).toBeNull();
    expect(iconOf('com.apple.Safari')).toBeUndefined();
    await loadIcons(['com.apple.Safari', 'com.example.unknown']);
    expect(iconOf('com.apple.Safari')).toMatch(/^data:image\//);
    expect(iconOf('com.example.unknown')).toBeNull();
  });

  it('shows the frontmost app’s icon only when known, never asking for it or caching it', () => {
    const spy = vi.spyOn(api, 'getAppIcons');
    try {
      rememberIcons([
        { id: 'com.example.front', name: 'F', iconDataUrl: 'data:image/png;base64,BB' },
      ]);
      expect(frontmostIcon('com.example.front')).toBe('data:image/png;base64,BB');
      // An app brought to the front that TakTak has no icon for: the generic icon, no lookup.
      expect(frontmostIcon('com.example.visited')).toBeNull();
      expect(iconOf('com.example.visited')).toBeUndefined();
      expect(spy).not.toHaveBeenCalled();
    } finally {
      spy.mockRestore();
    }
  });
});
