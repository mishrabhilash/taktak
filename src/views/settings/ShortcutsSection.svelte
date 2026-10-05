<script lang="ts">
  import HotkeyRecorder from '../../components/HotkeyRecorder.svelte';
  import { keyboardLayout } from '../../lib/keyboard.svelte';
  import { platform } from '../../lib/platform';
  import type { AppState } from '../../lib/types';

  let { s }: { s: AppState } = $props();

  // macOS registers key positions; without the app's (or the webview's) layout labels, names
  // follow the US layout.
  const usPositions = $derived(platform === 'mac' && !keyboardLayout.labels);
</script>

<header class="section-header">
  <h1>Shortcuts</h1>
  <p>Keyboard shortcuts that work in every app, even when TakTak is in the background.</p>
</header>

<div class="group">
  <div class="row shortcut">
    <div class="row-text">
      <span class="row-label" id="shortcut-mute">Mute / unmute</span>
      <span class="hint" id="shortcut-mute-hint">
        Silences TakTak without turning it off. Click Record, then press the keys together.
      </span>
    </div>
    <HotkeyRecorder
      accelerator={s.settings.muteHotkey}
      labelledby="shortcut-mute"
      describedby="shortcut-mute-hint"
    />
    {#if s.muteHotkeyError}
      <p class="startup-error" role="alert">{s.muteHotkeyError}</p>
    {/if}
  </div>
</div>

{#if usPositions}
  <p class="footnote">
    Shortcuts follow key positions. Letters and symbols are named as on a US keyboard, so with
    another layout a name can differ from the label on your key. Recording always uses the keys
    you press.
  </p>
{/if}
<p class="footnote">
  Keys are read only while you record, inside this window. TakTak keeps just the shortcut itself.
</p>

<style>
  .shortcut {
    flex-direction: column;
    align-items: stretch;
    gap: 10px;
    padding: 12px 14px 8px;
  }

  .startup-error {
    margin: 0;
    color: var(--error);
    font-size: 12px;
  }

  .footnote {
    margin: 12px 4px 0;
    color: var(--text-2);
    font-size: 12px;
  }

  .footnote + .footnote {
    margin-top: 6px;
  }
</style>
