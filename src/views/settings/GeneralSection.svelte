<script lang="ts">
  import Icon from '../../components/Icon.svelte';
  import Switch from '../../components/Switch.svelte';
  import {
    openOnboarding,
    openPermissionSettings,
    relaunch,
    setEnabled,
    setLaunchAtLogin,
    setMuteOnOutputChange,
  } from '../../lib/api';
  import { formatAudioDevice } from '../../lib/format';
  import { COPY } from '../../lib/onboarding';
  import { permissionName, platform } from '../../lib/platform';
  import { grantLabel } from '../../lib/status';
  import { run, set } from '../../lib/store.svelte';
  import type { AppState } from '../../lib/types';

  let { s }: { s: AppState } = $props();

  const permission = $derived(
    s.permission !== 'granted' && s.onboarding.relaunchSuggested
      ? { label: 'Needs a restart', tone: 'warning' }
      : {
          granted: { label: 'Allowed', tone: 'ok' },
          denied: { label: 'Not allowed', tone: 'warning' },
          unknown: { label: 'Not confirmed', tone: 'warning' },
        }[s.permission],
  );

  const audio = $derived(
    {
      ok: { label: 'Working', tone: 'ok' },
      starting: { label: 'Starting…', tone: 'off' },
      fault: { label: 'Not working', tone: 'error' },
    }[s.audio.state],
  );
</script>

<header class="section-header">
  <h1>General</h1>
  <p>Startup, permissions, auto-mute and sound output.</p>
</header>

<div class="group">
  <div class="row">
    <div class="row-text">
      <span class="row-label" id="gen-enabled">Sounds On</span>
      <span class="hint">Turns all of TakTak’s sounds on or off. Also in the menu bar.</span>
    </div>
    <Switch
      labelledby="gen-enabled"
      checked={s.settings.enabled}
      onchange={(v) => set('enabled', v, setEnabled)}
    />
  </div>
  <div class="row">
    <div class="row-text">
      <span class="row-label" id="gen-login">Launch at login</span>
      <span class="hint">Start TakTak in the background when you log in.</span>
    </div>
    <Switch
      labelledby="gen-login"
      checked={s.settings.launchAtLogin}
      onchange={(v) => set('launchAtLogin', v, setLaunchAtLogin)}
    />
  </div>
</div>

<h2 class="group-title">Permissions</h2>
<div class="group">
  <div class="row wrap">
    <div class="row-text">
      <span class="label-line">
        <span class="row-label">{permissionName(platform)}</span>
        <span class="pill {permission.tone}">
          <Icon name={permission.tone === 'ok' ? 'check' : 'warning'} size={12} />{permission.label}
        </span>
      </span>
      <span class="hint">
        Lets TakTak notice when keys go down and up. It never sees what you type.
      </span>
    </div>
    {#if s.permission !== 'granted' && s.onboarding.relaunchSuggested}
      <button type="button" class="btn primary" onclick={() => run(relaunch())}>
        Quit &amp; Reopen
      </button>
    {:else if s.permission !== 'granted'}
      <button type="button" class="btn primary" onclick={() => run(openPermissionSettings())}>
        {grantLabel(platform)}
      </button>
    {/if}
  </div>
  <div class="row wrap">
    <div class="row-text">
      <span class="row-label">Welcome guide</span>
      <span class="hint">
        {#if s.onboarding.permissionRequired}
          The guide from the first launch: what {permissionName(platform)} is for, and what to do
          when TakTak can’t hear your keys.
        {:else}
          The short introduction from the first launch.
        {/if}
      </span>
    </div>
    <button type="button" class="btn" onclick={() => run(openOnboarding())}>
      {COPY.showAgain}
    </button>
  </div>
</div>

<h2 class="group-title">Auto-mute</h2>
<div class="group">
  <div class="row">
    <div class="row-text">
      <span class="row-label" id="gen-output-mute">Mute when the output device changes</span>
      <span class="hint" id="gen-output-mute-hint">
        When headphones are plugged in or unplugged, or AirPods connect, TakTak mutes itself until
        you unmute.
      </span>
    </div>
    <Switch
      labelledby="gen-output-mute"
      describedby="gen-output-mute-hint"
      checked={s.settings.muteOnOutputChange}
      onchange={(v) => set('muteOnOutputChange', v, setMuteOnOutputChange)}
    />
  </div>
  {#if platform === 'mac'}
    <div class="row">
      <div class="row-text">
        <span class="row-label">While the screen is locked</span>
        <span class="hint">
          TakTak is always silent while the screen is locked, and while another user is using
          this Mac. Sounds come back when you return.
        </span>
      </div>
      <span class="pill"><Icon name="lock" size={12} />Always on</span>
    </div>
  {/if}
</div>

<h2 class="group-title">Sound output</h2>
<div class="group">
  <div class="row">
    <div class="row-text">
      <span class="row-label">Output device</span>
      <span class="hint selectable">{formatAudioDevice(s.audio)}</span>
      {#if s.audio.message}
        <span class="hint">{s.audio.message}</span>
      {/if}
    </div>
    <span class="pill {audio.tone}">{audio.label}</span>
  </div>
</div>

<style>
  /* In a narrow window the button goes under the text instead of squeezing it. */
  .wrap {
    flex-wrap: wrap;
  }

  .wrap .row-text {
    flex: 1 1 240px;
  }

  .label-line {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .pill {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 22px;
    padding: 0 8px;
    border-radius: 11px;
    background: var(--surface-2);
    color: var(--text-2);
    font-size: 12px;
    font-weight: 500;
  }

  .pill.ok {
    color: var(--ok-text);
  }

  .pill.warning {
    background: var(--warning-bg);
    color: var(--warning-text);
  }

  .pill.error {
    background: var(--error-bg);
    color: var(--error-text);
  }
</style>
