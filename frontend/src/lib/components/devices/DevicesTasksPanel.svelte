<script lang="ts">
  import type { Snippet } from 'svelte';
  import { createEventDispatcher } from 'svelte';
  import { Panel } from '$lib';

  export type TaskRow = {
    id: string;
    title: string;
    action: string;
    buttonLabel?: string;
    payload?: unknown;
  };

  const {
    tasks = [],
    eyebrow = 'Tasks',
    title = 'Registration follow-ups',
    emptyMessage = 'No open tasks right now.',
    actions,
    children
  }: {
    tasks?: TaskRow[];
    eyebrow?: string;
    title?: string;
    emptyMessage?: string;
    actions?: Snippet;
    children?: Snippet;
  } = $props();

  const dispatch = createEventDispatcher<{ select: { task: TaskRow } }>();

  const handleClick = (task: TaskRow) => {
    dispatch('select', { task });
  };
</script>

<Panel tone="contrast" {eyebrow} {title} {actions}>
  {#if children}
    {@render children()}
  {:else if tasks.length}
    <ul class="space-y-3">
      {#each tasks as task (task.id)}
        <li class="flex items-center justify-between border border-surface-800/70 px-3 py-2 text-sm">
          <div>
            <p class="font-semibold">{task.title}</p>
            <p class="text-xs text-surface-500">{task.action}</p>
          </div>
          <button
            class="btn btn-xs preset-tonal uppercase tracking-[0.3em]"
            type="button"
            onclick={() => handleClick(task)}
          >
            {task.buttonLabel ?? 'View'}
          </button>
        </li>
      {/each}
    </ul>
  {:else}
    <div class="rounded border border-dashed border-surface-700/60 bg-surface-900/30 p-4 text-center text-xs uppercase tracking-[0.3em] text-surface-500">
      {emptyMessage}
    </div>
  {/if}
</Panel>
