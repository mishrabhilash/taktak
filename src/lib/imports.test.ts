import { describe, expect, it } from 'vitest';
import {
  PERSONAL_LICENSE,
  importButtons,
  importLine,
  importTitle,
  isPersonal,
  personalBadgeTitle,
  personalNote,
} from './imports';
import type { ImportSummary } from './types';

const pack: ImportSummary = {
  id: 'mv-cream',
  name: 'Cream',
  source: 'cream.zip',
  format: 'Mechvibes v2',
  keysMapped: 84,
  keysWithRelease: 12,
  soundsWritten: 96,
  replaced: false,
  warnings: [],
};

describe('imports', () => {
  it('recognizes personal packs by their license', () => {
    expect(PERSONAL_LICENSE).toBe('LicenseRef-Personal');
    expect(isPersonal({ license: 'LicenseRef-Personal' })).toBe(true);
    expect(isPersonal({ license: 'CC0-1.0' })).toBe(false);
  });

  it('says imported packs stay on this machine, per platform', () => {
    expect(personalNote('mac')).toBe(
      'Imported packs stay on this Mac and are for your personal use only.',
    );
    expect(personalNote('windows')).toContain('this computer');
    expect(personalBadgeTitle('linux')).toContain('Not for sharing');
  });

  it('macOS gets one button (its panel takes both); elsewhere a .zip and a folder button', () => {
    expect(importButtons('mac')).toEqual([{ kind: 'any', label: 'Import Mechvibes pack…' }]);
    expect(importButtons('windows').map((b) => b.kind)).toEqual(['zip', 'folder']);
    expect(importButtons('linux')[0]?.label).toBe('Import Mechvibes pack…');
  });

  it('summarizes an import in one line', () => {
    expect(importLine(pack)).toBe('84 keys mapped, 12 with their own release sound.');
    expect(importLine({ ...pack, keysMapped: 1, keysWithRelease: 0 })).toBe('1 key mapped.');
    expect(importTitle(pack)).toBe('Imported “Cream”');
    expect(importTitle({ ...pack, replaced: true })).toBe('Replaced “Cream”');
  });
});
