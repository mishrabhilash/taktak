<script lang="ts">
  // The TakTak keycap (assets/icon.svg without the drop shadow), inline so nothing loads.
  let { size = 24 }: { size?: number } = $props();
  const uid = $props.id();
</script>

<svg
  class="logo"
  width={size}
  height={size}
  viewBox="100 100 824 824"
  aria-hidden="true"
  focusable="false"
>
  <defs>
    <linearGradient id="{uid}-skirt" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#E8692F" />
      <stop offset="1" stop-color="#BF4A17" />
    </linearGradient>
    <linearGradient id="{uid}-top" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#FFA36A" />
      <stop offset="1" stop-color="#FF8040" />
    </linearGradient>
  </defs>
  <rect x="100" y="100" width="824" height="824" rx="184" fill="url(#{uid}-skirt)" />
  <rect x="176" y="136" width="672" height="640" rx="132" fill="url(#{uid}-top)" />
  <rect
    x="176"
    y="136"
    width="672"
    height="640"
    rx="132"
    fill="none"
    stroke="#FFFFFF"
    stroke-opacity="0.25"
    stroke-width="6"
  />
  <g fill="#FFFFFF" fill-opacity="0.96">
    <rect x="356" y="280" width="312" height="72" rx="36" />
    <rect x="476" y="280" width="72" height="352" rx="36" />
  </g>
</svg>

<style>
  .logo {
    flex: none;
    display: block;
  }
</style>
