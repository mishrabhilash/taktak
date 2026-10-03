// App icons by bundle id, for the apps the user lists or picks: filled from list_running_apps,
// choose_app and get_app_icons. In memory for as long as this window lives; never persisted and
// never sent anywhere. The app in front is deliberately never looked up (see `frontmostIcon`):
// the app caches every icon it renders for the session, so asking for the icon of each app the
// user brings to the front would turn that cache into a record of the apps they visited.

import { SvelteMap } from 'svelte/reactivity';
import { getAppIcons } from './api';
import type { AppInfo } from './types';

/** The most ids one get_app_icons call answers. */
export const MAX_ICON_IDS = 200;

/** id → data: URL, or null when the app has no icon (show the generic one). */
const icons = new SvelteMap<string, string | null>();
/** Asked for and not answered yet. */
const pending = new Set<string>();

/** The icon of `id`: a data: URL, null for none, undefined while unknown. */
export function iconOf(id: string): string | null | undefined {
  return icons.get(id);
}

/**
 * The icon to show for the app in front: the one already known (a listed or picked app, or one
 * of the running apps "Add app" offered), else null (the generic icon). Never asks the app, and
 * never adds anything to this cache.
 */
export function frontmostIcon(id: string): string | null {
  return icons.get(id) ?? null;
}

/** Keeps the icons that came with apps from list_running_apps or choose_app. */
export function rememberIcons(apps: readonly AppInfo[]): void {
  for (const app of apps) icons.set(app.id, app.iconDataUrl);
}

/** The ids `known` lacks, once each, in order, at most `MAX_ICON_IDS`. */
export function missingIconIds(ids: readonly string[], known: (id: string) => boolean): string[] {
  const missing: string[] = [];
  const seen = new Set<string>();
  for (const id of ids) {
    if (missing.length >= MAX_ICON_IDS) break;
    if (seen.has(id) || known(id)) continue;
    seen.add(id);
    missing.push(id);
  }
  return missing;
}

/** Fetches the icons of `ids` that are not known yet (for listed apps that are not running). */
export async function loadIcons(ids: readonly string[]): Promise<void> {
  const missing = missingIconIds(ids, (id) => icons.has(id) || pending.has(id));
  if (missing.length === 0) return;
  for (const id of missing) pending.add(id);
  try {
    const found = await getAppIcons(missing);
    for (const id of missing) icons.set(id, found[id] ?? null);
  } catch {
    // Generic icons meanwhile; the next change of the list asks again.
  } finally {
    for (const id of missing) pending.delete(id);
  }
}
