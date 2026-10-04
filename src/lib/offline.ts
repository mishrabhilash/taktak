// The offline promise, word for word wherever TakTak states it: the welcome window, Settings →
// About, the tray popover, the README and the website. It stays true because TakTak has no
// network code at all, which CI's offline guard (`npm run no-network`) enforces.

/** The full sentence. */
export const OFFLINE = 'TakTak is fully offline — it never uses the internet.';

/** The short form, for the tray popover. */
export const OFFLINE_SHORT = 'Offline · never uses the internet';

/** The welcome window's badge (the full sentence is its tooltip and is said under "Why?"). */
export const OFFLINE_BADGE = 'Fully offline · never uses the internet';
