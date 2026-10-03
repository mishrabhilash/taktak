<script lang="ts">
  // One pack in the settings grid: a radio (the whole card) plus a separate ▶ preview button.
  import { plural } from '../lib/format';
  import type { PackSummary } from '../lib/types';
  import Icon from './Icon.svelte';
  import PreviewButton from './PreviewButton.svelte';

  interface Props {
    pack: PackSummary;
    selected: boolean;
    group: string;
    onselect: (id: string) => void;
    /** Why this (selected) pack failed to load, if it did. */
    error?: string | null;
  }
  let { pack, selected, group, onselect, error = null }: Props = $props();

  const descId = $derived(`pack-${pack.id}-desc`);
</script>

<div class="card" class:selected>
  <label class="main">
    <input
      type="radio"
      name={group}
      class="visually-hidden"
      value={pack.id}
      checked={selected}
      aria-describedby={descId}
      onchange={() => onselect(pack.id)}
    />
    <span class="ring" aria-hidden="true"></span>
    <span class="head">
      <span class="radio" aria-hidden="true"></span>
      <span class="name">{pack.name}</span>
    </span>
    <span class="author" title={pack.author}>by {pack.author}</span>
    <span class="desc" id={descId} title={pack.description ?? undefined}>
      {pack.description ?? 'No description.'}
    </span>
    <span class="badges">
      {#if error}
        <span class="badge error" title={error}><Icon name="error" size={11} />Not loaded</span>
      {/if}
      <span class="badge accent" title="License (SPDX)">{pack.license}</span>
      {#if pack.origin === 'user'}
        <span class="badge" title="From your packs folder">User</span>
      {:else}
        <span class="badge" title="Ships with TakTak">Bundled</span>
      {/if}
      {#if pack.perKey}
        <span class="badge" title="Some keys have their own sounds">Per-key</span>
      {/if}
      {#if pack.hasRelease}
        <span class="badge" title="Also plays a sound when a key comes up">Release</span>
      {/if}
      {#if pack.warnings.length > 0}
        <span class="badge warning" title={pack.warnings.join('\n')}>
          <Icon name="warning" size={11} />{plural(pack.warnings.length, 'warning')}
        </span>
      {/if}
    </span>
  </label>
  <div class="preview"><PreviewButton id={pack.id} name={pack.name} /></div>
  {#if pack.warnings.length > 0}
    <ul class="warnings selectable">
      {#each pack.warnings as warning, i (i)}
        <li>{warning}</li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .card {
    position: relative;
    display: flex;
    flex-direction: column;
    border-radius: var(--radius-l);
    background: var(--surface);
    box-shadow: var(--card-shadow);
    transition: box-shadow 0.15s;
  }

  .card:hover {
    box-shadow:
      var(--card-shadow),
      0 0 0 1px var(--border);
  }

  .card.selected {
    box-shadow: 0 0 0 2px var(--accent);
  }

  .main {
    position: relative;
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 12px;
    border-radius: var(--radius-l);
  }

  .ring {
    position: absolute;
    inset: 0;
    border-radius: inherit;
    pointer-events: none;
  }

  input:focus-visible + .ring {
    box-shadow: var(--focus-ring);
  }

  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    padding-right: 32px;
  }

  .name {
    font-size: 14px;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .radio {
    flex: none;
    position: relative;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: var(--surface);
    box-shadow: inset 0 0 0 1px var(--border);
  }

  .selected .radio {
    background: var(--accent);
    box-shadow: none;
  }

  .selected .radio::after {
    content: '';
    position: absolute;
    inset: 4px;
    border-radius: 50%;
    background: #fff;
  }

  .author {
    color: var(--text-2);
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .desc {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    min-height: calc(2 * 1.4em);
    margin-top: 2px;
    overflow: hidden;
    color: var(--text-2);
    font-size: 12px;
    line-height: 1.4;
  }

  .badges {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin-top: auto;
    padding-top: 6px;
  }

  .preview {
    position: absolute;
    top: 8px;
    right: 8px;
  }

  .warnings {
    margin: 0 12px 12px;
    padding: 6px 8px;
    border-radius: var(--radius-s);
    background: var(--warning-bg);
    color: var(--warning-text);
    font-size: 11.5px;
    overflow-wrap: anywhere;
  }
</style>
