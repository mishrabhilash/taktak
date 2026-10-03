// Opening Settings at a given section from the popover ("Edit rules…"). `open_settings` takes no
// arguments, so the popover leaves a short-lived note in localStorage, which both windows share
// (same origin). Settings takes it when it loads, or through the `storage` event when it is open
// already (where the webview delivers it; otherwise Settings just comes to the front). The note
// holds a section id and a time, is removed when taken and ignored once it is stale.

const KEY = 'taktak.openSection';
/** A note older than this is left over from an open that never happened. */
export const NOTE_TTL_MS = 10_000;

/** The part of `Storage` this needs (tests pass their own). */
export interface NoteStore {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

function localNotes(): NoteStore | null {
  try {
    return typeof window === 'undefined' ? null : window.localStorage;
  } catch {
    return null; // Storage disabled: Settings opens where it was.
  }
}

/** Asks the Settings window to show section `id` when it opens (or now, if it is open). */
export function requestSection(id: string, store = localNotes(), now = Date.now()): void {
  try {
    store?.setItem(KEY, JSON.stringify({ id, at: now }));
  } catch {
    // Best effort.
  }
}

/** The section the popover asked for, if any and still fresh; removes the note. */
export function takeRequestedSection(store = localNotes(), now = Date.now()): string | null {
  try {
    const raw = store?.getItem(KEY) ?? null;
    if (raw === null) return null;
    store?.removeItem(KEY);
    const note: unknown = JSON.parse(raw);
    if (typeof note !== 'object' || note === null) return null;
    const { id, at } = note as { id?: unknown; at?: unknown };
    if (typeof id !== 'string' || typeof at !== 'number') return null;
    return now - at >= 0 && now - at < NOTE_TTL_MS ? id : null;
  } catch {
    return null;
  }
}

/** Calls `show` when the popover asks for a section while this window is open. */
export function onSectionRequest(show: (id: string) => void): () => void {
  const listener = (e: StorageEvent): void => {
    if (e.key !== KEY || e.newValue === null) return;
    const id = takeRequestedSection();
    if (id !== null) show(id);
  };
  window.addEventListener('storage', listener);
  return () => window.removeEventListener('storage', listener);
}
