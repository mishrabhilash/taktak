<script lang="ts">
  import Icon from '../../components/Icon.svelte';
  import Logo from '../../components/Logo.svelte';
  import { getLatency, openOnboarding } from '../../lib/api';
  import { formatMs } from '../../lib/format';
  import { OFFLINE } from '../../lib/offline';
  import { COPY } from '../../lib/onboarding';
  import { run } from '../../lib/store.svelte';
  import type { AppState, LatencyReport } from '../../lib/types';

  let { s }: { s: AppState } = $props();

  /** The promise, word for word, wherever TakTak states it. */
  const PRIVACY =
    'TakTak never stores, logs or sends what you type. It only uses which key went down or up, in memory, to pick a sound. No accounts, no analytics, no update checks.';

  let latency = $state<LatencyReport | null>(null);
  let latencyError = $state(false);

  // Poll once a second, only while this section is shown and the window is visible.
  $effect(() => {
    let timer: ReturnType<typeof setInterval> | undefined;
    let alive = true;
    const poll = async () => {
      try {
        const report = await getLatency();
        if (alive) {
          latency = report;
          latencyError = false;
        }
      } catch {
        if (alive) latencyError = true;
      }
    };
    const startStop = () => {
      clearInterval(timer);
      timer = undefined;
      if (document.visibilityState === 'visible') {
        void poll();
        timer = setInterval(poll, 1000);
      }
    };
    startStop();
    document.addEventListener('visibilitychange', startStop);
    return () => {
      alive = false;
      clearInterval(timer);
      document.removeEventListener('visibilitychange', startStop);
    };
  });
</script>

<header class="about-header">
  <Logo size={64} />
  <div>
    <h1>TakTak</h1>
    <p class="version selectable">Version {s.version}</p>
    <p class="tagline">Mechanical keyboard sounds as you type.</p>
  </div>
</header>

<div class="group privacy">
  <span class="privacy-icon"><Icon name="shield" size={20} /></span>
  <div>
    <p class="offline">{OFFLINE}</p>
    <h2 class="row-label">Your typing stays yours</h2>
    <p class="privacy-text">{PRIVACY}</p>
  </div>
</div>

<div class="group">
  <div class="row wrap">
    <div class="row-text">
      <span class="row-label">Welcome guide</span>
      <span class="hint">
        {#if s.onboarding.permissionRequired}
          The welcome screen from the first launch, with help when TakTak can’t hear your keys.
        {:else}
          The welcome screen from the first launch.
        {/if}
      </span>
    </div>
    <button type="button" class="btn" onclick={() => run(openOnboarding())}>
      {COPY.showAgain}
    </button>
  </div>
</div>

<h2 class="group-title">Latency</h2>
<div class="group latency" aria-live="off">
  <!-- The same layout with or without numbers, so nothing moves when they arrive. -->
  <div class="row latency-row">
    <div class="figure">
      <span class="big tabular" class:empty={!latency}>
        {latency ? formatMs(latency.totalP50Ms) : '–'}
      </span>
      <span class="hint">typical, key down to sound</span>
    </div>
    <dl class="stats tabular">
      <div><dt>95% under</dt><dd>{latency ? formatMs(latency.totalP95Ms) : '–'}</dd></div>
      <div><dt>Worst</dt><dd>{latency ? formatMs(latency.totalMaxMs) : '–'}</dd></div>
      <div><dt>Key presses</dt><dd>{latency ? latency.count : '–'}</dd></div>
    </dl>
  </div>
  <div class="row breakdown tabular">
    {#if latency}
      <span>Input {formatMs(latency.inputP50Ms)}</span>
      <span aria-hidden="true">+</span>
      <span>Queue {formatMs(latency.queueP50Ms)}</span>
      <span aria-hidden="true">+</span>
      <span>Output {formatMs(latency.outputMs)}</span>
    {:else}
      <span>
        {latencyError
          ? 'Latency isn’t available right now.'
          : 'Type a few keys in any app to measure. Only timings are measured, never which keys.'}
      </span>
    {/if}
  </div>
</div>

<h2 class="group-title">Credits</h2>
<div class="group credits">
  <p class="row credits-intro">
    TakTak is free software under the MIT license. Each pack’s author and license are listed
    below. The licenses and sources of third-party code and of the bundled sounds are in
    CREDITS.md in TakTak’s source code.
  </p>
  {#each s.packs as pack (pack.id)}
    <div class="row credit">
      <div class="row-text">
        <span class="row-label">{pack.name}</span>
        <span class="hint selectable">
          {pack.author} · {pack.license}
        </span>
        {#if pack.attribution}
          <span class="hint selectable attribution">{pack.attribution}</span>
        {/if}
      </div>
    </div>
  {/each}
</div>

<style>
  /* In a narrow window the button goes under the text instead of squeezing it. */
  .wrap {
    flex-wrap: wrap;
  }

  .wrap .row-text {
    flex: 1 1 240px;
  }

  .about-header {
    display: flex;
    align-items: center;
    gap: 16px;
    margin-bottom: 20px;
  }

  h1 {
    font-size: 22px;
    font-weight: 700;
    line-height: 1.2;
  }

  .version {
    color: var(--text-2);
  }

  .tagline {
    margin-top: 2px;
    color: var(--text-2);
    font-size: 12px;
  }

  .privacy {
    display: flex;
    gap: 12px;
    padding: 14px;
  }

  .privacy-icon {
    color: var(--accent-text);
  }

  .offline {
    margin-bottom: 10px;
    color: var(--accent-text);
    font-size: 15px;
    font-weight: 650;
    line-height: 1.35;
  }

  .privacy h2 {
    font-size: 14px;
    font-weight: 600;
  }

  .privacy-text {
    margin-top: 4px;
    line-height: 1.5;
  }

  .latency-row {
    align-items: flex-end;
    justify-content: space-between;
    flex-wrap: wrap;
    padding: 14px;
  }

  .figure {
    display: flex;
    flex-direction: column;
  }

  .big {
    font-size: 28px;
    font-weight: 650;
    line-height: 1.1;
  }

  .big.empty {
    color: var(--text-3);
  }

  .stats {
    display: flex;
    gap: 20px;
  }

  dt {
    color: var(--text-2);
    font-size: 11px;
  }

  dd {
    font-size: 14px;
    font-weight: 600;
  }

  .breakdown {
    gap: 8px;
    min-height: 36px;
    color: var(--text-2);
    font-size: 12px;
  }

  .credits-intro {
    display: block;
    color: var(--text-2);
    font-size: 12px;
  }

  .credit {
    min-height: 0;
    padding: 8px 12px;
  }

  .attribution {
    overflow-wrap: anywhere;
  }
</style>
