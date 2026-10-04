// "Import Mechvibes pack…" (M5): the buttons per platform, the one-line summary of an import,
// and how imported (personal) packs are labelled. Plain functions so every text is tested.

import { plural } from './format';
import type { Platform } from './platform';
import type { ImportSummary, PackSummary, PickKind } from './types';

/** The license every imported pack carries: it plays here and is never bundled or shared. */
export const PERSONAL_LICENSE = 'LicenseRef-Personal';

/** A pack imported for personal use (shown with a "Personal" badge instead of its license). */
export function isPersonal(pack: Pick<PackSummary, 'license'>): boolean {
  return pack.license === PERSONAL_LICENSE;
}

/** The one-line note next to the import button and on the badge. */
export function personalNote(p: Platform): string {
  const here = p === 'mac' ? 'this Mac' : 'this computer';
  return `Imported packs stay on ${here} and are for your personal use only.`;
}

/** The "Personal" badge's tooltip. */
export function personalBadgeTitle(p: Platform): string {
  return `Imported for personal use. ${personalNote(p)} Not for sharing.`;
}

export interface ImportButton {
  kind: PickKind;
  label: string;
}

/**
 * macOS: one button, whose panel takes a folder or a .zip. Windows and Linux pickers choose
 * either files or folders, so there is a button for each.
 */
export function importButtons(p: Platform): ImportButton[] {
  if (p === 'mac') return [{ kind: 'any', label: 'Import Mechvibes pack…' }];
  return [
    { kind: 'zip', label: 'Import Mechvibes pack…' },
    { kind: 'folder', label: 'Import pack folder…' },
  ];
}

/** "84 keys mapped, 12 with their own release sound." */
export function importLine(pack: ImportSummary): string {
  const keys = `${plural(pack.keysMapped, 'key')} mapped`;
  const release =
    pack.keysWithRelease > 0 ? `, ${pack.keysWithRelease} with their own release sound` : '';
  return `${keys}${release}.`;
}

/** The result's title. */
export function importTitle(pack: ImportSummary): string {
  return pack.replaced ? `Replaced “${pack.name}”` : `Imported “${pack.name}”`;
}
