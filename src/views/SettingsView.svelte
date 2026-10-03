<script lang="ts" module>
  import type { IconName } from '../components/Icon.svelte';

  const SECTIONS = [
    { id: 'sounds', label: 'Sounds', icon: 'waveform' },
    { id: 'volume', label: 'Volume', icon: 'speaker' },
    { id: 'feel', label: 'Feel', icon: 'wave' },
    { id: 'apps', label: 'Apps', icon: 'apps' },
    { id: 'shortcuts', label: 'Shortcuts', icon: 'keyboard' },
    { id: 'general', label: 'General', icon: 'toggle' },
    { id: 'about', label: 'About', icon: 'info' },
  ] as const satisfies readonly { id: string; label: string; icon: IconName }[];

  type SectionId = (typeof SECTIONS)[number]['id'];

  function sectionId(id: string | null): SectionId | null {
    return SECTIONS.find((s) => s.id === id)?.id ?? null;
  }

  function fromHash(): SectionId {
    return sectionId(window.location.hash.slice(1)) ?? 'sounds';
  }
</script>

<script lang="ts">
  // The settings window: a sidebar of sections (a vertical tab list) and the selected one.
  // The section is mirrored in the URL hash (#about) so a reload keeps it; the popover can ask
  // for one when it opens Settings ("Edit rules…", see section.ts).
  import { tick, untrack } from 'svelte';
  import Icon from '../components/Icon.svelte';
  import Logo from '../components/Logo.svelte';
  import Notices from '../components/Notices.svelte';
  import Toast from '../components/Toast.svelte';
  import { platform } from '../lib/platform';
  import { onSectionRequest, takeRequestedSection } from '../lib/section';
  import { playbackStatus } from '../lib/status';
  import { app } from '../lib/store.svelte';
  import AboutSection from './settings/AboutSection.svelte';
  import AppsSection from './settings/AppsSection.svelte';
  import FeelSection from './settings/FeelSection.svelte';
  import GeneralSection from './settings/GeneralSection.svelte';
  import ShortcutsSection from './settings/ShortcutsSection.svelte';
  import SoundsSection from './settings/SoundsSection.svelte';
  import VolumeSection from './settings/VolumeSection.svelte';

  let current = $state<SectionId>(sectionId(takeRequestedSection()) ?? fromHash());
  let content: HTMLElement | undefined = $state();

  const s = $derived(app.state);
  const status = $derived(s ? playbackStatus(s, platform) : null);

  function select(id: SectionId, focus = false): void {
    if (id !== current) {
      current = id;
      history.replaceState(null, '', `#${id}`);
      if (content) content.scrollTop = 0;
    }
    if (focus) void tick().then(() => document.getElementById(`tab-${id}`)?.focus());
  }

  function onTabKeydown(e: KeyboardEvent): void {
    const index = SECTIONS.findIndex((x) => x.id === current);
    const last = SECTIONS.length - 1;
    const next =
      e.key === 'ArrowDown'
        ? index === last
          ? 0
          : index + 1
        : e.key === 'ArrowUp'
          ? index === 0
            ? last
            : index - 1
          : e.key === 'Home'
            ? 0
            : e.key === 'End'
              ? last
              : -1;
    if (next < 0) return;
    e.preventDefault();
    const section = SECTIONS[next];
    if (section) select(section.id, true);
  }

  $effect(() => {
    document.title = 'TakTak Settings';
    // A section the popover asked for goes into the hash too, so a reload keeps it.
    const shown = untrack(() => current);
    if (window.location.hash.slice(1) !== shown) history.replaceState(null, '', `#${shown}`);
    const onHash = () => select(fromHash());
    window.addEventListener('hashchange', onHash);
    const stopRequests = onSectionRequest((id) => {
      const section = sectionId(id);
      if (section) select(section);
    });
    return () => {
      window.removeEventListener('hashchange', onHash);
      stopRequests();
    };
  });
</script>

<div class="settings">
  <nav class="sidebar" aria-label="Settings">
    <div class="brand">
      <Logo size={24} />
      <span>TakTak</span>
    </div>
    <div class="tabs" role="tablist" aria-orientation="vertical" tabindex="-1" onkeydown={onTabKeydown}>
      {#each SECTIONS as section (section.id)}
        <button
          type="button"
          role="tab"
          class="tab"
          id="tab-{section.id}"
          aria-selected={current === section.id}
          aria-controls="panel"
          tabindex={current === section.id ? 0 : -1}
          onclick={() => select(section.id)}
        >
          <span class="tab-icon"><Icon name={section.icon} /></span>
          {section.label}
        </button>
      {/each}
    </div>
    {#if status}
      <p class="sidebar-status {status.tone}" aria-live="polite">
        <span class="dot" aria-hidden="true"></span>{status.label}
      </p>
    {/if}
  </nav>

  <main class="content" bind:this={content}>
    <div class="content-inner" id="panel" role="tabpanel" aria-labelledby="tab-{current}">
      {#if s}
        {#if current !== 'about'}
          <Notices
            state={s}
            hide={current === 'apps' ? ['rules'] : []}
            onrules={() => select('apps')}
          />
        {/if}
        {#if current === 'sounds'}
          <SoundsSection {s} />
        {:else if current === 'volume'}
          <VolumeSection {s} />
        {:else if current === 'feel'}
          <FeelSection {s} />
        {:else if current === 'apps'}
          <AppsSection {s} />
        {:else if current === 'shortcuts'}
          <ShortcutsSection {s} />
        {:else if current === 'general'}
          <GeneralSection {s} />
        {:else}
          <AboutSection {s} />
        {/if}
      {:else if !app.error}
        <p class="loading" aria-busy="true">Loading…</p>
      {/if}
    </div>
  </main>

  <Toast />
</div>

<style>
  .settings {
    display: flex;
    height: 100%;
  }

  .sidebar {
    flex: none;
    display: flex;
    flex-direction: column;
    width: 176px;
    padding: 16px 10px 12px;
    background: var(--bg-sidebar);
    border-right: 0.5px solid var(--separator);
    --icon-knob: var(--bg-sidebar);
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 8px 16px;
    font-size: 15px;
    font-weight: 650;
  }

  .tabs {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .tab {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 30px;
    padding: 0 8px;
    border: 0;
    border-radius: var(--radius-s);
    background: transparent;
    font-size: 13px;
    text-align: left;
  }

  .tab:hover {
    background: var(--hover);
  }

  .tab[aria-selected='true'] {
    background: var(--selected);
    font-weight: 600;
  }

  .tab-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    border-radius: 5px;
    background: var(--surface);
    color: var(--text-2);
    box-shadow: var(--card-shadow);
    --icon-knob: var(--surface);
  }

  .tab[aria-selected='true'] .tab-icon {
    background: var(--accent);
    color: #fff;
    box-shadow: none;
    --icon-knob: var(--accent);
  }

  .sidebar-status {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: auto;
    padding: 8px;
    color: var(--text-2);
    font-size: 12px;
  }

  .dot {
    flex: none;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--text-3);
  }

  .ok .dot {
    background: var(--ok);
  }

  .warning .dot {
    background: var(--warning);
  }

  .error .dot {
    background: var(--error);
  }

  .content {
    flex: 1;
    min-width: 0;
    overflow-y: auto;
    background: var(--bg);
  }

  .content-inner {
    max-width: 760px;
    margin: 0 auto;
    padding: 24px 24px 72px;
  }

  @media (max-width: 680px) {
    .content-inner {
      padding: 20px 20px 72px;
    }
  }

  .loading {
    color: var(--text-2);
  }
</style>
