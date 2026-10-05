<script lang="ts">
  let {
    checked = $bindable(false),
    label,
    disabled = false,
    onchange,
  }: { checked?: boolean; label?: string; disabled?: boolean; onchange?: (checked: boolean) => void } = $props();
</script>

<button
  type="button"
  role="switch"
  aria-checked={checked}
  aria-label={label}
  class="sw"
  class:on={checked}
  {disabled}
  onclick={() => {
    checked = !checked;
    onchange?.(checked);
  }}
>
  <span></span>
</button>

<style>
  .sw {
    position: relative;
    width: 26px;
    height: 15px;
    border-radius: 99px;
    background: var(--fg-4);
    transition: background var(--t-fast);
    flex-shrink: 0;
  }
  .sw span {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 11px;
    height: 11px;
    border-radius: 50%;
    background: #fff;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.3);
    transition: transform var(--t-fast) var(--ease-out);
  }
  .sw.on {
    background: var(--accent);
  }
  .sw.on span {
    transform: translateX(11px);
  }
  .sw:disabled {
    opacity: 0.4;
  }
</style>
