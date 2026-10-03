<script lang="ts">
  import { isTauri, windowLabel } from './lib/api';
  import { watchKeyboardLayout } from './lib/keyboard.svelte';
  import { connect } from './lib/store.svelte';
  import OnboardingView from './views/OnboardingView.svelte';
  import SettingsView from './views/SettingsView.svelte';
  import TrayView from './views/TrayView.svelte';

  // Every window loads this bundle; the label decides the view (docs/ui-contract.md).
  const label = windowLabel();
  void connect();
  // Labels shortcut keys with the layout's characters where the webview can tell (macOS).
  watchKeyboardLayout();

  // A native app has no "Reload / Inspect Element" menu. Keep the system menu where it helps:
  // on text meant to be copied.
  function onContextMenu(e: MouseEvent): void {
    if (!isTauri || !import.meta.env.PROD) return;
    if (e.target instanceof Element && e.target.closest('input, .selectable')) return;
    e.preventDefault();
  }
</script>

<svelte:document oncontextmenu={onContextMenu} />

{#if label === 'tray'}
  <TrayView />
{:else if label === 'onboarding'}
  <OnboardingView />
{:else}
  <SettingsView />
{/if}
