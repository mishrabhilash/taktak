// Which pack's preview is playing, for the ▶/■ buttons. The app does not report when a clip
// ends, so the button shows "playing" for about as long as the bundled previews last (~2 s).

import { previewPack, stopPreview } from './api';
import { showError } from './store.svelte';

const PREVIEW_MS = 2200;

export const preview = $state<{ id: string | null }>({ id: null });

let timer: ReturnType<typeof setTimeout> | undefined;

/** Plays `id`'s preview, or stops it if it is the one playing. */
export async function togglePreview(id: string): Promise<void> {
  clearTimeout(timer);
  if (preview.id === id) {
    preview.id = null;
    await stopPreview().catch(showError);
    return;
  }
  preview.id = id;
  timer = setTimeout(() => {
    if (preview.id === id) preview.id = null;
  }, PREVIEW_MS);
  try {
    await previewPack(id);
  } catch (e) {
    if (preview.id === id) preview.id = null;
    showError(e);
  }
}
