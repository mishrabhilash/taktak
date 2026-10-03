<script lang="ts">
  // Records the mute shortcut. Key events are read only while recording, only inside this
  // window, and only to build the accelerator string; nothing else is kept or sent.
  // Layout is fixed (field, buttons, one message line) so nothing moves while recording.
  import { tick } from 'svelte';
  import { formatHeld, recordKey } from '../lib/accelerator';
  import { errorMessage, setMuteHotkey } from '../lib/api';
  import { platform } from '../lib/platform';
  import { apply } from '../lib/store.svelte';
  import Keycaps from './Keycaps.svelte';

  let {
    accelerator,
    labelledby,
    describedby,
  }: { accelerator: string | null; labelledby?: string; describedby?: string } = $props();

  const uid = $props.id();
  const messageId = `${uid}-message`;

  type Mode = 'idle' | 'recording' | 'captured';
  let mode = $state<Mode>('idle');
  /** Modifiers held while recording, e.g. "⌥⇧". */
  let held = $state('');
  let candidate = $state<string | null>(null);
  /** Why a key press was not accepted (shown while recording). */
  let hint = $state<string | null>(null);
  /** The app's rejection of the last save. */
  let error = $state<string | null>(null);
  let busy = $state(false);

  let field: HTMLElement | undefined = $state();
  let recordButton: HTMLButtonElement | undefined = $state();
  let saveButton: HTMLButtonElement | undefined = $state();

  const message = $derived(
    error ??
      (mode === 'recording'
        ? (hint ?? 'Press the keys together. Esc cancels.')
        : mode === 'captured'
          ? 'Save to use this shortcut in every app.'
          : ''),
  );

  function start(): void {
    error = null;
    hint = null;
    held = '';
    candidate = null;
    mode = 'recording';
    void tick().then(() => field?.focus());
  }

  function stop(): void {
    mode = 'idle';
    held = '';
    hint = null;
    candidate = null;
    void tick().then(() => recordButton?.focus());
  }

  function onKeydown(e: KeyboardEvent): void {
    e.preventDefault();
    e.stopPropagation();
    if (e.repeat) return;
    const result = recordKey(e, platform);
    switch (result.kind) {
      case 'modifiers':
        held = result.held;
        hint = null;
        break;
      case 'cancel':
        stop();
        break;
      case 'invalid':
        hint = result.reason;
        break;
      case 'accelerator':
        candidate = result.accelerator;
        held = '';
        hint = null;
        mode = 'captured';
        // The field goes away; keep keyboard focus in the recorder (Return saves).
        void tick().then(() => saveButton?.focus());
        break;
    }
  }

  function onKeyup(e: KeyboardEvent): void {
    e.preventDefault();
    e.stopPropagation();
    held = formatHeld(e, platform);
  }

  // Listen only while recording; capture phase so nothing else in the page sees the keys.
  $effect(() => {
    if (mode !== 'recording') return;
    window.addEventListener('keydown', onKeydown, true);
    window.addEventListener('keyup', onKeyup, true);
    window.addEventListener('blur', stop);
    return () => {
      window.removeEventListener('keydown', onKeydown, true);
      window.removeEventListener('keyup', onKeyup, true);
      window.removeEventListener('blur', stop);
    };
  });

  async function save(value: string | null): Promise<void> {
    busy = true;
    error = null;
    try {
      await apply(() => setMuteHotkey(value));
    } catch (e) {
      // Back to the shortcut in use; the message says why (e.g. taken by the system).
      error = errorMessage(e);
    } finally {
      busy = false;
      stop();
    }
  }
</script>

<div class="recorder">
  <div class="line">
    {#if mode === 'recording'}
      <div
        class="field recording"
        tabindex="-1"
        role="textbox"
        aria-readonly="true"
        aria-labelledby={labelledby}
        aria-describedby={messageId}
        bind:this={field}
      >
        {#if held}
          <span class="held">{held}</span>
        {:else}
          <span class="placeholder">Type shortcut…</span>
        {/if}
      </div>
      <button type="button" class="btn" onclick={stop}>Cancel</button>
    {:else if mode === 'captured' && candidate}
      <div class="field captured"><Keycaps accelerator={candidate} /></div>
      <button
        type="button"
        class="btn primary"
        disabled={busy}
        onclick={() => save(candidate)}
        bind:this={saveButton}
      >
        Save
      </button>
      <button type="button" class="btn" disabled={busy} onclick={stop}>Cancel</button>
    {:else}
      <div class="field" aria-labelledby={labelledby} role="group">
        {#if accelerator}
          <Keycaps {accelerator} />
        {:else}
          <span class="placeholder">None</span>
        {/if}
      </div>
      <button
        type="button"
        class="btn"
        aria-describedby={describedby}
        onclick={start}
        bind:this={recordButton}
      >
        Record
      </button>
      <button type="button" class="btn" disabled={!accelerator || busy} onclick={() => save(null)}>
        Clear
      </button>
    {/if}
  </div>
  {#if error}
    <p class="message error" id={messageId} role="alert">{message}</p>
  {:else}
    <p class="message" id={messageId} aria-live="polite">{message}</p>
  {/if}
</div>

<style>
  .recorder {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .line {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .field {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: none;
    width: 180px;
    height: 32px;
    padding: 0 10px;
    border-radius: var(--radius-s);
    background: var(--surface-2);
  }

  .field.recording {
    background: var(--surface);
    box-shadow:
      inset 0 0 0 1.5px var(--accent),
      var(--focus-ring);
  }

  .field.captured {
    background: var(--accent-soft);
  }

  .placeholder {
    color: var(--text-3);
  }

  .recording .placeholder {
    animation: blink 1.2s ease-in-out infinite;
  }

  .held {
    font-size: 15px;
    letter-spacing: 0.08em;
  }

  .message {
    min-height: 1.35em;
    color: var(--text-2);
    font-size: 12px;
    line-height: 1.35;
  }

  .message.error {
    color: var(--error-text);
  }

  @keyframes blink {
    50% {
      opacity: 0.45;
    }
  }
</style>
