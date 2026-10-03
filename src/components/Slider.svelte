<script lang="ts">
  // A 0..1 slider on a native range input (keyboard, screen readers and dragging for free).
  // `oninput` fires while dragging, `oncommit` on release or a keyboard step.
  import { formatPercent, unit } from '../lib/format';

  interface Props {
    value: number;
    oninput: (value: number) => void;
    oncommit: (value: number) => void;
    id?: string;
    label?: string;
    labelledby?: string;
    describedby?: string;
    disabled?: boolean;
    /** Show the value (e.g. "70%") after the track. */
    showValue?: boolean;
    format?: (value: number) => string;
  }
  let {
    value,
    oninput,
    oncommit,
    id,
    label,
    labelledby,
    describedby,
    disabled = false,
    showValue = true,
    format = formatPercent,
  }: Props = $props();

  const fill = $derived(`${(unit(value) * 100).toFixed(1)}%`);
</script>

<div class="slider" class:disabled>
  <input
    {id}
    type="range"
    min="0"
    max="1"
    step="0.01"
    {value}
    {disabled}
    aria-label={label}
    aria-labelledby={labelledby}
    aria-describedby={describedby}
    aria-valuetext={format(value)}
    style:--fill={fill}
    oninput={(e) => oninput(e.currentTarget.valueAsNumber)}
    onchange={(e) => oncommit(e.currentTarget.valueAsNumber)}
  />
  {#if showValue}
    <output class="value tabular" for={id} aria-hidden="true">{format(value)}</output>
  {/if}
</div>

<style>
  .slider {
    display: flex;
    align-items: center;
    gap: 10px;
    flex: 1;
    min-width: 0;
  }

  .slider.disabled {
    opacity: 0.45;
  }

  .value {
    flex: none;
    width: 4ch;
    text-align: right;
    color: var(--text-2);
    font-size: 12px;
  }

  input {
    flex: 1;
    min-width: 0;
    height: 20px;
    margin: 0;
    padding: 0;
    background: transparent;
    -webkit-appearance: none;
    appearance: none;
    border-radius: 10px;
  }

  input::-webkit-slider-runnable-track {
    height: 4px;
    border-radius: 2px;
    background: linear-gradient(
      to right,
      var(--accent) 0,
      var(--accent) var(--fill),
      var(--track) var(--fill),
      var(--track) 100%
    );
  }

  input::-webkit-slider-thumb {
    -webkit-appearance: none;
    appearance: none;
    width: 16px;
    height: 16px;
    margin-top: -6px;
    border: 0;
    border-radius: 50%;
    background: var(--thumb);
    box-shadow: var(--thumb-shadow);
    transition: transform 0.1s ease;
  }

  input:active::-webkit-slider-thumb {
    transform: scale(1.08);
  }

  input:focus-visible {
    box-shadow: none;
  }

  input:focus-visible::-webkit-slider-thumb {
    box-shadow: var(--thumb-shadow), var(--focus-ring);
  }

  /* Firefox (Linux dev only; the app runs on WebKit / Chromium WebView2). */
  input::-moz-range-track {
    height: 4px;
    border-radius: 2px;
    background: var(--track);
  }

  input::-moz-range-progress {
    height: 4px;
    border-radius: 2px;
    background: var(--accent);
  }

  input::-moz-range-thumb {
    width: 16px;
    height: 16px;
    border: 0;
    border-radius: 50%;
    background: var(--thumb);
    box-shadow: var(--thumb-shadow);
  }

  input:focus-visible::-moz-range-thumb {
    box-shadow: var(--thumb-shadow), var(--focus-ring);
  }
</style>
