<script lang="ts" generics="T extends string">
  // A compact segmented control.
  import Icon from "#lib/components/common/Icon.svelte";
  import type { IconName } from "#lib/ui/icons.js";

  let {
    value = $bindable(),
    options,
    label,
    onchange,
  }: {
    value: T;
    options: { value: T; label?: string; icon?: IconName; title?: string }[];
    label: string;
    onchange?: (value: T) => void;
  } = $props();
</script>

<div class="seg" role="radiogroup" aria-label={label}>
  {#each options as o (o.value)}
    <button
      type="button"
      role="radio"
      aria-checked={value === o.value}
      class:on={value === o.value}
      data-tip={o.title ?? (o.label ? undefined : o.value)}
      onclick={() => {
        value = o.value;
        onchange?.(o.value);
      }}
    >
      {#if o.icon}<Icon name={o.icon} size={13} stroke={1.8} />{/if}
      {#if o.label}<span>{o.label}</span>{/if}
    </button>
  {/each}
</div>

<style>
  .seg {
    display: inline-flex;
    padding: 2px;
    gap: 2px;
    background: var(--inset);
    border: 1px solid var(--line);
    border-radius: var(--r-2);
    flex-wrap: wrap;
    max-width: 100%;
  }
  button {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: calc(var(--row) - 8px);
    padding: 0 7px;
    font-size: 11.5px;
    font-weight: 500;
    color: var(--fg-2);
    border-radius: calc(var(--r-2) - 2px);
  }
  button:hover {
    color: var(--fg);
  }
  button.on {
    color: var(--fg);
    background: var(--s3);
    box-shadow: inset 0 0 0 1px var(--line-strong);
  }
</style>
