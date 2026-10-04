<script lang="ts">
  // The popover under the tray icon: on/off, mute, volume, pack choice, and why nothing plays
  // (auto-mute, permission, audio, per-app rules). Escape hides it (it also hides when it loses
  // focus).
  import Icon from '../components/Icon.svelte';
  import Keycaps from '../components/Keycaps.svelte';
  import Logo from '../components/Logo.svelte';
  import Notices from '../components/Notices.svelte';
  import PreviewButton from '../components/PreviewButton.svelte';
  import Slider from '../components/Slider.svelte';
  import Switch from '../components/Switch.svelte';
  import Toast from '../components/Toast.svelte';
  import {
    hideTray,
    openSettings,
    quit,
    setEnabled,
    setMasterVolume,
    setMuted,
    setPack,
  } from '../lib/api';
  import { isPersonal } from '../lib/imports';
  import { OFFLINE, OFFLINE_SHORT } from '../lib/offline';
  import { platform } from '../lib/platform';
  import { requestSection } from '../lib/section';
  import { effectiveMuted, playbackStatus, playingName } from '../lib/status';
  import { app, levelControl, run, set } from '../lib/store.svelte';

  const master = levelControl('masterVolume', setMasterVolume);

  const s = $derived(app.state);
  const status = $derived(s ? playbackStatus(s, platform) : null);
  // The switch shows the manual mute or an "outputChanged" auto-mute, like the tray menu's Mute.
  const muted = $derived(s ? effectiveMuted(s) : false);

  function editRules(): void {
    requestSection('apps');
    void run(openSettings());
  }

  let list: HTMLElement | undefined = $state();
  // Only the id: the effect below must not rerun on every state change (a volume drag).
  const packId = $derived(s?.settings.packId ?? null);

  // Keep the selected pack in view (first load, or a change from the menu or settings).
  $effect(() => {
    const id = packId;
    if (!list || !id) return;
    const row = list.querySelector<HTMLElement>(`[data-pack="${CSS.escape(id)}"]`);
    if (!row) return;
    // Scroll the list only (scrollIntoView would also scroll the notices out of view).
    const r = row.getBoundingClientRect();
    const l = list.getBoundingClientRect();
    if (r.top < l.top + 4) list.scrollTop -= l.top + 4 - r.top;
    else if (r.bottom > l.bottom - 4) list.scrollTop += r.bottom - (l.bottom - 4);
  });

  function onKeydown(e: KeyboardEvent): void {
    if (e.key === 'Escape' && !e.defaultPrevented) void hideTray();
  }

  $effect(() => {
    document.title = 'TakTak';
  });
</script>

<svelte:window onkeydown={onKeydown} />

<div class="tray">
  <header class="header">
    <Logo size={30} />
    <div class="title">
      <h1>TakTak</h1>
      {#if s && status}
        <p class="status {status.tone}" aria-live="polite" title={status.label}>
          <span class="dot" aria-hidden="true"></span>
          <span class="status-text">
            {status.label}
            {#if s.playing || s.muted}
              <span class="pack-hint">· {playingName(s)}</span>
            {/if}
          </span>
        </p>
      {:else}
        <p class="status off"><span class="dot" aria-hidden="true"></span>Connecting…</p>
      {/if}
    </div>
    <Switch
      label="Sounds On"
      checked={s?.settings.enabled ?? false}
      disabled={!s}
      onchange={(v) => set('enabled', v, setEnabled)}
    />
  </header>

  {#if s}
    <div class="body">
      <Notices
        state={s}
        compact
        limit={1}
        onmore={() => run(openSettings())}
        onrules={editRules}
      />

      <div class="controls">
        <div class="control">
          <label class="visually-hidden" for="tray-volume">Volume</label>
          <span class="control-icon" title="Volume"><Icon name="speaker-low" /></span>
          <Slider
            id="tray-volume"
            value={s.settings.masterVolume}
            oninput={master.input}
            oncommit={master.commit}
          />
        </div>
        <div class="control">
          <span class="control-icon" class:muted>
            <Icon name={muted ? 'mute' : 'speaker'} />
          </span>
          <span class="control-label" id="tray-mute-label">Mute</span>
          {#if s.settings.muteHotkey}
            <Keycaps accelerator={s.settings.muteHotkey} small />
          {/if}
          <Switch
            small
            labelledby="tray-mute-label"
            checked={muted}
            onchange={(v) => set('muted', v, setMuted)}
          />
        </div>
      </div>

      <section class="packs" aria-labelledby="tray-packs-heading">
        <h2 class="section-label" id="tray-packs-heading">Sound pack</h2>
        <div class="pack-list" role="radiogroup" aria-labelledby="tray-packs-heading" bind:this={list}>
          {#each s.packs as pack (pack.id)}
            {@const selected = pack.id === s.settings.packId}
            <div class="pack-row" class:selected data-pack={pack.id}>
              <label class="pack-choice">
                <input
                  type="radio"
                  name="tray-pack"
                  class="visually-hidden"
                  value={pack.id}
                  checked={selected}
                  onchange={() => set('packId', pack.id, setPack)}
                />
                <span class="radio" aria-hidden="true"></span>
                <span class="pack-name">{pack.name}</span>
                {#if isPersonal(pack)}
                  <span class="badge" title="Imported for personal use">Personal</span>
                {:else if pack.origin === 'user'}
                  <span class="badge">User</span>
                {/if}
              </label>
              <PreviewButton id={pack.id} name={pack.name} />
            </div>
          {:else}
            <p class="empty">No sound packs found. TakTak plays its built-in click.</p>
          {/each}
        </div>
      </section>
    </div>
  {:else}
    <div class="body loading" aria-busy="true"></div>
  {/if}

  <footer class="footer">
    <p class="offline" title={OFFLINE}>
      <Icon name="shield" size={12} /><span>{OFFLINE_SHORT}</span>
    </p>
    <div class="footer-buttons">
      <button type="button" class="btn plain" onclick={() => run(openSettings())}>
        <Icon name="sliders" />Settings…
      </button>
      <button type="button" class="btn plain" aria-label="Quit TakTak" onclick={() => run(quit())}>
        <Icon name="power" />Quit
      </button>
    </div>
  </footer>

  <Toast bottom={72} />
</div>

<style>
  .tray {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--bg-popover);
    --icon-knob: var(--bg-popover);
  }

  .header {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 14px 16px 12px;
  }

  .title {
    flex: 1;
    min-width: 0;
  }

  h1 {
    font-size: 15px;
    font-weight: 650;
    line-height: 1.2;
  }

  .status {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 2px;
    color: var(--text-2);
    font-size: 12px;
    white-space: nowrap;
  }

  .status-text {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .dot {
    flex: none;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--text-3);
  }

  .status.ok .dot {
    background: var(--ok);
  }

  .status.warning .dot {
    background: var(--warning);
  }

  .status.error .dot {
    background: var(--error);
  }

  .pack-hint {
    color: var(--text-3);
  }

  .body {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 0 12px;
    /* Only if every notice shows at once: never let the pack list collapse under the footer. */
    overflow-y: auto;
  }

  .controls {
    display: flex;
    flex-direction: column;
    border-radius: var(--radius-m);
    background: var(--surface);
    box-shadow: var(--card-shadow);
  }

  .control {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 40px;
    padding: 0 12px;
  }

  .control + .control {
    border-top: 0.5px solid var(--separator);
  }

  .control-icon {
    width: 16px;
    color: var(--text-2);
  }

  .control-icon.muted {
    color: var(--accent-text);
  }

  .control-label {
    flex: 1;
    font-weight: 500;
  }

  .packs {
    flex: 1 1 0;
    min-height: 120px;
    display: flex;
    flex-direction: column;
  }

  .section-label {
    margin: 0 0 6px 4px;
    color: var(--text-2);
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.02em;
    text-transform: uppercase;
  }

  .pack-list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 4px;
    border-radius: var(--radius-m);
    background: var(--surface);
    box-shadow: var(--card-shadow);
    overscroll-behavior: contain;
  }

  .pack-row {
    display: flex;
    align-items: center;
    gap: 6px;
    padding-right: 4px;
    border-radius: var(--radius-s);
  }

  .pack-row:hover {
    background: var(--hover);
  }

  .pack-row.selected {
    background: var(--accent-soft);
  }

  .pack-choice {
    /* Holds the visually hidden radio, so focusing it never scrolls anything but the list. */
    position: relative;
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    height: 32px;
    padding: 0 8px;
    border-radius: var(--radius-s);
  }

  .pack-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .selected .pack-name {
    font-weight: 600;
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

  input:focus-visible + .radio {
    box-shadow: var(--focus-ring);
  }

  .empty {
    padding: 12px;
    color: var(--text-2);
    font-size: 12px;
  }

  .footer {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 6px 8px 8px;
    margin-top: 12px;
    border-top: 0.5px solid var(--separator);
  }

  .footer-buttons {
    display: flex;
    justify-content: space-between;
    gap: 8px;
  }

  .footer .btn {
    color: var(--text-2);
  }

  /* The offline promise, short form (src/lib/offline.ts). */
  .offline {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 4px;
    color: var(--text-2);
    font-size: 11px;
    white-space: nowrap;
  }

  .offline span {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .footer .btn:hover {
    color: var(--text);
  }
</style>
