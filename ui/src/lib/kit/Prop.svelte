<script lang="ts">
  // One property row: label on the left, control on the right. Optional help
  // (tooltip), reset-to-default (shown only when changed) and a changed marker.
  import type { Snippet } from "svelte";
  import Icon from "$lib/components/common/Icon.svelte";

  let {
    label,
    help,
    changed = false,
    onreset,
    stacked = false,
    children,
  }: { label: string; help?: string; changed?: boolean; onreset?: () => void; stacked?: boolean; children: Snippet } = $props();
</script>

<div class="prop" class:stacked>
  <span class="label" data-tip={help}>
    {#if changed}<span class="dot" aria-label="changed"></span>{/if}
    <span class="text">{label}</span>
  </span>
  <span class="ctl">
    {@render children()}
    {#if changed && onreset}
      <button type="button" class="reset" onclick={onreset} aria-label="Reset {label}" data-tip="Reset to default">
        <Icon name="rotate-clockwise" size={11} stroke={2} />
      </button>
    {/if}
  </span>
</div>

<style>
  .prop {
    display: grid;
    grid-template-columns: minmax(80px, 38%) 1fr;
    align-items: center;
    gap: 8px;
    min-height: var(--row);
    padding: 1px 10px;
  }
  .prop:hover {
    background: color-mix(in oklab, var(--s3) 50%, transparent);
  }
  .prop.stacked {
    grid-template-columns: 1fr;
    gap: 3px;
    padding-block: 4px;
  }
  .prop > :global(*) {
    min-width: 0;
  }
  /* Narrow pane: label above its control instead of squeezing both. */
  @container pane (max-width: 330px) {
    .prop {
      grid-template-columns: 1fr;
      gap: 2px;
      padding-block: 4px;
    }
    .prop .ctl {
      justify-content: flex-start;
    }
  }
  .label {
    display: flex;
    align-items: center;
    gap: 5px;
    min-width: 0;
    font-size: 12px;
    color: var(--fg-2);
  }
  .text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dot {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--accent);
    flex-shrink: 0;
  }
  .ctl {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    flex-wrap: wrap;
    gap: 4px 6px;
    min-width: 0;
    padding-block: 2px;
  }
  .reset {
    display: inline-flex;
    padding: 3px;
    color: var(--fg-3);
    border-radius: var(--r-1);
  }
  .reset:hover {
    color: var(--fg);
    background: var(--s3);
  }
</style>
