// Copying a short text the user asked to copy (the tccutil command). Nothing is ever read from
// the clipboard.

/** The part of `Clipboard` this needs (tests pass their own). */
export interface ClipboardWriter {
  writeText(text: string): Promise<void>;
}

/** Copies `text`; resolves to whether it worked. */
export async function copyText(
  text: string,
  clipboard: ClipboardWriter | undefined = typeof navigator === 'undefined'
    ? undefined
    : navigator.clipboard,
): Promise<boolean> {
  try {
    if (clipboard) {
      await clipboard.writeText(text);
      return true;
    }
  } catch {
    // Not allowed here (e.g. no secure context): try the old way.
  }
  return copyWithSelection(text);
}

/** The pre-Clipboard-API way, for webviews that refuse `writeText`. */
function copyWithSelection(text: string): boolean {
  if (typeof document === 'undefined') return false;
  const area = document.createElement('textarea');
  area.value = text;
  area.readOnly = true;
  area.style.position = 'fixed';
  area.style.opacity = '0';
  area.style.userSelect = 'text';
  area.style.webkitUserSelect = 'text';
  document.body.append(area);
  area.select();
  let ok = false;
  try {
    ok = document.execCommand('copy');
  } catch {
    ok = false;
  }
  area.remove();
  return ok;
}
