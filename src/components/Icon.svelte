<script lang="ts" module>
  export type IconName =
    | 'play'
    | 'stop'
    | 'speaker'
    | 'speaker-low'
    | 'mute'
    | 'sliders'
    | 'power'
    | 'check'
    | 'warning'
    | 'error'
    | 'info'
    | 'folder'
    | 'close'
    | 'waveform'
    | 'wave'
    | 'keyboard'
    | 'toggle'
    | 'shield'
    | 'gauge'
    | 'plus'
    | 'search'
    | 'apps'
    | 'lock'
    | 'copy'
    | 'restart'
    | 'chevron'
    | 'headphones';
</script>

<script lang="ts">
  // Inline 16×16 stroke icons (no icon font, nothing remote). Decorative: aria-hidden.
  let { name, size = 16 }: { name: IconName; size?: number } = $props();
</script>

<svg
  class="icon"
  width={size}
  height={size}
  viewBox="0 0 16 16"
  fill="none"
  stroke="currentColor"
  stroke-width="1.5"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
  focusable="false"
>
  {#if name === 'play'}
    <path
      d="M5.2 3.6v8.8a.6.6 0 0 0 .92.5l6.7-4.4a.6.6 0 0 0 0-1l-6.7-4.4a.6.6 0 0 0-.92.5z"
      fill="currentColor"
      stroke="none"
    />
  {:else if name === 'stop'}
    <rect x="4" y="4" width="8" height="8" rx="1.6" fill="currentColor" stroke="none" />
  {:else if name === 'speaker' || name === 'speaker-low' || name === 'mute'}
    <path
      d="M2.4 6.2v3.6c0 .3.2.5.5.5h2.2l3.1 2.6c.3.3.8 0 .8-.4V3.5c0-.4-.5-.7-.8-.4L5.1 5.7H2.9c-.3 0-.5.2-.5.5z"
      fill="currentColor"
      stroke="none"
    />
    {#if name === 'mute'}
      <path d="M11 6.2l3.6 3.6M14.6 6.2L11 9.8" />
    {:else}
      <path d="M10.8 5.9a3 3 0 0 1 0 4.2" />
      {#if name === 'speaker'}<path d="M12.7 4a5.7 5.7 0 0 1 0 8" />{/if}
    {/if}
  {:else if name === 'sliders'}
    <path d="M2.5 4.5h11M2.5 8h11M2.5 11.5h11" />
    <circle cx="5.5" cy="4.5" r="1.7" fill="var(--icon-knob, var(--surface))" />
    <circle cx="10.5" cy="8" r="1.7" fill="var(--icon-knob, var(--surface))" />
    <circle cx="6.5" cy="11.5" r="1.7" fill="var(--icon-knob, var(--surface))" />
  {:else if name === 'power'}
    <path d="M8 2.2v5.3" />
    <path d="M4.7 4.4a5 5 0 1 0 6.6 0" />
  {:else if name === 'check'}
    <path d="M3.4 8.4l3 3 6.2-6.8" />
  {:else if name === 'warning'}
    <path d="M7.1 2.9a1 1 0 0 1 1.8 0l5.3 9.4a1 1 0 0 1-.9 1.5H2.7a1 1 0 0 1-.9-1.5z" />
    <path d="M8 6.4v3" />
    <circle cx="8" cy="11.4" r="0.4" fill="currentColor" />
  {:else if name === 'error'}
    <circle cx="8" cy="8" r="6" />
    <path d="M6.1 6.1l3.8 3.8M9.9 6.1L6.1 9.9" />
  {:else if name === 'info'}
    <circle cx="8" cy="8" r="6" />
    <path d="M8 7.4v3.7" />
    <circle cx="8" cy="5.1" r="0.4" fill="currentColor" />
  {:else if name === 'folder'}
    <path
      d="M2 4.6c0-.6.4-1.1 1-1.1h3l1.4 1.5H13c.6 0 1 .4 1 1v6.4c0 .6-.4 1-1 1H3c-.6 0-1-.4-1-1z"
    />
  {:else if name === 'close'}
    <path d="M4.5 4.5l7 7M11.5 4.5l-7 7" />
  {:else if name === 'waveform'}
    <path d="M2.5 6.8v2.4M5.2 4.5v7M8 2.5v11M10.8 5v6M13.5 6.8v2.4" />
  {:else if name === 'wave'}
    <path d="M1.5 8c1.1-3.2 2.2-3.2 3.3 0s2.2 3.2 3.2 0 2.2-3.2 3.3 0 2.1 3.2 3.2 0" />
  {:else if name === 'keyboard'}
    <rect x="1.5" y="3.8" width="13" height="8.4" rx="1.6" />
    <path d="M4.2 6.6h.1M6.7 6.6h.1M9.2 6.6h.1M11.7 6.6h.1M5 9.4h6" />
  {:else if name === 'toggle'}
    <rect x="1.5" y="4.5" width="13" height="7" rx="3.5" />
    <circle cx="11" cy="8" r="1.8" fill="currentColor" stroke="none" />
  {:else if name === 'shield'}
    <path d="M8 1.9l5 1.9v3.9c0 3-2.1 5.2-5 6.4-2.9-1.2-5-3.4-5-6.4V3.8z" />
    <path d="M5.8 8.1l1.5 1.5 3-3.2" />
  {:else if name === 'gauge'}
    <path d="M2.6 11.5a5.6 5.6 0 1 1 10.8 0" />
    <path d="M8 9.6l2.6-3.2" />
  {:else if name === 'plus'}
    <path d="M8 3.2v9.6M3.2 8h9.6" />
  {:else if name === 'search'}
    <circle cx="7" cy="7" r="4.3" />
    <path d="M10.2 10.2l3.4 3.4" />
  {:else if name === 'apps'}
    <rect x="2.3" y="2.3" width="4.8" height="4.8" rx="1.3" />
    <rect x="8.9" y="2.3" width="4.8" height="4.8" rx="1.3" />
    <rect x="2.3" y="8.9" width="4.8" height="4.8" rx="1.3" />
    <rect x="8.9" y="8.9" width="4.8" height="4.8" rx="1.3" />
  {:else if name === 'lock'}
    <rect x="3.3" y="7" width="9.4" height="6.6" rx="1.6" />
    <path d="M5.4 7V5.3a2.6 2.6 0 0 1 5.2 0V7" />
  {:else if name === 'copy'}
    <rect x="5.6" y="5.6" width="7.9" height="7.9" rx="1.5" />
    <path d="M10.4 5.6V3.9c0-.8-.6-1.4-1.4-1.4H3.9c-.8 0-1.4.6-1.4 1.4V9c0 .8.6 1.4 1.4 1.4h1.7" />
  {:else if name === 'restart'}
    <path d="M3.4 9.6a4.8 4.8 0 1 0 .9-5" />
    <path d="M3.6 2.4v2.9h2.9" />
  {:else if name === 'chevron'}
    <path d="M6.2 3.6L10.6 8l-4.4 4.4" />
  {:else if name === 'headphones'}
    <path d="M2.6 11V8.6a5.4 5.4 0 0 1 10.8 0V11" />
    <rect x="2.2" y="9.4" width="3" height="4.4" rx="1.2" />
    <rect x="10.8" y="9.4" width="3" height="4.4" rx="1.2" />
  {/if}
</svg>

<style>
  .icon {
    flex: none;
    display: block;
  }
</style>
