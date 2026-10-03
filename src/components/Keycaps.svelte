<script lang="ts">
  // An accelerator drawn as keycaps (⌥ ⇧ ⌘ M), read out as words to screen readers. On macOS
  // the key is a position: it shows the layout's character when known, else the US key, and
  // the tooltip says so.
  import { acceleratorParts, describeAccelerator, namesUsPosition } from '../lib/accelerator';
  import { keyboardLayout } from '../lib/keyboard.svelte';
  import { platform } from '../lib/platform';

  let { accelerator, small = false }: { accelerator: string; small?: boolean } = $props();

  const layout = $derived(keyboardLayout.labels);
  const parts = $derived(acceleratorParts(accelerator, platform, layout));
  const words = $derived(describeAccelerator(accelerator, platform, layout));
  const title = $derived(
    namesUsPosition(accelerator, platform, layout)
      ? `${words} (the key in that place on a US keyboard)`
      : words,
  );
</script>

<span class="keycaps" class:small {title}>
  <span class="visually-hidden">{words}</span>
  {#each parts as part, i (i)}
    <kbd aria-hidden="true">{part}</kbd>
  {/each}
</span>

<style>
  .keycaps {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: 3px;
    flex: none;
  }

  kbd {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 22px;
    height: 22px;
    padding: 0 6px;
    border-radius: 5px;
    background: var(--surface);
    box-shadow:
      0 0 0 0.5px var(--border),
      0 1px 0 var(--border);
    color: var(--text);
    font-family: var(--font);
    font-size: 12px;
    font-weight: 500;
    line-height: 1;
  }

  .small kbd {
    min-width: 18px;
    height: 18px;
    padding: 0 4px;
    border-radius: 4px;
    font-size: 11px;
  }
</style>
