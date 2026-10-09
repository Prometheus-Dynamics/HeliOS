<script lang="ts">
  // A compact icon button with a tooltip. `active` for toggles, `tone` for intent.
  import Icon from "#lib/components/common/Icon.svelte";
  import type { IconName } from "#lib/ui/icons.js";

  let {
    icon,
    label,
    onclick,
    active = false,
    disabled = false,
    tone = "default",
    size = 26,
    shortcut,
    text,
  }: {
    icon: IconName;
    label: string;
    onclick?: (event: MouseEvent) => void;
    active?: boolean;
    disabled?: boolean;
    tone?: "default" | "accent" | "danger" | "ok";
    size?: number;
    shortcut?: string;
    /** Optional visible text beside the icon. */
    text?: string;
  } = $props();
</script>

<button
  type="button"
  class="ib tone-{tone}"
  class:active
  class:with-text={Boolean(text)}
  style:height="{size}px"
  style:min-width="{size}px"
  {disabled}
  {onclick}
  aria-label={label}
  aria-pressed={active}
  data-tip={shortcut ? `${label} · ${shortcut}` : label}
>
  <Icon name={icon} size={Math.round(size * 0.58)} stroke={1.8} />
  {#if text}<span>{text}</span>{/if}
</button>

<style>
  .ib {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 5px;
    padding: 0 4px;
    border-radius: var(--r-2);
    color: var(--fg-2);
    border: 1px solid transparent;
    transition: background var(--t-fast), color var(--t-fast);
    flex-shrink: 0;
  }
  .ib.with-text {
    padding: 0 8px 0 6px;
    font-size: 12px;
    font-weight: 500;
  }
  .ib:hover:not(:disabled) {
    background: var(--s3);
    color: var(--fg);
  }
  .ib.active {
    background: var(--accent-tint);
    color: var(--accent-fg);
  }
  .ib:disabled {
    opacity: 0.35;
  }
  .tone-accent {
    color: var(--on-accent);
    background: var(--accent);
  }
  .tone-accent:hover:not(:disabled) {
    background: var(--accent-hover);
    color: var(--on-accent);
  }
  .tone-danger:hover:not(:disabled) {
    color: var(--err);
    background: var(--err-bg);
  }
  .tone-ok {
    color: var(--ok);
  }
</style>
