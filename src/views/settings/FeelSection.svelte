<script lang="ts">
  import Slider from '../../components/Slider.svelte';
  import Switch from '../../components/Switch.svelte';
  import { setHumanize, setVariantMode } from '../../lib/api';
  import { levelControl, set } from '../../lib/store.svelte';
  import type { AppState } from '../../lib/types';

  let { s }: { s: AppState } = $props();

  const humanize = levelControl('humanize', setHumanize);
</script>

<header class="section-header">
  <h1>Feel</h1>
  <p>How alike your keystrokes sound.</p>
</header>

<div class="group">
  <div class="row">
    <div class="row-text">
      <span class="row-label" id="feel-consistent">Same key, same sound</span>
      <span class="hint" id="feel-consistent-hint">
        On: each key keeps its own sound, like a real keyboard. Off: a random one every press.
      </span>
    </div>
    <Switch
      labelledby="feel-consistent"
      describedby="feel-consistent-hint"
      checked={s.settings.variantMode === 'consistent'}
      onchange={(on) => set('variantMode', on ? 'consistent' : 'random', setVariantMode)}
    />
  </div>
  <div class="row level">
    <div>
      <label class="row-label" for="feel-humanize">Natural variation</label>
      <span class="hint" id="feel-humanize-hint">
        Small random changes in pitch and loudness on every keystroke. At 0% every press sounds
        exactly the same.
      </span>
    </div>
    <Slider
      id="feel-humanize"
      describedby="feel-humanize-hint"
      value={s.settings.humanize}
      oninput={humanize.input}
      oncommit={humanize.commit}
    />
  </div>
</div>

<style>
  .level {
    flex-direction: column;
    align-items: stretch;
    gap: 8px;
    padding: 12px 14px;
  }
</style>
