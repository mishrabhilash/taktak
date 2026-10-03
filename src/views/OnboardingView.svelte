<script lang="ts">
  // The "Welcome to TakTak" window: what TakTak is and where it lives, then (macOS) the Input
  // Monitoring step with its privacy promise, a live status that turns into success without a
  // restart, "Quit & Reopen" when macOS wants a relaunch, and troubleshooting for a stale entry.
  // The steps are decided in onboarding.ts; nothing here reads what the user types.
  import Icon from '../components/Icon.svelte';
  import Keycaps from '../components/Keycaps.svelte';
  import Logo from '../components/Logo.svelte';
  import Toast from '../components/Toast.svelte';
  import { finishOnboarding, openPermissionSettings, relaunch } from '../lib/api';
  import { copyText } from '../lib/clipboard';
  import {
    type OnboardingFacts,
    type OnboardingPhase,
    TCC_RESET_COMMAND,
    justGranted,
    onboardingStep,
    troubleshootDelay,
  } from '../lib/onboarding';
  import { platform } from '../lib/platform';
  import { app, apply, run, showError } from '../lib/store.svelte';

  const s = $derived(app.state);

  /** When the user pressed "Open Input Monitoring Settings" here. */
  let openedAt = $state<number | null>(null);
  /** The clock as of the last re-evaluation (a one-shot timer moves it, nothing polls). */
  let now = $state(Date.now());

  const facts = $derived<OnboardingFacts | null>(
    s
      ? {
          permission: s.permission,
          permissionRequired: s.onboarding.permissionRequired,
          relaunchSuggested: s.onboarding.relaunchSuggested,
          openedAt,
          now,
        }
      : null,
  );
  const step = $derived(facts ? onboardingStep(facts) : null);
  const phase = $derived(step?.phase ?? null);

  // Open the troubleshooting by itself once the user has waited a while.
  $effect(() => {
    if (!facts) return;
    const delay = troubleshootDelay({ ...facts, now: Date.now() });
    if (delay === null) return;
    const timer = setTimeout(() => (now = Date.now()), delay + 50);
    return () => clearTimeout(timer);
  });

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

  let relaunching = $state(false);
  let closing = $state(false);
  let copied = $state(false);
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

  async function openSettings(): Promise<void> {
    openedAt = Date.now();
    now = openedAt;
    await run(openPermissionSettings());
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

  async function copyCommand(): Promise<void> {
    if (await copyText(TCC_RESET_COMMAND)) {
      clearTimeout(copiedTimer);
      copied = true;
      copiedTimer = setTimeout(() => (copied = false), 2000);
    } else {
      showError('Couldn’t copy the command. Select it and copy it instead.');
    }
  }

  const place = platform === 'mac' ? 'menu bar' : 'system tray';
  /** The confetti: ten tiny keycaps. */
  const PIECES = Array.from({ length: 10 }, (_, i) => i);

  $effect(() => {
    document.title = 'Welcome to TakTak';
  });
</script>

<div class="onboarding">
  <main class="content">
    <header class="hero">
      <div class="logo" class:celebrate><Logo size={52} /></div>
      <h1>Welcome to TakTak</h1>
      <p class="lead">Mechanical keyboard sounds as you type, in every app.</p>
      <p class="where">
        TakTak lives in your {place}: look for the
        <span class="inline-logo"><Logo size={14} /></span>
        keycap{platform === 'mac' ? ' at the top of your screen' : ''}. Click it for volume and sound
        packs; right-click it for a quick menu.
      </p>
    </header>

    {#if s && step}
      {#if step.phase === 'welcome'}
        <section class="card" aria-labelledby="ready-title">
          <div class="status granted">
            <span class="badge-check"><Icon name="check" size={18} /></span>
            <div>
              <h2 class="status-title" id="ready-title">You’re all set</h2>
              <p class="status-hint">Start typing in any app to hear TakTak.</p>
            </div>
          </div>
          {#if s.settings.muteHotkey}
            <p class="tip">
              Mute or unmute any time with <Keycaps accelerator={s.settings.muteHotkey} small />
            </p>
          {/if}
        </section>
      {:else if step.phase === 'unavailable'}
        <section class="card" aria-labelledby="unavailable-title">
          <div class="status">
            <span class="status-icon info"><Icon name="info" size={18} /></span>
            <div>
              {#if platform === 'mac'}
                <h2 class="status-title" id="unavailable-title">Key sounds are turned off</h2>
                <p class="status-hint">
                  TakTak was started without its key listener, so typing makes no sound.
                </p>
              {:else}
                <h2 class="status-title" id="unavailable-title">
                  Key sounds aren’t available on this system yet
                </h2>
                <p class="status-hint">
                  TakTak can’t hear key presses on {platform === 'windows' ? 'Windows' : 'Linux'}
                  yet, so typing makes no sound. You can still try the sound packs in Settings.
                </p>
              {/if}
            </div>
          </div>
        </section>
      {:else}
        <section class="card" aria-labelledby="perm-title">
          <h2 class="card-title" id="perm-title">Allow Input Monitoring</h2>
          <p class="why">
            macOS asks you to allow Input Monitoring for any app that notices key presses outside
            its own windows.
          </p>
          <div class="promise">
            <span class="promise-icon"><Icon name="shield" size={18} /></span>
            <p>
              TakTak only notices that a key went down or up — never what you type. Nothing is
              recorded, stored or sent anywhere; TakTak has no network access.
            </p>
          </div>

          <div class="live" aria-live="polite">
            {#if step.phase === 'granted'}
              <div class="status granted" role="status">
                <span class="badge-check" class:pop={celebrate}>
                  <Icon name="check" size={18} />
                  {#if celebrate}
                    <span class="confetti" aria-hidden="true">
                      {#each PIECES as i (i)}<span class="piece" style:--i={i}></span>{/each}
                    </span>
                  {/if}
                </span>
                <div>
                  <p class="status-title">TakTak can hear your keys. Try typing!</p>
                  <p class="status-hint">
                    Start typing in any app: every key you press makes a sound.
                  </p>
                </div>
              </div>
              {#if s.settings.muteHotkey}
                <p class="tip">
                  Mute or unmute any time with <Keycaps accelerator={s.settings.muteHotkey} small />
                </p>
              {/if}
            {:else if step.phase === 'relaunch'}
              <div class="status relaunch" role="status">
                <span class="status-icon"><Icon name="restart" size={18} /></span>
                <div>
                  <p class="status-title">macOS needs TakTak to restart before it can listen.</p>
                  <p class="status-hint">Input Monitoring is on. Your settings are kept.</p>
                </div>
              </div>
              <button
                type="button"
                class="btn primary wide"
                disabled={relaunching}
                onclick={reopen}
              >
                <Icon name="restart" size={14} />{relaunching ? 'Reopening…' : 'Quit & Reopen'}
              </button>
            {:else}
              <ol class="steps">
                <li>
                  <span class="num" aria-hidden="true">1</span>
                  <div class="step-body with-button">
                    <button type="button" class="btn primary" onclick={openSettings}>
                      Open Input Monitoring Settings
                    </button>
                    {#if step.phase === 'ask'}
                      <span class="step-hint">
                        If macOS asks first, choose Open System Settings (Open System Preferences
                        on macOS 12 and earlier).
                      </span>
                    {/if}
                  </div>
                </li>
                <li>
                  <span class="num" aria-hidden="true">2</span>
                  <p class="step-body">
                    Turn on <strong>TakTak</strong> in the list. On macOS 12 and earlier, click the
                    lock at the bottom first to make changes.
                  </p>
                </li>
                <li>
                  <span class="num" aria-hidden="true">3</span>
                  <p class="step-body">
                    Come back here. If macOS offers to <strong>Quit &amp; Reopen</strong> TakTak,
                    either choice works: choose Later and this window usually notices by itself;
                    if it doesn’t, it offers Quit &amp; Reopen too.
                  </p>
                </li>
              </ol>
              <div class="status waiting" role="status">
                {#if step.phase === 'waiting'}
                  <span class="spinner" aria-hidden="true"></span>
                  <p class="status-title">Waiting for Input Monitoring…</p>
                {:else}
                  <span class="status-icon warn"><Icon name="warning" size={16} /></span>
                  <p class="status-title">Input Monitoring isn’t allowed yet.</p>
                {/if}
              </div>
            {/if}
          </div>
        </section>

        {#if step.troubleshootAvailable}
          <details class="trouble" bind:open={troubleOpen}>
            <summary>
              <span class="chevron"><Icon name="chevron" size={12} /></span>
              TakTak is on in the list but still can’t hear keys?
            </summary>
            <div class="trouble-body">
              <p>
                macOS may be holding on to an entry from an older copy of TakTak. Select TakTak in
                the list and remove it with <kbd>−</kbd>, then add it again with <kbd>+</kbd> (it is
                in Applications). On macOS 12 and earlier, click the lock first to make changes.
              </p>
              <p>Or run this in Terminal, then open TakTak again:</p>
              <div class="command">
                <code class="mono selectable">{TCC_RESET_COMMAND}</code>
                <button
                  type="button"
                  class="btn small"
                  aria-label={copied ? 'Copied' : 'Copy the command'}
                  onclick={copyCommand}
                >
                  <Icon name={copied ? 'check' : 'copy'} size={12} />{copied ? 'Copied' : 'Copy'}
                </button>
              </div>
              {#if step.phase !== 'relaunch'}
                <div class="trouble-actions">
                  <button type="button" class="btn" disabled={relaunching} onclick={reopen}>
                    <Icon name="restart" size={14} />{relaunching ? 'Reopening…' : 'Quit & Reopen'}
                  </button>
                  <span class="step-hint">
                    Sometimes macOS only lets a freshly opened TakTak listen.
                  </span>
                </div>
              {/if}
            </div>
          </details>
        {/if}
      {/if}
    {:else if !app.error}
      <p class="loading" aria-busy="true">Loading…</p>
    {/if}
  </main>

  <footer class="footer">
    {#if step && !step.closePrimary}
      <p class="footer-hint">TakTak reminds you the next time it starts.</p>
    {/if}
    <button
      type="button"
      class="btn"
      class:primary={step?.closePrimary ?? false}
      disabled={!s || closing}
      onclick={close}
    >
      {step?.closeLabel ?? 'Later'}
    </button>
  </footer>

  <Toast bottom={64} />
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
    padding: 22px 36px 16px;
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
    margin-top: 10px;
    font-size: 22px;
    font-weight: 700;
    line-height: 1.2;
  }

  .lead {
    margin-top: 4px;
    font-size: 14px;
  }

  .where {
    max-width: 420px;
    margin-top: 10px;
    color: var(--text-2);
    font-size: 12px;
    line-height: 1.5;
  }

  .inline-logo {
    display: inline-block;
    vertical-align: -2px;
  }

  .card {
    margin-top: 16px;
    padding: 16px 20px;
    border-radius: var(--radius-l);
    background: var(--surface);
    box-shadow: var(--card-shadow);
  }

  .card-title {
    font-size: 15px;
    font-weight: 650;
  }

  .why {
    margin-top: 4px;
    color: var(--text-2);
  }

  .promise {
    display: flex;
    gap: 10px;
    margin-top: 10px;
    padding: 10px 12px;
    border-radius: var(--radius-m);
    background: var(--accent-soft);
  }

  .promise-icon {
    padding-top: 1px;
    color: var(--accent-text);
  }

  .promise p {
    font-size: 12px;
    line-height: 1.45;
  }

  .live {
    margin-top: 14px;
  }

  .steps {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .steps li {
    display: flex;
    align-items: flex-start;
    gap: 10px;
  }

  .num {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    margin-top: 2px;
    border-radius: 50%;
    background: var(--surface-2);
    color: var(--text-2);
    font-size: 12px;
    font-weight: 600;
  }

  .step-body {
    padding-top: 3px;
  }

  .step-body.with-button {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 4px;
    padding-top: 0;
  }

  .step-hint {
    color: var(--text-2);
    font-size: 12px;
  }

  .status {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: 14px;
    padding: 10px 12px;
    border-radius: var(--radius-m);
    background: var(--surface-2);
  }

  .steps + .status {
    margin-top: 12px;
  }

  .live > .status:first-child,
  .card > .status:first-child {
    margin-top: 0;
  }

  .status.granted {
    background: var(--ok-bg);
  }

  .status.relaunch {
    background: var(--warning-bg);
  }

  .status-title {
    font-weight: 600;
  }

  h2.status-title {
    font-size: 13px;
  }

  .status-hint {
    margin-top: 1px;
    color: var(--text-2);
    font-size: 12px;
  }

  .status-icon {
    display: inline-flex;
    color: var(--warning);
  }

  .status-icon.info {
    color: var(--text-2);
  }

  .relaunch .status-icon {
    color: var(--warning-text);
  }

  .badge-check {
    position: relative;
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
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
    width: 7px;
    height: 7px;
    margin: -3.5px 0 0 -3.5px;
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

  .tip {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: 10px;
    color: var(--text-2);
    font-size: 12px;
  }

  .btn.wide {
    width: 100%;
    height: 32px;
    margin-top: 10px;
  }

  .spinner {
    flex: none;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    border: 2px solid var(--track);
    border-top-color: var(--accent);
    animation: spin 0.9s linear infinite;
  }

  .trouble {
    margin-top: 12px;
    border-radius: var(--radius-l);
    background: var(--surface);
    box-shadow: var(--card-shadow);
  }

  summary {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 40px;
    padding: 8px 16px;
    border-radius: var(--radius-l);
    font-weight: 500;
    list-style: none;
  }

  summary::-webkit-details-marker {
    display: none;
  }

  summary:hover {
    background: var(--hover);
  }

  .chevron {
    display: inline-flex;
    color: var(--text-2);
    transition: transform 0.15s;
  }

  .trouble[open] .chevron {
    transform: rotate(90deg);
  }

  .trouble-body {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 0 16px 14px 36px;
    color: var(--text-2);
    font-size: 12px;
    line-height: 1.45;
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

  .trouble-actions {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 10px;
    margin-top: 2px;
  }

  .loading {
    margin-top: 24px;
    color: var(--text-2);
    text-align: center;
  }

  .footer {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 12px;
    padding: 12px 20px;
    border-top: 0.5px solid var(--separator);
    background: var(--bg);
  }

  .footer-hint {
    flex: 1;
    color: var(--text-2);
    font-size: 12px;
  }

  .footer .btn {
    min-width: 88px;
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
      transform: rotate(var(--angle)) translateY(-34px) rotate(140deg);
    }
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
