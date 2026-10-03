<script lang="ts">
  // Problems that keep TakTak silent or degraded: auto-mute, permission, audio output, the
  // active pack, per-app rules. `compact` is the tray popover's status lines: short text (full
  // text on hover), same buttons.
  import { openOnboarding, openPermissionSettings, relaunch, setMuted } from '../lib/api';
  import { plural } from '../lib/format';
  import { platform } from '../lib/platform';
  import { type Notice, type NoticeAction, actionLabel, notices } from '../lib/status';
  import { run, set } from '../lib/store.svelte';
  import type { AppState } from '../lib/types';
  import Icon, { type IconName } from './Icon.svelte';

  interface Props {
    state: AppState;
    compact?: boolean;
    /** Show at most this many (most important first); the rest are counted. */
    limit?: number;
    /** Where the counted rest can be seen (e.g. opens Settings). */
    onmore?: () => void;
    /** Notices this view explains itself (e.g. the Apps section and the rules notice). */
    hide?: readonly Notice['id'][];
    /** Shows the per-app rules; without it the rules notice has no button. */
    onrules?: () => void;
  }
  let { state, compact = false, limit = Infinity, onmore, hide = [], onrules }: Props = $props();

  const all = $derived(notices(state, platform).filter((n) => !hide.includes(n.id)));
  const list = $derived(all.slice(0, limit));
  const hidden = $derived(all.length - list.length);
  const ICON: Record<'info' | 'warning' | 'error', IconName> = {
    info: 'info',
    warning: 'warning',
    error: 'error',
  };

  function available(actions: NoticeAction[]): NoticeAction[] {
    return actions.filter((a) => a !== 'rules' || onrules);
  }

  function act(action: NoticeAction): void {
    switch (action) {
      case 'permission':
        void run(openPermissionSettings());
        break;
      case 'guide':
        void run(openOnboarding());
        break;
      case 'relaunch':
        void run(relaunch());
        break;
      case 'unmute':
        void set('muted', false, setMuted);
        break;
      case 'rules':
        onrules?.();
        break;
    }
  }
</script>

{#if list.length > 0}
  <div class="notices" class:compact role="status">
    {#each list as notice (notice.id)}
      <div class="notice {notice.tone}">
        <span class="notice-icon"><Icon name={ICON[notice.tone]} size={compact ? 14 : 16} /></span>
        <div class="notice-body">
          {#if compact}
            <p class="notice-line" title={notice.message}>{notice.line}</p>
          {:else}
            <p class="notice-title">{notice.title}</p>
            <p class="notice-message selectable">{notice.message}</p>
          {/if}
          {#if available(notice.actions).length > 0}
            <div class="notice-actions">
              {#each available(notice.actions) as action, i (action)}
                <button
                  type="button"
                  class="btn small"
                  class:primary={i === 0}
                  onclick={() => act(action)}
                >
                  {actionLabel(action, platform)}
                </button>
              {/each}
            </div>
          {/if}
        </div>
      </div>
    {/each}
    {#if hidden > 0}
      <button type="button" class="more" onclick={onmore} disabled={!onmore}>
        {plural(hidden, 'more issue')} · Show in Settings…
      </button>
    {/if}
  </div>
{/if}

<style>
  .notices {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-bottom: 16px;
  }

  .notices.compact {
    flex: none;
    gap: 4px;
    margin-bottom: 0;
  }

  .notice {
    display: flex;
    gap: 10px;
    padding: 10px 12px;
    border-radius: var(--radius-m);
  }

  .compact .notice {
    gap: 8px;
    padding: 6px 10px;
  }

  .notice.warning {
    background: var(--warning-bg);
  }

  .notice.error {
    background: var(--error-bg);
  }

  .notice.info {
    background: var(--info-bg);
  }

  .notice-icon {
    padding-top: 1px;
  }

  .compact .notice-icon {
    padding-top: 2px;
  }

  .warning .notice-icon {
    color: var(--warning);
  }

  .error .notice-icon {
    color: var(--error);
  }

  .info .notice-icon {
    color: var(--info);
  }

  .notice-body {
    flex: 1;
    min-width: 0;
  }

  .notice-title {
    font-weight: 600;
  }

  .notice-message {
    margin-top: 2px;
    color: var(--text-2);
    font-size: 12px;
    overflow-wrap: anywhere;
  }

  .notice-line {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    overflow: hidden;
    font-size: 12px;
    line-height: 1.35;
    overflow-wrap: anywhere;
  }

  .notice-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: 8px;
  }

  .more {
    align-self: flex-start;
    height: 20px;
    padding: 0 4px;
    border: 0;
    border-radius: 4px;
    background: transparent;
    color: var(--text-2);
    font-size: 11.5px;
  }

  .more:hover:not(:disabled) {
    color: var(--text);
    background: var(--hover);
  }

  .compact .notice-actions {
    margin-top: 6px;
  }
</style>
