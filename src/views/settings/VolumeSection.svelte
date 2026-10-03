<script lang="ts">
  import Icon from '../../components/Icon.svelte';
  import Slider from '../../components/Slider.svelte';
  import { setMasterVolume, setPressVolume, setReleaseVolume } from '../../lib/api';
  import { levelControl } from '../../lib/store.svelte';
  import type { AppState } from '../../lib/types';

  let { s }: { s: AppState } = $props();

  const master = levelControl('masterVolume', setMasterVolume);
  const press = levelControl('pressVolume', setPressVolume);
  const release = levelControl('releaseVolume', setReleaseVolume);

  // The pack that plays (a fallback when the selected one failed), for the release hint.
  const playingPack = $derived(s.packs.find((p) => p.id === s.playingPackId) ?? null);
</script>

<header class="section-header">
  <h1>Volume</h1>
  <p>TakTak’s own levels, on top of your system volume.</p>
</header>

<div class="group">
  <div class="row level">
    <div class="level-head">
      <label class="row-label" for="vol-master">Master volume</label>
      <span class="hint" id="vol-master-hint">Everything TakTak plays, including previews.</span>
    </div>
    <div class="level-slider">
      <Icon name="speaker-low" />
      <Slider
        id="vol-master"
        describedby="vol-master-hint"
        value={s.settings.masterVolume}
        oninput={master.input}
        oncommit={master.commit}
      />
    </div>
  </div>
  <div class="row level">
    <div class="level-head">
      <label class="row-label" for="vol-press">Key press</label>
      <span class="hint" id="vol-press-hint">The sound when a key goes down.</span>
    </div>
    <div class="level-slider">
      <Icon name="speaker-low" />
      <Slider
        id="vol-press"
        describedby="vol-press-hint"
        value={s.settings.pressVolume}
        oninput={press.input}
        oncommit={press.commit}
      />
    </div>
  </div>
  <div class="row level">
    <div class="level-head">
      <label class="row-label" for="vol-release">Key release</label>
      <span class="hint" id="vol-release-hint">
        The sound when a key comes back up.
        {#if playingPack && !playingPack.hasRelease}
          {playingPack.name} has no release sounds.
        {/if}
      </span>
    </div>
    <div class="level-slider">
      <Icon name="speaker-low" />
      <Slider
        id="vol-release"
        describedby="vol-release-hint"
        value={s.settings.releaseVolume}
        oninput={release.input}
        oncommit={release.commit}
      />
    </div>
  </div>
</div>

<style>
  .level {
    flex-direction: column;
    align-items: stretch;
    gap: 8px;
    padding: 12px 14px;
  }

  .level-slider {
    display: flex;
    align-items: center;
    gap: 10px;
    color: var(--text-2);
  }
</style>
