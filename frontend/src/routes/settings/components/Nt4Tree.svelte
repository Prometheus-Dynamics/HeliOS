<script lang="ts">
  import Self from './Nt4Tree.svelte';
  import type { NtTreeNode } from './nt4TreeTypes';
  import { SvelteSet } from 'svelte/reactivity';

  type Props = {
    nodes: NtTreeNode[];
    selectedTopic: string | null;
    openFolders: Set<string>;
    toggleFolder: (path: string) => void;
    selectTopic: (topic: string) => void;
    depth?: number;
  };

  let props: Partial<Props> = $props();
  const nodes = $derived(Array.isArray(props.nodes) ? props.nodes : []);
  const selectedTopic = $derived(typeof props.selectedTopic === 'string' ? props.selectedTopic : null);
  const openFolders = $derived(props.openFolders instanceof Set ? props.openFolders : new SvelteSet<string>());

  const depth = $derived(typeof props.depth === 'number' ? props.depth : 0);

  function isOpen(path: string): boolean {
    return openFolders.has(path);
  }

  function toggle(path: string): void {
    props.toggleFolder?.(path);
  }

  function select(topic: string): void {
    props.selectTopic?.(topic);
  }
</script>

<div class="space-y-1">
  {#each nodes as node (node.path)}
    {#if node.kind === 'folder'}
      <div class={`rounded border border-surface-800/60 bg-surface-950/20 ${depth ? 'ml-3' : ''}`}>
        <button
          type="button"
          class="flex w-full items-center justify-between gap-3 px-3 py-2 text-left text-xs uppercase tracking-[0.25em] text-surface-200 hover:bg-surface-800/30"
          onclick={() => toggle(node.path)}
        >
          <span class="min-w-0 truncate">
            <span class={`inline-block w-4 text-surface-500 transition-transform ${isOpen(node.path) ? 'rotate-90' : ''}`}>▸</span>
            {node.name}
          </span>
          <span class="text-micro-tight text-surface-500">{node.topicCount}</span>
        </button>

        {#if isOpen(node.path)}
          <div class="border-t border-surface-800/60 px-2 py-2">
            <Self
              nodes={node.children ?? []}
              selectedTopic={selectedTopic}
              openFolders={openFolders}
              toggleFolder={(path) => toggle(path)}
              selectTopic={(topic) => select(topic)}
              depth={depth + 1}
            />
          </div>
        {/if}
      </div>
    {:else}
      <button
        type="button"
        class={`flex w-full items-center justify-between gap-3 rounded border border-surface-800/60 px-3 py-2 text-left text-xs transition hover:bg-surface-800/30 ${
          selectedTopic === node.path ? 'bg-surface-800/30 text-primary-100' : 'text-primary-200'
        } ${depth ? 'ml-3' : ''}`}
        onclick={() => select(node.path)}
      >
        <span class="min-w-0 truncate">{node.name}</span>
        {#if node.dataType}
          <span class="shrink-0 font-mono text-micro-tight text-surface-500">{node.dataType}</span>
        {/if}
      </button>
    {/if}
  {/each}
</div>
