<script lang="ts">
  // An on/off switch (role="switch"). Name it with `label` or `labelledby`.
  interface Props {
    checked: boolean;
    onchange: (checked: boolean) => void;
    label?: string;
    labelledby?: string;
    describedby?: string;
    disabled?: boolean;
    small?: boolean;
    id?: string;
  }
  let {
    checked,
    onchange,
    label,
    labelledby,
    describedby,
    disabled = false,
    small = false,
    id,
  }: Props = $props();
</script>

<button
  {id}
  type="button"
  role="switch"
  class="switch"
  class:small
  aria-checked={checked}
  aria-label={label}
  aria-labelledby={labelledby}
  aria-describedby={describedby}
  title={label}
  {disabled}
  onclick={() => onchange(!checked)}
>
  <span class="knob"></span>
</button>

<style>
  .switch {
    --w: 36px;
    --h: 20px;
    flex: none;
    position: relative;
    width: var(--w);
    height: var(--h);
    padding: 0;
    border: 0;
    border-radius: calc(var(--h) / 2);
    background: var(--switch-off);
    transition: background-color 0.18s ease;
  }

  .switch.small {
    --w: 30px;
    --h: 17px;
  }

  .switch[aria-checked='true'] {
    background: var(--accent);
  }

  .switch:disabled {
    opacity: 0.45;
  }

  .knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: calc(var(--h) - 4px);
    height: calc(var(--h) - 4px);
    border-radius: 50%;
    background: #fff;
    box-shadow: 0 0 0 0.5px rgba(0, 0, 0, 0.12), 0 1px 2px rgba(0, 0, 0, 0.25);
    transition: transform 0.18s ease;
  }

  .switch[aria-checked='true'] .knob {
    transform: translateX(calc(var(--w) - var(--h)));
  }
</style>
