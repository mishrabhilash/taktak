<script lang="ts">
  // An app's icon from the in-memory cache (or `src`), or a generic one while there is none.
  // Decorative: the app's name is always written next to it.
  import { iconOf } from '../lib/icons.svelte';

  interface Props {
    id: string;
    size?: number;
    /** Use this icon instead of the cache's (undefined = the cache's). */
    src?: string | null;
  }
  let { id, size = 24, src }: Props = $props();

  const url = $derived(src === undefined ? iconOf(id) : src);
  /** A data: URL WebKit could not decode: show the generic icon for it. */
  let broken = $state<string | null>(null);
</script>

{#if url && url !== broken}
  <img
    class="app-icon"
    src={url}
    alt=""
    width={size}
    height={size}
    draggable="false"
    onerror={() => (broken = url)}
  />
{:else}
  <svg class="app-icon" width={size} height={size} viewBox="0 0 32 32" aria-hidden="true">
    <rect class="tile" x="2.5" y="2.5" width="27" height="27" rx="7" />
    <g class="glyph">
      <rect x="9" y="9" width="5.5" height="5.5" rx="1.4" />
      <rect x="17.5" y="9" width="5.5" height="5.5" rx="1.4" />
      <rect x="9" y="17.5" width="5.5" height="5.5" rx="1.4" />
      <rect x="17.5" y="17.5" width="5.5" height="5.5" rx="1.4" />
    </g>
  </svg>
{/if}

<style>
  .app-icon {
    flex: none;
    display: block;
  }

  .tile {
    fill: var(--surface-2);
    stroke: var(--border);
    stroke-width: 1;
  }

  .glyph {
    fill: var(--text-3);
    opacity: 0.55;
  }
</style>
