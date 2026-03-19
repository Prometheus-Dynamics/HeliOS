<script lang="ts">
  import { Handle, Position } from '@xyflow/svelte';
  import type { CompactPortHandle } from './types';

  const props = $props<{
    direction: 'input' | 'output';
    handles: CompactPortHandle[];
    emptyLabel?: string;
  }>();

  const direction = $derived(props.direction);
  const handles = $derived(props.handles);
  const isInput = $derived(direction === 'input');
  const emptyLabel = $derived(props.emptyLabel ?? (isInput ? 'No inputs' : 'No outputs'));
</script>

<section class={`flex flex-col gap-1.5 ${isInput ? '-ml-1' : 'items-end -mr-1'}`}>
  {#if handles.length > 0}
    {#each handles as handle (handle.handleId)}
      <div
        class={`pipeline-port-handle relative flex min-h-5 items-center rounded border px-2 py-1 ${
          isInput ? 'pl-3 text-left' : 'pr-3 text-right'
        } ${handle.hasIssue ? 'pipeline-port-handle--issue' : ''} ${
          handle.isHighlighted ? 'pipeline-port-handle--highlight' : ''
        }`}
        title={handle.name}
      >
        {#if isInput}
          <Handle id={handle.handleId} type="target" position={Position.Left}>
            <span class="sr-only">Input {handle.name}</span>
          </Handle>
        {/if}
        <span class="pipeline-port-handle__dot" aria-hidden="true"></span>
        {#if !isInput}
          <Handle id={handle.handleId} type="source" position={Position.Right}>
            <span class="sr-only">Output {handle.name}</span>
          </Handle>
        {/if}
      </div>
    {/each}
  {:else}
    <p class="px-2 text-micro-tight uppercase tracking-[0.12em] text-surface-500/70">{emptyLabel}</p>
  {/if}
</section>

<style>
  .pipeline-port-handle {
    border-color: color-mix(in srgb, var(--flow-border, #1f2937) 90%, transparent);
    background: color-mix(in srgb, var(--flow-surface-soft, rgba(15, 23, 42, 0.82)) 88%, transparent);
  }

  .pipeline-port-handle__dot {
    display: inline-block;
    width: 0.45rem;
    height: 0.45rem;
    border-radius: 9999px;
    background: color-mix(in srgb, var(--flow-text, #f8fafc) 82%, transparent);
    opacity: 0.78;
  }

  .pipeline-port-handle--issue {
    border-color: color-mix(in srgb, #fbbf24 60%, var(--flow-border, #1f2937));
    box-shadow: 0 0 0 1px rgba(251, 191, 36, 0.2);
  }

  .pipeline-port-handle--issue .pipeline-port-handle__dot {
    background: #fbbf24;
    opacity: 0.95;
  }

  .pipeline-port-handle--highlight {
    border-color: color-mix(in srgb, #7dd3fc 65%, var(--flow-border, #1f2937));
    box-shadow: 0 0 0 1px rgba(56, 189, 248, 0.18);
  }

  .pipeline-port-handle--highlight .pipeline-port-handle__dot {
    background: #7dd3fc;
    opacity: 1;
  }
</style>
