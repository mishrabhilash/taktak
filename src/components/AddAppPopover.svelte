<script lang="ts">
  // "Add App…": the running apps to tick on or off (with a search box) and "Choose App…" for any
  // other installed app. The running list is fetched each time the popover opens and dropped when
  // it closes; only the icons stay, in the in-memory cache.
  import { tick } from 'svelte';
  import { chooseApp, errorMessage, listRunningApps } from '../lib/api';
  import { rememberIcons } from '../lib/icons.svelte';
  import { isListed, searchApps } from '../lib/rules';
  import { showError } from '../lib/store.svelte';
  import type { AppInfo, AppRule } from '../lib/types';
  import AppIcon from './AppIcon.svelte';
  import Icon from './Icon.svelte';

  interface Props {
    rule: AppRule;
    disabled?: boolean;
    onadd: (app: AppInfo) => void;
    onremove: (id: string) => void;
  }
  let { rule, disabled = false, onadd, onremove }: Props = $props();

  const uid = $props.id();

  let open = $state(false);
  /** The running apps; null while loading. */
  let apps = $state.raw<AppInfo[] | null>(null);
  let loadError = $state<string | null>(null);
  let query = $state('');
  let choosing = $state(false);

  let trigger: HTMLButtonElement | undefined = $state();
  let panel: HTMLElement | undefined = $state();
  let search: HTMLInputElement | undefined = $state();

  const shown = $derived(apps ? searchApps(apps, query) : []);

  async function show(): Promise<void> {
    open = true;
    query = '';
    apps = null;
    loadError = null;
    await tick();
    search?.focus();
    panel?.scrollIntoView({ block: 'nearest' });
    try {
      const running = await listRunningApps();
      rememberIcons(running);
      if (open) apps = running;
    } catch (e) {
      if (open) loadError = errorMessage(e);
    }
  }

  function hide(returnFocus = true): void {
    open = false;
    apps = null;
    if (returnFocus) void tick().then(() => trigger?.focus());
  }

  function toggle(app: AppInfo): void {
    if (isListed(rule, app.id)) onremove(app.id);
    else onadd(app);
  }

  async function choose(): Promise<void> {
    // Not disabled meanwhile: a disabled button would drop the keyboard focus out of the popover.
    if (choosing) return;
    choosing = true;
    try {
      const app = await chooseApp();
      if (app) {
        rememberIcons([app]);
        onadd(app);
      }
    } catch (e) {
      showError(e);
    } finally {
      choosing = false;
    }
  }

  // A click anywhere else closes it (the native app picker is a separate window: no click here).
  $effect(() => {
    if (!open) return;
    const onPointerDown = (e: PointerEvent): void => {
      const target = e.target as Node | null;
      if (target && (panel?.contains(target) || trigger?.contains(target))) return;
      hide(false);
    };
    document.addEventListener('pointerdown', onPointerDown, true);
    return () => document.removeEventListener('pointerdown', onPointerDown, true);
  });

  // Escape closes it wherever the focus is (also after the native picker hands it back).
  function onWindowKeydown(e: KeyboardEvent): void {
    if (!open || e.key !== 'Escape' || e.defaultPrevented) return;
    e.preventDefault();
    hide();
  }

  function onKeydown(e: KeyboardEvent): void {
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
    const options = [...(panel?.querySelectorAll<HTMLElement>('.option') ?? [])];
    if (options.length === 0) return;
    e.preventDefault();
    const index = options.indexOf(document.activeElement as HTMLElement);
    const next =
      e.key === 'ArrowDown'
        ? index < 0 || index === options.length - 1
          ? 0
          : index + 1
        : index <= 0
          ? options.length - 1
          : index - 1;
    options[next]?.focus();
  }

  function onSearchKeydown(e: KeyboardEvent): void {
    // Return in the search box ticks the only match.
    if (e.key === 'Enter' && shown.length === 1 && shown[0]) {
      e.preventDefault();
      toggle(shown[0]);
    }
  }
</script>

<svelte:window onkeydown={onWindowKeydown} />

<div class="add-app">
  <button
    type="button"
    class="btn"
    data-add-app
    aria-haspopup="dialog"
    aria-expanded={open}
    aria-controls={open ? `${uid}-panel` : undefined}
    {disabled}
    bind:this={trigger}
    onclick={() => (open ? hide() : void show())}
  >
    <Icon name="plus" size={14} />Add App…
  </button>

  {#if open}
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="panel"
      id="{uid}-panel"
      role="dialog"
      aria-label="Add apps"
      tabindex="-1"
      bind:this={panel}
      onkeydown={onKeydown}
    >
      <div class="search">
        <Icon name="search" size={14} />
        <input
          type="search"
          placeholder="Search running apps"
          aria-label="Search running apps"
          aria-controls="{uid}-list"
          autocomplete="off"
          spellcheck="false"
          bind:value={query}
          bind:this={search}
          onkeydown={onSearchKeydown}
        />
      </div>

      <p class="caption" id="{uid}-caption">Running apps</p>
      <div class="list-wrap">
        {#if loadError}
          <p class="state error" role="alert">{loadError}</p>
        {:else if apps === null}
          <p class="state" aria-busy="true">Looking for running apps…</p>
        {:else if apps.length === 0}
          <p class="state">No other apps are running. Use Choose App… to pick any app.</p>
        {:else if shown.length === 0}
          <p class="state">No running app matches “{query}”. Choose App… finds any installed app.</p>
        {:else}
          <ul class="list" id="{uid}-list" aria-labelledby="{uid}-caption">
            {#each shown as option (option.id)}
              {@const listed = isListed(rule, option.id)}
              <li>
                <button
                  type="button"
                  class="option"
                  class:listed
                  aria-pressed={listed}
                  onclick={() => toggle(option)}
                >
                  <AppIcon id={option.id} src={option.iconDataUrl} size={22} />
                  <span class="option-name">{option.name}</span>
                  <span class="check" aria-hidden="true">
                    {#if listed}<Icon name="check" size={14} />{/if}
                  </span>
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      </div>

      <div class="panel-footer">
        <button type="button" class="btn small" aria-busy={choosing} onclick={choose}>
          {choosing ? 'Choosing…' : 'Choose App…'}
        </button>
        <button type="button" class="btn small primary" onclick={() => hide()}>Done</button>
      </div>
    </div>
  {/if}
</div>

<style>
  .add-app {
    position: relative;
    flex: none;
  }

  .panel {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    z-index: 20;
    display: flex;
    flex-direction: column;
    width: 320px;
    max-width: calc(100vw - 220px);
    min-width: 260px;
    padding: 8px;
    border-radius: var(--radius-l);
    background: var(--surface);
    box-shadow:
      0 0 0 0.5px var(--border),
      0 10px 32px rgba(0, 0, 0, 0.22);
    animation: drop 0.14s ease-out;
  }

  .search {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 30px;
    padding: 0 8px;
    border-radius: var(--radius-s);
    background: var(--surface-2);
    color: var(--text-3);
  }

  .search:focus-within {
    box-shadow: var(--focus-ring);
  }

  .search input {
    flex: 1;
    min-width: 0;
    height: 100%;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--text);
    font-size: 13px;
  }

  .search input:focus-visible {
    box-shadow: none;
  }

  .caption {
    margin: 10px 6px 4px;
    color: var(--text-2);
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.02em;
    text-transform: uppercase;
  }

  .list-wrap {
    max-height: 264px;
    min-height: 72px;
    overflow-y: auto;
    overscroll-behavior: contain;
  }

  .option {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    height: 32px;
    padding: 0 8px;
    border: 0;
    border-radius: var(--radius-s);
    background: transparent;
    text-align: left;
  }

  .option:hover {
    background: var(--hover);
  }

  .option.listed {
    background: var(--accent-soft);
  }

  .option-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .listed .option-name {
    font-weight: 600;
  }

  .check {
    flex: none;
    width: 14px;
    color: var(--accent-text);
  }

  .state {
    padding: 12px 8px;
    color: var(--text-2);
    font-size: 12px;
  }

  .state.error {
    color: var(--error-text);
  }

  .panel-footer {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    margin-top: 8px;
    padding-top: 8px;
    border-top: 0.5px solid var(--separator);
  }

  @keyframes drop {
    from {
      transform: translateY(-4px);
      opacity: 0;
    }
  }
</style>
