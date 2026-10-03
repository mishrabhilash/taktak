<script lang="ts">
  // The last command error, floating over the bottom of the window (no layout shift).
  import { app, dismissError } from '../lib/store.svelte';
  import Icon from './Icon.svelte';

  let { bottom = 12 }: { bottom?: number } = $props();
</script>

<div class="toast-region" style:bottom="{bottom}px" aria-live="assertive">
  {#if app.error}
    <div class="toast" role="alert">
      <span class="toast-icon"><Icon name="error" size={14} /></span>
      <p class="toast-text selectable">{app.error}</p>
      <button type="button" class="dismiss" aria-label="Dismiss" onclick={dismissError}>
        <Icon name="close" size={12} />
      </button>
    </div>
  {/if}
</div>

<style>
  .toast-region {
    position: fixed;
    left: 12px;
    right: 12px;
    z-index: 10;
    display: flex;
    justify-content: center;
    pointer-events: none;
  }

  .toast {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    max-width: 480px;
    padding: 8px 8px 8px 12px;
    border-radius: var(--radius-m);
    background: var(--surface);
    box-shadow:
      0 0 0 0.5px var(--border),
      0 6px 20px rgba(0, 0, 0, 0.18);
    pointer-events: auto;
    animation: rise 0.18s ease-out;
  }

  .toast-icon {
    padding-top: 2px;
    color: var(--error);
  }

  .toast-text {
    flex: 1;
    font-size: 12px;
    overflow-wrap: anywhere;
  }

  .dismiss {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    padding: 0;
    border: 0;
    border-radius: 4px;
    background: transparent;
    color: var(--text-2);
  }

  .dismiss:hover {
    background: var(--hover);
  }

  @keyframes rise {
    from {
      transform: translateY(6px);
      opacity: 0;
    }
  }
</style>
