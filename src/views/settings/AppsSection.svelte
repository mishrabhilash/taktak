<script lang="ts">
  // Per-app rules: where TakTak plays (everywhere, only in listed apps, never in listed apps),
  // the list, and what that means for the app in front right now. That app is the one the user
  // came from (TakTak's own windows don't count); only its current value is ever shown, and the
  // rule list is the only thing saved.
  import { tick } from 'svelte';
  import AddAppPopover from '../../components/AddAppPopover.svelte';
  import AppIcon from '../../components/AppIcon.svelte';
  import Icon from '../../components/Icon.svelte';
  import { addRuleApp, removeRuleApp, setAppRuleMode } from '../../lib/api';
  import { frontmostIcon, loadIcons } from '../../lib/icons.svelte';
  import {
    RULE_MODES,
    frontmostSummary,
    isListed,
    ruleAppProblem,
    withApp,
    withMode,
    withoutApp,
  } from '../../lib/rules';
  import { set, showError } from '../../lib/store.svelte';
  import type { AppRef, AppRuleMode, AppState } from '../../lib/types';

  let { s }: { s: AppState } = $props();

  const rule = $derived(s.settings.appRule);
  const supported = $derived(s.rulesSupported);
  const front = $derived(frontmostSummary(s));
  const frontApp = $derived(s.frontmostApp);
  const emptyOnly = $derived(rule.mode === 'only' && rule.apps.length === 0);

  // Icons of listed apps that are not running. Keyed on the ids only, so a volume drag (a new
  // state) does not ask again.
  const listedIds = $derived(rule.apps.map((a) => a.id).join('\n'));
  $effect(() => {
    if (supported && listedIds) void loadIcons(listedIds.split('\n'));
  });

  // The app in front shows an icon only when one is known already (listed, picked, or among the
  // running apps offered by "Add app"); otherwise the generic one. It is never looked up: the
  // app keeps every icon it renders for the session, so fetching the frontmost app's icon would
  // build up a list of the apps the user visited.
  const frontIcon = $derived(frontApp ? frontmostIcon(frontApp.id) : null);

  /** A listed app the user tried to add again: shown briefly. */
  let flash = $state<string | null>(null);
  let flashTimer: ReturnType<typeof setTimeout> | undefined;

  function showListed(id: string): void {
    clearTimeout(flashTimer);
    flash = id;
    void tick().then(() =>
      document.querySelector(`[data-app="${CSS.escape(id)}"]`)?.scrollIntoView({ block: 'nearest' }),
    );
    flashTimer = setTimeout(() => (flash = null), 1400);
  }

  function setMode(mode: AppRuleMode): void {
    void set('appRule', withMode(rule, mode), () => setAppRuleMode(mode));
  }

  function add(app: AppRef): void {
    const problem = ruleAppProblem(rule, app.id);
    if (problem) {
      showError(problem);
      return;
    }
    if (isListed(rule, app.id.trim())) {
      showListed(app.id.trim());
      return;
    }
    void set('appRule', withApp(rule, app), () => addRuleApp(app));
  }

  function remove(id: string, moveFocus = false): void {
    if (moveFocus) {
      // Keep the keyboard in the list: the next row's button, else the previous, else "Add".
      const index = rule.apps.findIndex((a) => a.id === id);
      const next = rule.apps[index + 1] ?? rule.apps[index - 1];
      void tick().then(() => {
        const target = next
          ? document.querySelector<HTMLElement>(`[data-remove="${CSS.escape(next.id)}"]`)
          : document.querySelector<HTMLElement>('[data-add-app]');
        target?.focus();
      });
    }
    void set('appRule', withoutApp(rule, id), () => removeRuleApp(id));
  }

  const emptyHint = $derived(
    rule.mode === 'only'
      ? 'Add the apps where TakTak should play.'
      : rule.mode === 'never'
        ? 'Add the apps where TakTak should stay silent, like video calls or games.'
        : 'Add apps here, then choose “Only” or “Never” above to use the list.',
  );
</script>

<header class="section-header">
  <h1>Apps</h1>
  <p>Choose the apps where TakTak plays, or the ones where it stays silent.</p>
</header>

{#if !supported}
  <div class="group unsupported" role="note">
    <span class="unsupported-icon"><Icon name="info" size={18} /></span>
    <div>
      <p class="row-label">Per-app rules aren’t available on this system yet.</p>
      <p class="hint">
        They work on macOS for now. TakTak plays in every app here, and any apps listed below are
        kept for later.
      </p>
    </div>
  </div>
{:else if front}
  <div class="group now" class:silent={front.silent} aria-live="polite">
    <div class="row now-row">
      {#if frontApp}
        <AppIcon
          id={frontApp.id}
          src={frontIcon}
          size={32}
        />
      {:else}
        <span class="now-icon"><Icon name="apps" size={20} /></span>
      {/if}
      <div class="row-text">
        <span class="caption">Current app</span>
        <span class="now-text">{front.text}</span>
      </div>
      {#if frontApp && front.canAdd && rule.mode !== 'everywhere'}
        <button
          type="button"
          class="btn"
          aria-label="Add {frontApp.name} to the list"
          onclick={() => add(frontApp)}
        >
          <Icon name="plus" size={14} />Add {frontApp.name}
        </button>
      {/if}
    </div>
  </div>
{/if}

<h2 class="group-title" id="apps-mode-title">Where TakTak plays</h2>
<div class="group" role="radiogroup" aria-labelledby="apps-mode-title">
  {#each RULE_MODES as m (m.mode)}
    {@const selected = rule.mode === m.mode}
    <label class="row mode" class:selected class:disabled={!supported}>
      <input
        type="radio"
        name="app-rule-mode"
        class="visually-hidden"
        value={m.mode}
        checked={selected}
        disabled={!supported}
        aria-describedby="apps-mode-{m.mode}-hint"
        onchange={() => setMode(m.mode)}
      />
      <span class="radio" aria-hidden="true"></span>
      <span class="row-text">
        <span class="row-label">{m.label}</span>
        <span class="hint" id="apps-mode-{m.mode}-hint">{m.hint}</span>
      </span>
    </label>
  {/each}
</div>

<div class="list-head">
  <h2 class="group-title" id="apps-list-title">Your apps</h2>
  <AddAppPopover {rule} disabled={!supported} onadd={add} onremove={(id) => remove(id)} />
</div>

{#if emptyOnly}
  <p class="warning-line" role="status">
    <Icon name="warning" size={14} />No apps listed: TakTak is silent everywhere.
  </p>
{/if}

{#if rule.apps.length > 0}
  <ul class="group app-list" class:unused={rule.mode === 'everywhere'} aria-labelledby="apps-list-title">
    {#each rule.apps as entry (entry.id)}
      <li class="row app-row" class:flash={flash === entry.id} data-app={entry.id}>
        <AppIcon id={entry.id} size={28} />
        <div class="row-text">
          <span class="row-label app-name">{entry.name}</span>
          <span class="hint mono selectable app-id">{entry.id}</span>
        </div>
        <button
          type="button"
          class="remove"
          aria-label="Remove {entry.name}"
          title="Remove"
          data-remove={entry.id}
          disabled={!supported}
          onclick={() => remove(entry.id, true)}
        >
          <Icon name="close" size={12} />
        </button>
      </li>
    {/each}
  </ul>
  {#if rule.mode === 'everywhere'}
    <p class="footnote">Choose “Only in these apps” or “Never in these apps” to use this list.</p>
  {/if}
{:else}
  <div class="group empty">
    <span class="empty-icon"><Icon name="apps" size={20} /></span>
    <div>
      <p class="row-label">No apps listed yet</p>
      <p class="hint">{emptyHint}</p>
    </div>
  </div>
{/if}

<h2 class="group-title">Good to know</h2>
<ul class="group limits">
  {#if supported}
    <li class="row limit">
      <span class="limit-icon"><Icon name="search" size={14} /></span>
      <span class="hint">
        Spotlight, Raycast and Alfred don’t count as the app in front: typing in them follows the
        rules of the app you were using.
      </span>
    </li>
    <li class="row limit">
      <span class="limit-icon"><Icon name="lock" size={14} /></span>
      <span class="hint">
        Password fields are always silent, whatever the rules: macOS hides those keystrokes from
        every app (Secure Input).
      </span>
    </li>
  {/if}
  <li class="row limit">
    <span class="limit-icon"><Icon name="shield" size={14} /></span>
    <span class="hint">
      TakTak only looks at which app is in front right now. It keeps no history of the apps you
      use; only this list is saved.
    </span>
  </li>
</ul>

<style>
  .unsupported,
  .empty {
    display: flex;
    gap: 12px;
    padding: 14px;
  }

  .unsupported-icon {
    color: var(--info);
  }

  .empty-icon {
    color: var(--text-3);
  }

  .now-row {
    gap: 12px;
  }

  .now.silent {
    box-shadow:
      var(--card-shadow),
      inset 3px 0 0 var(--text-3);
  }

  .now-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    color: var(--text-3);
  }

  .caption {
    display: block;
    color: var(--text-2);
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.02em;
    text-transform: uppercase;
  }

  .now-text {
    display: block;
    margin-top: 1px;
    font-weight: 500;
  }

  .mode {
    gap: 10px;
    align-items: flex-start;
    /* Holds the visually hidden radio. */
    position: relative;
  }

  .mode.disabled {
    opacity: 0.55;
  }

  .radio {
    flex: none;
    position: relative;
    width: 16px;
    height: 16px;
    margin-top: 1px;
    border-radius: 50%;
    background: var(--surface);
    box-shadow: inset 0 0 0 1px var(--border);
  }

  .mode.selected .radio {
    background: var(--accent);
    box-shadow: none;
  }

  .mode.selected .radio::after {
    content: '';
    position: absolute;
    inset: 5px;
    border-radius: 50%;
    background: #fff;
  }

  input:focus-visible + .radio {
    box-shadow: var(--focus-ring);
  }

  .list-head {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: 12px;
    margin-top: 24px;
  }

  .list-head .group-title {
    margin-top: 0;
  }

  .list-head + .group,
  .list-head + .warning-line {
    margin-top: 8px;
  }

  .warning-line {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0 0 8px;
    padding: 8px 12px;
    border-radius: var(--radius-m);
    background: var(--warning-bg);
    color: var(--warning-text);
    font-size: 12px;
    font-weight: 500;
  }

  .warning-line + .group {
    margin-top: 0;
  }

  .app-list.unused .app-row {
    opacity: 0.6;
  }

  .app-row {
    min-height: 48px;
    padding: 8px 12px;
    transition: background-color 0.3s;
  }

  .app-row.flash {
    background: var(--accent-soft);
  }

  .app-name,
  .app-id {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .app-id {
    font-size: 11px;
  }

  .remove {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: var(--surface-2);
    color: var(--text-2);
  }

  .remove:hover:not(:disabled) {
    background: var(--error-bg);
    color: var(--error-text);
  }

  .remove:disabled {
    opacity: 0.45;
  }

  .footnote {
    margin: 8px 4px 0;
    color: var(--text-2);
    font-size: 12px;
  }

  .limit {
    align-items: flex-start;
    min-height: 0;
  }

  .limit .hint {
    margin-top: 0;
  }

  .limit-icon {
    padding-top: 1px;
    color: var(--text-3);
  }
</style>
