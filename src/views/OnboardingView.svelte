<script lang="ts">
  // The "Welcome to TakTak" window, kept minimal: the keycap, one line, the offline badge, one big
  // button and a live status. Everything else (why, privacy details, Quit & Reopen, adding
  // TakTak by hand, the stale-entry fix) sits behind the small "Why?" and "Having trouble?"
  // links, collapsed until asked for (or until the troubleshooting is clearly needed). Linux
  // shows the `input` group opt-in and its cost instead; Windows and Linux lead with Quit &
  // Reopen when the key listener failed. The steps and words are in onboarding.ts; nothing here
  // reads what the user types.
  import Icon from '../components/Icon.svelte';
  import Logo from '../components/Logo.svelte';
  import Toast from '../components/Toast.svelte';
  import { finishOnboarding, openPermissionSettings, relaunch, revealApp } from '../lib/api';
  import { copyText } from '../lib/clipboard';
  import {
    COPY,
    type OnboardingFacts,
    type OnboardingPhase,
    TCC_RESET_COMMAND,
    justGranted,
    onboardingLine,
    onboardingStatus,
    onboardingStep,
    troubleshootDelay,
  } from '../lib/onboarding';
  import { OFFLINE, OFFLINE_BADGE } from '../lib/offline';
  import { platform } from '../lib/platform';
  import { INPUT_GROUP_COMMAND, INPUT_GROUP_COST, INPUT_GROUP_WHY, UNAVAILABLE } from '../lib/status';
  import { app, apply, run, showError } from '../lib/store.svelte';

  const s = $derived(app.state);

  /** When the user pressed "Allow Input Monitoring" here. */
  let openedAt = $state<number | null>(null);
  /** What that press answered: TakTak is in the Input Monitoring list (null: no answer yet). */
  let listed = $state<boolean | null>(null);
  /** The clock as of the last re-evaluation (a one-shot timer moves it, nothing polls). */
  let now = $state(Date.now());

  const facts = $derived<OnboardingFacts | null>(
    s
      ? {
          permission: s.permission,
          permissionRequired: s.onboarding.permissionRequired,
          relaunchSuggested: s.onboarding.relaunchSuggested,
          inputGroupNeeded: s.onboarding.inputGroupNeeded,
          openedAt,
          listed,
          now,
        }
      : null,
  );
  const step = $derived(facts ? onboardingStep(facts) : null);
  const status = $derived(facts ? onboardingStatus(facts) : null);
  const line = $derived(facts ? onboardingLine(facts) : '');
  const phase = $derived(step?.phase ?? null);

  // Open "Having trouble?" by itself once the user has waited a while.
  $effect(() => {
    if (!facts) return;
    const delay = troubleshootDelay({ ...facts, now: Date.now() });
    if (delay === null) return;
    const timer = setTimeout(() => (now = Date.now()), delay + 50);
    return () => clearTimeout(timer);
  });

  let whyOpen = $state(false);
  let troubleOpen = $state(false);
  $effect(() => {
    if (step?.troubleshoot) troubleOpen = true;
  });

  // A short celebration when the permission comes through while the window is open.
  let previous: OnboardingPhase | null = null;
  let celebrate = $state(false);
  $effect(() => {
    if (phase === null) return;
    if (justGranted(previous, phase)) celebrate = true;
    previous = phase;
  });

  let asking = $state(false);
  let relaunching = $state(false);
  let closing = $state(false);
  /** The command whose Copy button was pressed last, for 2 s. */
  let copied = $state<string | null>(null);
  let copiedTimer: ReturnType<typeof setTimeout> | undefined;

  // A relaunch that did not happen (or the mock, which stays) frees the button again.
  $effect(() => {
    if (!relaunching) return;
    if (phase === 'granted') {
      relaunching = false;
      return;
    }
    const timer = setTimeout(() => (relaunching = false), 10_000);
    return () => clearTimeout(timer);
  });

  async function allow(): Promise<void> {
    asking = true;
    openedAt = Date.now();
    now = openedAt;
    try {
      // Takes up to a couple of seconds while macOS adds TakTak to the list.
      const answer = await run(openPermissionSettings());
      if (answer !== undefined) listed = answer;
    } finally {
      asking = false;
    }
  }

  async function reopen(): Promise<void> {
    relaunching = true;
    try {
      await relaunch();
    } catch (e) {
      relaunching = false;
      showError(e);
    }
  }

  async function close(): Promise<void> {
    closing = true;
    try {
      // The app closes this window; in a browser the page just stays.
      await apply(finishOnboarding);
    } catch (e) {
      showError(e);
    } finally {
      closing = false;
    }
  }

  async function copyCommand(command: string): Promise<void> {
    if (await copyText(command)) {
      clearTimeout(copiedTimer);
      copied = command;
      copiedTimer = setTimeout(() => (copied = null), 2000);
    } else {
      showError('Couldn’t copy the command. Select it and copy it instead.');
    }
  }

  const place = platform === 'mac' ? 'menu bar' : 'system tray';
  /** The confetti: ten tiny keycaps. */
  const PIECES = Array.from({ length: 10 }, (_, i) => i);

  $effect(() => {
    document.title = COPY.title;
  });
</script>

{#snippet copyable(command: string)}
  <div class="command">
    <code class="mono selectable">{command}</code>
    <button
      type="button"
      class="btn small"
      aria-label={copied === command ? 'Copied' : 'Copy the command'}
      onclick={() => copyCommand(command)}
    >
      <Icon name={copied === command ? 'check' : 'copy'} size={12} />
      {copied === command ? 'Copied' : 'Copy'}
    </button>
  </div>
{/snippet}

<div class="onboarding">
  <main class="content">
    <header class="hero">
      <div class="logo" class:celebrate><Logo size={64} /></div>
      <h1>{COPY.title}</h1>
      {#if s}<p class="line">{line}</p>{/if}
      <p class="badge" title={OFFLINE}>
        <Icon name="shield" size={12} /><span>{OFFLINE_BADGE}</span>
      </p>
    </header>

    {#if s && step}
      {#if step.phase === 'inputGroup'}
        <div class="opt-in">
          {@render copyable(INPUT_GROUP_COMMAND)}
          <p class="hint">Run it in a terminal, then log out and back in.</p>
          <p class="caution">
            <span class="caution-icon"><Icon name="warning" size={14} /></span>
            <span>{INPUT_GROUP_COST}</span>
          </p>
        </div>
      {/if}

      <div class="action">
        {#if step.action === 'allow'}
          <button type="button" class="btn primary big" disabled={asking} onclick={allow}>
            {COPY.allow}
          </button>
        {:else if step.action === 'reopen'}
          <button type="button" class="btn primary big" disabled={relaunching} onclick={reopen}>
            <Icon name="restart" size={14} />{relaunching ? COPY.reopening : COPY.reopen}
          </button>
        {:else}
          <button type="button" class="btn primary big" disabled={closing} onclick={close}>
            {COPY.done}
          </button>
        {/if}
      </div>

      <div class="live" aria-live="polite">
        {#if status}
          <div class="status {status.tone}" role="status">
            {#if status.tone === 'ok'}
              <span class="badge-check" class:pop={celebrate}>
                <Icon name="check" size={14} />
                {#if celebrate}
                  <span class="confetti" aria-hidden="true">
                    {#each PIECES as i (i)}<span class="piece" style:--i={i}></span>{/each}
                  </span>
                {/if}
              </span>
            {:else if status.tone === 'waiting'}
              <span class="spinner" aria-hidden="true"></span>
            {:else if status.tone === 'warn'}
              <span class="status-icon"><Icon name="warning" size={14} /></span>
            {:else if status.tone === 'info'}
              <span class="status-icon"><Icon name="info" size={14} /></span>
            {/if}
            <span>{status.text}</span>
          </div>
          {#if status.tone === 'ok'}
            <p class="hint where">
              TakTak lives in your {place}: look for the
              <span class="inline-logo"><Logo size={13} /></span> keycap.
            </p>
          {:else if step.phase === 'unavailable'}
            <p class="hint">{UNAVAILABLE}</p>
          {/if}
        {/if}
      </div>

      {#if step.whyAvailable || step.troubleshootAvailable || step.later}
        <nav class="links" aria-label="More">
          {#if step.whyAvailable}
            <button
              type="button"
              class="link"
              aria-expanded={whyOpen}
              aria-controls="why"
              onclick={() => (whyOpen = !whyOpen)}
            >
              {COPY.why}
            </button>
          {/if}
          {#if step.troubleshootAvailable}
            <button
              type="button"
              class="link"
              aria-expanded={troubleOpen}
              aria-controls="trouble"
              onclick={() => (troubleOpen = !troubleOpen)}
            >
              {COPY.trouble}
            </button>
          {/if}
          {#if step.later}
            <button type="button" class="link" disabled={closing} onclick={close}>
              {COPY.later}
            </button>
          {/if}
        </nav>
      {/if}

      {#if step.whyAvailable && whyOpen}
        <section class="panel" id="why" aria-label="Why">
          {#if step.phase === 'inputGroup'}
            <p>{INPUT_GROUP_WHY}</p>
            <p>Prefer not to? Key sounds stay off; everything else works.</p>
          {:else}
            <p>
              macOS asks you to allow Input Monitoring for any app that notices key presses outside
              its own windows.
            </p>
            <p>
              TakTak only notices that a key went down or up — never what you type. Nothing is
              recorded, stored or sent anywhere.
            </p>
          {/if}
          <p>{OFFLINE}</p>
        </section>
      {/if}

      {#if step.troubleshootAvailable && troubleOpen}
        <section class="panel" id="trouble" aria-label="Having trouble?">
          <h2>TakTak isn’t in the list?</h2>
          <p>
            Click <kbd>+</kbd> below the list and choose TakTak, or drag TakTak into the list.
          </p>
          <div class="row">
            <button type="button" class="btn small" onclick={() => run(revealApp())}>
              <Icon name="folder" size={12} />Show TakTak in Finder
            </button>
          </div>
          <h2>On, but still no sound?</h2>
          <p>
            macOS may remember an older copy of TakTak. Remove TakTak with <kbd>−</kbd> and add it
            again with <kbd>+</kbd>, or run this in Terminal and reopen TakTak:
          </p>
          {@render copyable(TCC_RESET_COMMAND)}
          {#if step.action !== 'reopen'}
            <div class="row">
              <button type="button" class="btn small" disabled={relaunching} onclick={reopen}>
                <Icon name="restart" size={12} />{relaunching ? COPY.reopening : COPY.reopen}
              </button>
              <span class="hint">Sometimes macOS only lets a freshly opened TakTak listen.</span>
            </div>
          {/if}
          <p class="hint">On macOS 12 or earlier, click the lock first to make changes.</p>
        </section>
      {/if}
    {:else if !app.error}
      <p class="loading" aria-busy="true">Loading…</p>
    {/if}
  </main>

  <Toast bottom={16} />
</div>

<style>
  .onboarding {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--bg);
  }

  .content {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 28px 40px 24px;
  }

  .hero {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
  }

  .logo {
    filter: drop-shadow(0 4px 10px rgba(194, 76, 24, 0.28));
  }

  .logo.celebrate {
    animation: press 0.5s ease-in-out 2;
  }

  h1 {
    margin-top: 14px;
    font-size: 22px;
    font-weight: 700;
    line-height: 1.2;
  }

  .line {
    max-width: 360px;
    margin-top: 8px;
    color: var(--text-2);
    font-size: 14px;
    line-height: 1.45;
  }

  .badge {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    margin-top: 12px;
    padding: 3px 10px;
    border-radius: 999px;
    background: var(--accent-soft);
    color: var(--accent-text);
    font-size: 11.5px;
    font-weight: 600;
  }

  .action {
    display: flex;
    justify-content: center;
    margin-top: 22px;
  }

  .btn.big {
    min-width: 240px;
    height: 36px;
    padding: 0 20px;
    border-radius: var(--radius-m);
    font-size: 14px;
    font-weight: 600;
  }

  .live {
    display: flex;
    flex-direction: column;
    align-items: center;
    min-height: 34px;
    margin-top: 12px;
    text-align: center;
  }

  .status {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    color: var(--text-2);
  }

  .status.ok {
    color: var(--ok-text);
    font-size: 14px;
    font-weight: 600;
  }

  .status.warn {
    color: var(--warning-text);
    font-weight: 500;
  }

  .status-icon {
    display: inline-flex;
  }

  .status.warn .status-icon {
    color: var(--warning);
  }

  .hint {
    color: var(--text-2);
    font-size: 12px;
  }

  .live .hint {
    margin-top: 4px;
    max-width: 360px;
  }

  .inline-logo {
    display: inline-block;
    vertical-align: -2px;
  }

  .badge-check {
    position: relative;
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    border-radius: 50%;
    background: var(--ok);
    color: #fff;
  }

  .badge-check.pop {
    animation: pop 0.45s cubic-bezier(0.3, 1.6, 0.5, 1);
  }

  .confetti {
    position: absolute;
    inset: 0;
    pointer-events: none;
  }

  /* Ten tiny keycaps flying out of the check mark, once. */
  .piece {
    --angle: calc(var(--i) * 36deg);
    position: absolute;
    top: 50%;
    left: 50%;
    width: 6px;
    height: 6px;
    margin: -3px 0 0 -3px;
    border-radius: 2px;
    background: var(--accent);
    opacity: 0;
    animation: fly 0.9s ease-out 0.1s 1 both;
  }

  .piece:nth-child(3n) {
    background: #ffa36a;
  }

  .piece:nth-child(3n + 1) {
    background: var(--ok);
  }

  .spinner {
    flex: none;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    border: 2px solid var(--track);
    border-top-color: var(--accent);
    animation: spin 0.9s linear infinite;
  }

  .links {
    display: flex;
    justify-content: center;
    flex-wrap: wrap;
    gap: 4px 18px;
    margin-top: 14px;
  }

  .link {
    padding: 2px 4px;
    border: 0;
    border-radius: 4px;
    background: none;
    color: var(--text-2);
    font-size: 12px;
    text-decoration: underline;
    text-decoration-color: var(--border);
    text-underline-offset: 3px;
  }

  .link:hover:not(:disabled),
  .link[aria-expanded='true'] {
    color: var(--text);
    text-decoration-color: currentColor;
  }

  .link:disabled {
    opacity: 0.45;
  }

  .panel {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-top: 12px;
    padding: 14px 16px;
    border-radius: var(--radius-l);
    background: var(--surface);
    box-shadow: var(--card-shadow);
    color: var(--text-2);
    font-size: 12px;
    line-height: 1.45;
  }

  .panel h2 {
    color: var(--text);
    font-size: 12px;
    font-weight: 600;
  }

  .panel h2:not(:first-child) {
    margin-top: 4px;
  }

  .row {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 10px;
  }

  .opt-in {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-top: 20px;
  }

  .caution {
    display: flex;
    gap: 8px;
    padding: 8px 10px;
    border-radius: var(--radius-m);
    background: var(--warning-bg);
    color: var(--warning-text);
    font-size: 12px;
    line-height: 1.45;
  }

  .caution-icon {
    display: inline-flex;
    padding-top: 1px;
  }

  kbd {
    display: inline-block;
    min-width: 16px;
    padding: 0 4px;
    border-radius: 4px;
    background: var(--surface-2);
    box-shadow: 0 0 0 0.5px var(--border);
    color: var(--text);
    font-family: var(--font);
    font-weight: 600;
    text-align: center;
  }

  .command {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 6px 6px 10px;
    border-radius: var(--radius-s);
    background: var(--surface-2);
  }

  .command code {
    flex: 1;
    min-width: 0;
    color: var(--text);
    overflow-wrap: anywhere;
  }

  .loading {
    margin-top: 24px;
    color: var(--text-2);
    text-align: center;
  }

  @media (prefers-reduced-motion: reduce) {
    .logo.celebrate,
    .badge-check.pop,
    .piece,
    .spinner {
      animation: none;
    }
  }

  @keyframes press {
    50% {
      transform: translateY(3px) scale(0.96);
    }
  }

  @keyframes pop {
    from {
      transform: scale(0.4);
    }
  }

  @keyframes fly {
    0% {
      opacity: 1;
      transform: rotate(var(--angle)) translateY(0) rotate(0deg);
    }
    100% {
      opacity: 0;
      transform: rotate(var(--angle)) translateY(-30px) rotate(140deg);
    }
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
