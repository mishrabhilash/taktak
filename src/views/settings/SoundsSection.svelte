<script lang="ts">
  // Pack browser: every valid pack as a card, packs that failed to load with their problems,
  // and where to put your own.
  import Icon from '../../components/Icon.svelte';
  import PackCard from '../../components/PackCard.svelte';
  import { openUserPacksDir, setPack } from '../../lib/api';
  import { run, set } from '../../lib/store.svelte';
  import type { AppState } from '../../lib/types';

  let { s }: { s: AppState } = $props();

  function select(id: string): void {
    void set('packId', id, setPack);
  }
</script>

<header class="section-header">
  <h1>Sounds</h1>
  <p>Pick how your keyboard sounds. Press ▶ to hear a pack without switching to it.</p>
</header>

{#if s.packs.length > 0}
  <div class="grid" role="radiogroup" aria-label="Sound pack">
    {#each s.packs as pack (pack.id)}
      {@const selected = pack.id === s.settings.packId}
      <PackCard
        {pack}
        {selected}
        group="pack"
        error={selected ? s.activePackError : null}
        onselect={select}
      />
    {/each}
  </div>
{:else}
  <div class="group empty">
    <p class="row-label">No sound packs found</p>
    <p class="hint">TakTak plays its built-in click until a pack is available.</p>
  </div>
{/if}

{#if s.invalidPacks.length > 0}
  <h2 class="group-title">Couldn’t load</h2>
  <ul class="group invalid">
    {#each s.invalidPacks as invalid (invalid.location)}
      <li class="row invalid-row">
        <span class="invalid-icon"><Icon name="warning" /></span>
        <div class="row-text">
          <p class="mono selectable location">{invalid.location}</p>
          <ul class="problems selectable">
            {#each invalid.problems as problem, i (i)}
              <li>{problem}</li>
            {/each}
          </ul>
        </div>
      </li>
    {/each}
  </ul>
{/if}

<h2 class="group-title">Your own packs</h2>
<div class="group">
  <div class="row user-packs">
    <div class="row-text">
      <span class="row-label">Add your own packs</span>
      <span class="hint">
        Put a pack folder or <span class="mono">.zip</span> in your packs folder. TakTak picks it up
        right away. The format is described in <span class="mono">docs/pack-format.md</span> in
        TakTak’s source code.
      </span>
      {#if s.userPacksDir}
        <p class="mono selectable path" title={s.userPacksDir}>{s.userPacksDir}</p>
      {/if}
    </div>
    <button type="button" class="btn" onclick={() => run(openUserPacksDir())}>
      <Icon name="folder" />Open packs folder
    </button>
  </div>
</div>

<style>
  .grid {
    display: grid;
    /* Two columns even at the window's minimum width. */
    grid-template-columns: repeat(auto-fill, minmax(172px, 1fr));
    gap: 12px;
  }

  .empty {
    padding: 16px;
  }

  .invalid-row {
    align-items: flex-start;
  }

  .invalid-icon {
    padding-top: 1px;
    color: var(--warning);
  }

  .location {
    overflow-wrap: anywhere;
  }

  .problems {
    margin-top: 4px;
    color: var(--text-2);
    font-size: 12px;
    overflow-wrap: anywhere;
  }

  .problems li + li {
    margin-top: 2px;
  }

  .user-packs {
    align-items: flex-start;
  }

  .path {
    margin-top: 8px;
    padding: 4px 8px;
    border-radius: var(--radius-s);
    background: var(--surface-2);
    color: var(--text-2);
    overflow-wrap: anywhere;
  }
</style>
