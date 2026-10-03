<script lang="ts">
  // ▶ plays a pack's preview clip without selecting it; ■ stops it.
  import { preview, togglePreview } from '../lib/preview.svelte';
  import Icon from './Icon.svelte';

  let { id, name }: { id: string; name: string } = $props();

  const playing = $derived(preview.id === id);
</script>

<button
  type="button"
  class="preview"
  class:playing
  aria-label={playing ? `Stop preview of ${name}` : `Preview ${name}`}
  title={playing ? 'Stop preview' : 'Preview'}
  onclick={() => togglePreview(id)}
>
  <Icon name={playing ? 'stop' : 'play'} size={12} />
</button>

<style>
  .preview {
    flex: none;
    position: relative;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: var(--surface-2);
    color: var(--text-2);
    transition:
      background-color 0.12s,
      color 0.12s;
  }

  .preview:hover {
    background: var(--press);
    color: var(--text);
  }

  .preview.playing {
    background: var(--accent-soft);
    color: var(--accent-text);
  }

  .preview.playing::after {
    content: '';
    position: absolute;
    inset: -2px;
    border-radius: 50%;
    border: 1.5px solid var(--accent);
    opacity: 0;
    animation: pulse 1.1s ease-out infinite;
  }

  @keyframes pulse {
    0% {
      transform: scale(0.85);
      opacity: 0.8;
    }
    100% {
      transform: scale(1.25);
      opacity: 0;
    }
  }
</style>
