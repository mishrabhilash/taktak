<script lang="ts">
  // Pack browser: every valid pack as a card, packs that failed to load with their problems,
  // where to put your own, and (M5) importing a Mechvibes pack.
  import Icon from '../../components/Icon.svelte';
  import PackCard from '../../components/PackCard.svelte';
  import {
    importMechvibesPack,
    openUserPacksDir,
    overwriteMechvibesPack,
    setPack,
  } from '../../lib/api';
  import { plural } from '../../lib/format';
  import { importButtons, importLine, importTitle, personalNote } from '../../lib/imports';
  import { platform } from '../../lib/platform';
  import { run, set, showError } from '../../lib/store.svelte';
  import type { AppState, MechvibesImport, PickKind } from '../../lib/types';

  let { s }: { s: AppState } = $props();

  function select(id: string): void {
    void set('packId', id, setPack);
  }

  const buttons = importButtons(platform);
  /** A picker is open or an import runs. */
  let importing = $state(false);
  /** The last import's outcome, until dismissed. */
  let result = $state<MechvibesImport | null>(null);

  async function runImport(task: () => Promise<MechvibesImport | null>): Promise<void> {
    importing = true;
    try {
      const outcome = await task();
      // A cancelled picker leaves the last result where it was.
      if (outcome) result = outcome;
    } catch (e) {
      result = null;
      showError(e);
    } finally {
      importing = false;
    }
  }

  const startImport = (kind: PickKind) => runImport(() => importMechvibesPack(kind));
  const overwrite = () => runImport(overwriteMechvibesPack);

  /** The imported pack, once the registry lists it (about a second after the import). */
  const listed = $derived.by(() => {
    const r = result;
    return r?.outcome === 'imported' && s.packs.some((p) => p.id === r.pack.id);
  });
  /** The earlier import's name, for "already imported". */
  const earlierName = $derived.by(() => {
    const r = result;
    if (r?.outcome !== 'alreadyImported') return '';
    return s.packs.find((p) => p.id === r.id)?.name ?? r.source;
  });

  function useImported(id: string): void {
    select(id);
    result = null;
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
  <div class="row user-packs">
    <div class="row-text">
      <span class="row-label">Import from Mechvibes</span>
      <span class="hint">
        Converts a Mechvibes sound pack, its folder or <span class="mono">.zip</span>, into a TakTak
        pack in your packs folder. The original isn’t changed.
      </span>
      <p class="personal-note"><Icon name="lock" size={12} />{personalNote(platform)}</p>
    </div>
    <div class="import-buttons">
      {#each buttons as button (button.kind)}
        <button
          type="button"
          class="btn"
          disabled={importing}
          onclick={() => startImport(button.kind)}
        >
          {button.label}
        </button>
      {/each}
    </div>
  </div>
  {#if result}
    <div class="row import-result" role="status" aria-live="polite">
      {#if result.outcome === 'imported'}
        {@const pack = result.pack}
        <span class="result-icon ok"><Icon name="check" size={14} /></span>
        <div class="row-text">
          <span class="row-label">{importTitle(pack)}</span>
          <span class="hint">{importLine(pack)} {pack.format} · {pack.source}</span>
          {#if pack.warnings.length > 0}
            <details class="import-notes">
              <summary>{plural(pack.warnings.length, 'note')}</summary>
              <ul class="selectable">
                {#each pack.warnings as warning, i (i)}
                  <li>{warning}</li>
                {/each}
              </ul>
            </details>
          {/if}
        </div>
        <div class="result-actions">
          <button
            type="button"
            class="btn primary"
            disabled={!listed || s.settings.packId === pack.id}
            onclick={() => useImported(pack.id)}
          >
            {listed ? 'Use this pack' : 'Adding…'}
          </button>
          <button
            type="button"
            class="btn plain icon-only"
            aria-label="Dismiss"
            onclick={() => (result = null)}
          >
            <Icon name="close" size={12} />
          </button>
        </div>
      {:else}
        <span class="result-icon"><Icon name="info" size={14} /></span>
        <div class="row-text">
          <span class="row-label">“{earlierName}” was imported before</span>
          <span class="hint">
            Replace it with a fresh import of {result.source}? Its sounds are converted again.
          </span>
        </div>
        <div class="result-actions">
          <button type="button" class="btn primary" disabled={importing} onclick={overwrite}>
            Replace
          </button>
          <button type="button" class="btn" disabled={importing} onclick={() => (result = null)}>
            Cancel
          </button>
        </div>
      {/if}
    </div>
  {/if}
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

  .personal-note {
    display: flex;
    align-items: center;
    gap: 5px;
    margin-top: 6px;
    color: var(--text-2);
    font-size: 12px;
  }

  .import-buttons,
  .result-actions {
    flex: none;
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: 6px;
  }

  .result-actions {
    flex-direction: row;
    align-items: center;
  }

  .import-result {
    align-items: flex-start;
    background: var(--accent-soft);
  }

  .result-icon {
    padding-top: 1px;
    color: var(--text-2);
  }

  .result-icon.ok {
    color: var(--ok);
  }

  .import-notes {
    margin-top: 4px;
    color: var(--text-2);
    font-size: 12px;
  }

  .import-notes summary {
    cursor: default;
  }

  .import-notes ul {
    margin-top: 4px;
    overflow-wrap: anywhere;
  }

  .import-notes li + li {
    margin-top: 2px;
  }

  .icon-only {
    width: 24px;
    padding: 0;
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
