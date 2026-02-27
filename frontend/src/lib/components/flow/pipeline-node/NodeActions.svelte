<script lang="ts">
  type NodeActionsProps = {
    isGroup?: boolean;
    className?: string;
  };

  const { isGroup = false, className = '' }: NodeActionsProps = $props();
  const showGroupChip = $derived(Boolean(isGroup));

  export type $$Props = NodeActionsProps;
</script>

{#if showGroupChip}
  <div class={`absolute right-2 top-2 pointer-events-none z-30 ${className}`.trim()}>
    <span class="group-chip" title="Node group">Node group</span>
  </div>
{/if}

<style>
  .group-chip {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: 999px;
    border: 1px solid color-mix(in srgb, var(--pipeline-node-group-accent, #f59e0b) 60%, transparent);
    background: color-mix(in srgb, var(--pipeline-node-group-accent, #f59e0b) 22%, transparent);
    color: var(--pipeline-node-text-strong);
    padding: 0.28rem 0.7rem;
    font-size: 0.62rem;
    font-weight: 700;
    letter-spacing: 0.22em;
    text-transform: uppercase;
    text-shadow: 0 1px 4px rgba(0, 0, 0, 0.35);
    box-shadow:
      0 0 0 1px color-mix(in srgb, var(--pipeline-node-group-accent, #f59e0b) 20%, transparent),
      0 6px 14px -10px color-mix(in srgb, var(--pipeline-node-group-accent, #f59e0b) 70%, transparent);
  }

</style>
