<script lang="ts">
  import { onMount } from 'svelte';
  import type { Readable, Writable } from 'svelte/store';
  import type { GraphContextMenuState } from '$lib/features/pipelines/controller/types';
  import type { PipelineGraphPlan, PipelineRegistryEntry } from '$lib/types/pipeline';
  import type { PipelineGraphEdgeSelection } from '$lib';
  import { isPipelineInputNode, isPipelineOutputNode } from '$lib/features/pipelines/boundary';

  type GraphSelection = {
    nodeId: string | null;
    nodes: string[];
    edge: PipelineGraphEdgeSelection | { id: string } | null;
  };

  type Props = {
    editingPlan: Readable<PipelineGraphPlan | null>;
    graphContextMenu: Writable<GraphContextMenuState>;
    graphContextSearch: Writable<string>;
    contextRegistryOptions: Readable<PipelineRegistryEntry[]>;
    graphSelection: Readable<GraphSelection>;
    openRegistryPalette: () => void;
    openActionMenu: () => void;
    openBoundaryMenu: (params?: { direction?: 'input' | 'output' }) => void;
    openBoundaryEditor: (nodeId: string | null) => void;
    openGroupEditor: (nodeId: string | null) => void;
    closeGraphContextMenu: () => void;
    addNodeFromRegistry: (entryId: string) => void;
    addBoundaryDraftPort: () => void;
    removeBoundaryDraftPort: (portId: string) => void;
    setBoundaryDraftPortName: (portId: string, name: string) => void;
    setBoundaryDraftPortType: (portId: string, dataTypeKey: string) => void;
    setBoundaryDraftDirection: (direction: 'input' | 'output') => void;
    applyBoundaryDraft: () => void;
    setGroupDraftName: (name: string) => void;
    setGroupDraftSummary: (summary: string) => void;
    setGroupDraftColor: (color: string) => void;
    applyGroupDraft: () => void;
    setNodeMetadata: (nodeId: string, metadata: { name?: string; summary?: string }) => void;
    removeGraphNode: (nodeId: string | null) => void;
    removeGraphConnection: (edge: PipelineGraphEdgeSelection) => void;
    groupSelection: () => void;
    ungroupSelection: () => void;
    setGraphContextMenuElement: (element: HTMLDivElement | null) => void;
  };

  let {
    editingPlan,
    graphContextMenu,
    graphContextSearch,
    contextRegistryOptions,
    graphSelection,
    openRegistryPalette,
    openActionMenu,
    openBoundaryMenu,
    openBoundaryEditor,
    openGroupEditor,
    closeGraphContextMenu,
    addNodeFromRegistry,
    addBoundaryDraftPort,
    removeBoundaryDraftPort,
    setBoundaryDraftPortName,
    setBoundaryDraftPortType,
    setBoundaryDraftDirection,
    applyBoundaryDraft,
    setGroupDraftName,
    setGroupDraftSummary,
    setGroupDraftColor,
    applyGroupDraft,
    setNodeMetadata,
    removeGraphNode,
    removeGraphConnection,
    groupSelection,
    ungroupSelection,
    setGraphContextMenuElement
  }: Props = $props();

  let menuRef = $state<HTMLDivElement | null>(null);

  const handleClickOutside = (event: PointerEvent) => {
    if (!menuRef) return;
    const target = event.target as Node | null;
    if (target && menuRef.contains(target)) return;
    closeGraphContextMenu();
  };

  onMount(() => {
    const updateViewport = () => {
      viewport = { width: window.innerWidth, height: window.innerHeight };
    };
    updateViewport();
    window.addEventListener('resize', updateViewport);
    window.addEventListener('pointerdown', handleClickOutside, true);
    return () => {
      window.removeEventListener('resize', updateViewport);
      window.removeEventListener('pointerdown', handleClickOutside, true);
    };
  });

  $effect(() => {
    setGraphContextMenuElement(menuRef);
  });

  const selectedNodeId = $derived.by(() => {
    const menu = $graphContextMenu;
    if (menu.visible && menu.nodeId) return menu.nodeId;
    const selection = $graphSelection;
    if (selection.nodeId) return selection.nodeId;
    if (selection.nodes?.length === 1) return selection.nodes[0];
    return null;
  });

  const selectedNode = $derived.by(() => {
    const plan = $editingPlan;
    const nodeId = selectedNodeId;
    if (!plan || !nodeId) return null;
    return plan.nodes?.[nodeId] ?? null;
  });

  const canDeleteNode = $derived(Boolean(selectedNodeId));
  const canDeleteEdge = $derived(Boolean($graphSelection.edge));
  const canGroupSelection = $derived.by(() => ($graphSelection.nodes ?? []).filter(Boolean).length > 1);
  const isGroupNode = $derived.by(() => {
    const node = selectedNode;
    return Boolean(node && (node.backendId ?? '').toLowerCase() === 'pipeline:child');
  });
  const canUngroup = $derived.by(() => Boolean(selectedNode && isGroupNode && selectedNode.embedded));
  const isPipelineInput = $derived.by(() => isPipelineInputNode(selectedNode));
  const isPipelineOutput = $derived.by(() => isPipelineOutputNode(selectedNode));

  let nodeEditOpen = $state(false);
  let nodeEditNodeId = $state<string | null>(null);
  let nodeNameDraft = $state('');
  let nodeSummaryDraft = $state('');
  let viewport = $state({ width: 0, height: 0 });
  let nodeEditorAnchor = $state({ x: 0, y: 0, width: 0 });

  const closeNodeEditor = () => {
    nodeEditOpen = false;
    nodeEditNodeId = null;
  };

  const hydrateNodeDrafts = () => {
    const plan = $editingPlan;
    const nodeId = nodeEditNodeId ?? selectedNodeId;
    if (!plan || !nodeId) {
      nodeNameDraft = '';
      nodeSummaryDraft = '';
      return;
    }
    const node = plan.nodes?.[nodeId] ?? null;
    nodeNameDraft = node?.metadata?.name ?? '';
    nodeSummaryDraft = node?.metadata?.summary ?? '';
  };

  $effect(() => {
    if (!$graphContextMenu.visible && !nodeEditOpen) return;
    hydrateNodeDrafts();
  });

  const openNodeEditor = () => {
    if (!selectedNodeId) return;
    nodeEditNodeId = selectedNodeId;
    nodeEditOpen = true;
    const menu = $graphContextMenu;
    nodeEditorAnchor = menu.visible
      ? {
          x: menu.position.x,
          y: menu.position.y,
          width: menu.size.width
        }
      : { x: 0, y: 0, width: 0 };
    hydrateNodeDrafts();
    closeGraphContextMenu();
  };

  const nodeEditorStyle = $derived.by(() => {
    if (!nodeEditOpen) return '';
    const gap = 12;
    const width = 352;
    const safeX = Math.max(viewport.width, 0);
    const safeY = Math.max(viewport.height, 0);
    const menuX = nodeEditorAnchor.x;
    const menuY = nodeEditorAnchor.y;
    const menuW = nodeEditorAnchor.width;
    let left = menuX + menuW + gap;
    if (left + width > safeX - 16) {
      left = menuX - width - gap;
    }
    if (left < 16) {
      left = Math.max(16, safeX - width - 16);
    }
    let top = menuY;
    const maxTop = Math.max(16, safeY - 16 - 420);
    if (top > maxTop) top = maxTop;
    if (top < 16) top = 16;
    return `left:${left}px; top:${top}px;`;
  });

  const applyNodeDraft = () => {
    const nodeId = nodeEditNodeId ?? selectedNodeId;
    if (!nodeId) return;
    const name = nodeNameDraft.trim();
    const summary = nodeSummaryDraft.trim();
    setNodeMetadata(nodeId, {
      name: name || undefined,
      summary: summary || undefined
    });
    closeNodeEditor();
  };

  const handleDeleteNode = () => {
    const nodeId = selectedNodeId;
    if (nodeId) {
      removeGraphNode(nodeId);
    }
    closeGraphContextMenu();
  };

  const handleDeleteEdge = () => {
    const selection = $graphSelection;
    if (selection?.edge) {
      removeGraphConnection(selection.edge as PipelineGraphEdgeSelection);
    }
    closeGraphContextMenu();
  };
</script>

{#if $graphContextMenu.visible}
  <div
    class="fixed z-50 flex flex-col gap-3 rounded border border-surface-800/70 bg-surface-950/90 p-3 text-surface-100 shadow-2xl shadow-black/40"
    style={`left:${$graphContextMenu.position.x}px; top:${$graphContextMenu.position.y}px; width:${$graphContextMenu.size.width}px; max-height:${$graphContextMenu.size.height}px;`}
    bind:this={menuRef}
    role="menu"
    aria-label="Graph context menu"
  >
    {#if $graphContextMenu.mode === 'registry'}
      <div class="flex items-center justify-between gap-2">
        <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Add node</p>
        <button class="btn btn-3xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={openActionMenu}>Back</button>
      </div>
      <label class="flex flex-col gap-1 text-xs text-surface-200">
        <span class="text-micro-tight uppercase tracking-[0.25em] text-surface-500">Search</span>
        <input
          class="w-full rounded border border-surface-800 bg-surface-950 px-3 py-2 text-xs text-surface-100"
          type="search"
          placeholder="Node name or tag"
          bind:value={$graphContextSearch}
        />
      </label>
      <div class="flex flex-col gap-2 max-h-64 overflow-y-auto pr-1">
        {#if $contextRegistryOptions.length === 0}
          <p class="text-xs text-surface-500">No registry entries match.</p>
        {:else}
          {#each $contextRegistryOptions as entry (entry.id)}
            <button
              class="w-full rounded border border-surface-800/70 bg-surface-900/40 px-3 py-2 text-left text-micro uppercase tracking-[0.25em] text-surface-200 hover:border-primary-400/40 hover:text-primary-100"
              type="button"
              onclick={() => addNodeFromRegistry(entry.id)}
            >
              <span class="block text-[0.7rem]">{entry.metadata?.name ?? entry.id}</span>
              <span class="block text-micro-tight text-surface-500">{entry.id}</span>
            </button>
          {/each}
        {/if}
      </div>
      <div class="flex items-center justify-end gap-2">
        <button class="btn btn-3xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={closeGraphContextMenu}>Close</button>
      </div>
    {:else if $graphContextMenu.mode === 'boundary' && $graphContextMenu.boundary}
      {@const draft = $graphContextMenu.boundary}
      <div class="flex items-center justify-between gap-2">
        <p class="text-micro uppercase tracking-[0.3em] text-surface-500">
          {draft.direction === 'input' ? 'Pipeline inputs' : 'Pipeline outputs'}
        </p>
        <button class="btn btn-3xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={openActionMenu}>Back</button>
      </div>
      {#if !draft.nodeId}
        <div class="inline-flex rounded border border-surface-800 bg-surface-950/60">
          <button
            class={`px-3 py-1 text-micro-tight uppercase tracking-[0.2em] ${draft.direction === 'input' ? 'bg-surface-900/60 text-surface-100' : 'text-surface-400'}`}
            type="button"
            data-active={draft.direction === 'input' ? 'true' : undefined}
            onclick={() => setBoundaryDraftDirection('input')}
          >
            Input
          </button>
          <button
            class={`px-3 py-1 text-micro-tight uppercase tracking-[0.2em] ${draft.direction === 'output' ? 'bg-surface-900/60 text-surface-100' : 'text-surface-400'}`}
            type="button"
            data-active={draft.direction === 'output' ? 'true' : undefined}
            onclick={() => setBoundaryDraftDirection('output')}
          >
            Output
          </button>
        </div>
      {/if}
      <div class="flex flex-col gap-2 max-h-64 overflow-y-auto pr-1">
        {#each draft.ports as port (port.id)}
          <div class="grid grid-cols-[minmax(0,1fr)_auto_auto] items-center gap-2">
            <input
              class="w-full rounded border border-surface-800 bg-surface-950 px-3 py-2 text-xs text-surface-100"
              type="text"
              placeholder="Port name"
              value={port.name}
              oninput={(event) => setBoundaryDraftPortName(port.id, (event.currentTarget as HTMLInputElement).value)}
            />
            <span class="rounded border border-surface-700 bg-surface-950/60 px-2 py-1 text-micro-tight uppercase tracking-[0.2em] text-surface-400">Generic</span>
            <button
              class="btn btn-3xs preset-outline uppercase tracking-[0.3em] text-error-200 border-error-500/40"
              type="button"
              onclick={() => removeBoundaryDraftPort(port.id)}
            >
              Remove
            </button>
          </div>
        {/each}
        <button class="w-full rounded border border-surface-800/70 bg-surface-900/40 px-3 py-2 text-left text-micro uppercase tracking-[0.25em] text-surface-200 hover:border-primary-400/40 hover:text-primary-100" type="button" onclick={addBoundaryDraftPort}>
          <span class="block text-[0.7rem]">Add port</span>
        </button>
      </div>
      <div class="flex items-center justify-end gap-2">
        <button class="btn btn-3xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={closeGraphContextMenu}>Close</button>
        <button class="btn btn-3xs preset-filled-primary-500 uppercase tracking-[0.3em]" type="button" onclick={applyBoundaryDraft}>
          Apply
        </button>
      </div>
    {:else if $graphContextMenu.mode === 'group' && $graphContextMenu.group}
      {@const draft = $graphContextMenu.group}
      <div class="flex items-center justify-between gap-2">
        <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Group editor</p>
        <button class="btn btn-3xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={openActionMenu}>Back</button>
      </div>
      <div class="flex flex-col gap-3">
        <label class="flex flex-col gap-1 text-xs text-surface-200">
          <span class="text-micro-tight uppercase tracking-[0.25em] text-surface-500">Name</span>
          <input
            class="w-full rounded border border-surface-800 bg-surface-950 px-3 py-2 text-xs text-surface-100"
            type="text"
            placeholder="Group name"
            value={draft.name}
            oninput={(event) => setGroupDraftName((event.currentTarget as HTMLInputElement).value)}
          />
        </label>
        <label class="flex flex-col gap-1 text-xs text-surface-200">
          <span class="text-micro-tight uppercase tracking-[0.25em] text-surface-500">Summary</span>
          <textarea
            class="w-full rounded border border-surface-800 bg-surface-950 px-3 py-2 text-xs text-surface-100"
            rows="3"
            placeholder="Summary"
            value={draft.summary}
            oninput={(event) => setGroupDraftSummary((event.currentTarget as HTMLTextAreaElement).value)}
          ></textarea>
        </label>
        <label class="flex flex-col gap-1 text-xs text-surface-200">
          <span class="text-micro-tight uppercase tracking-[0.25em] text-surface-500">Accent color</span>
          <div class="grid grid-cols-[auto_minmax(0,1fr)] items-center gap-2">
            <input
              class="h-9 w-12 rounded border border-surface-800 bg-surface-950"
              type="color"
              value={draft.color || '#1f2937'}
              oninput={(event) => setGroupDraftColor((event.currentTarget as HTMLInputElement).value)}
            />
            <input
              class="w-full rounded border border-surface-800 bg-surface-950 px-3 py-2 text-xs text-surface-100"
              type="text"
              placeholder="#1f2937"
              value={draft.color}
              oninput={(event) => setGroupDraftColor((event.currentTarget as HTMLInputElement).value)}
            />
          </div>
        </label>
      </div>
      <div class="flex items-center justify-end gap-2">
        <button class="btn btn-3xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={closeGraphContextMenu}>Close</button>
        <button class="btn btn-3xs preset-filled-primary-500 uppercase tracking-[0.3em]" type="button" onclick={applyGroupDraft}>
          Apply
        </button>
      </div>
    {:else if $graphContextMenu.mode === 'actions'}
      <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Actions</p>
      <div class="flex flex-col gap-2 max-h-64 overflow-y-auto pr-1">
        <button class="w-full rounded border border-surface-800/70 bg-surface-900/40 px-3 py-2 text-left text-micro uppercase tracking-[0.25em] text-surface-200 hover:border-primary-400/40 hover:text-primary-100" type="button" onclick={openRegistryPalette}>
          <span class="block text-[0.7rem]">Add node</span>
        </button>
        {#if $graphContextMenu.port}
          <button class="w-full rounded border border-surface-800/70 bg-surface-900/40 px-3 py-2 text-left text-micro uppercase tracking-[0.25em] text-surface-200 hover:border-primary-400/40 hover:text-primary-100" type="button" onclick={() => openBoundaryMenu()}>
            <span class="block text-[0.7rem]">Create pipeline port</span>
            <span class="block text-micro-tight text-surface-500">From selection</span>
          </button>
        {/if}
        <button class="w-full rounded border border-surface-800/70 bg-surface-900/40 px-3 py-2 text-left text-micro uppercase tracking-[0.25em] text-surface-200 hover:border-primary-400/40 hover:text-primary-100" type="button" onclick={() => openBoundaryMenu({ direction: 'input' })}>
          <span class="block text-[0.7rem]">Add pipeline input</span>
        </button>
        <button class="w-full rounded border border-surface-800/70 bg-surface-900/40 px-3 py-2 text-left text-micro uppercase tracking-[0.25em] text-surface-200 hover:border-primary-400/40 hover:text-primary-100" type="button" onclick={() => openBoundaryMenu({ direction: 'output' })}>
          <span class="block text-[0.7rem]">Add pipeline output</span>
        </button>
        {#if isPipelineInput || isPipelineOutput}
          <button class="w-full rounded border border-surface-800/70 bg-surface-900/40 px-3 py-2 text-left text-micro uppercase tracking-[0.25em] text-surface-200 hover:border-primary-400/40 hover:text-primary-100" type="button" onclick={() => openBoundaryEditor(selectedNodeId)}>
            <span class="block text-[0.7rem]">Edit pipeline {isPipelineInput ? 'inputs' : 'outputs'}</span>
          </button>
        {/if}
        {#if canGroupSelection}
          <button class="w-full rounded border border-surface-800/70 bg-surface-900/40 px-3 py-2 text-left text-micro uppercase tracking-[0.25em] text-surface-200 hover:border-primary-400/40 hover:text-primary-100" type="button" onclick={groupSelection}>
            <span class="block text-[0.7rem]">Group selection</span>
          </button>
        {/if}
        {#if isGroupNode}
          <button class="w-full rounded border border-surface-800/70 bg-surface-900/40 px-3 py-2 text-left text-micro uppercase tracking-[0.25em] text-surface-200 hover:border-primary-400/40 hover:text-primary-100" type="button" onclick={() => openGroupEditor(selectedNodeId)}>
            <span class="block text-[0.7rem]">Edit group</span>
          </button>
        {/if}
        {#if selectedNodeId}
          <button class="w-full rounded border border-surface-800/70 bg-surface-900/40 px-3 py-2 text-left text-micro uppercase tracking-[0.25em] text-surface-200 hover:border-primary-400/40 hover:text-primary-100" type="button" onclick={openNodeEditor}>
            <span class="block text-[0.7rem]">Rename node</span>
          </button>
        {/if}
        {#if canUngroup}
          <button class="w-full rounded border border-surface-800/70 bg-surface-900/40 px-3 py-2 text-left text-micro uppercase tracking-[0.25em] text-surface-200 hover:border-primary-400/40 hover:text-primary-100" type="button" onclick={ungroupSelection}>
            <span class="block text-[0.7rem]">Ungroup</span>
          </button>
        {/if}
        {#if canDeleteNode}
          <button class="w-full rounded border border-error-500/40 bg-surface-900/40 px-3 py-2 text-left text-micro uppercase tracking-[0.25em] text-error-200 hover:border-error-400 hover:text-error-100" type="button" onclick={handleDeleteNode}>
            <span class="block text-[0.7rem]">Delete node</span>
          </button>
        {/if}
        {#if canDeleteEdge}
          <button class="w-full rounded border border-error-500/40 bg-surface-900/40 px-3 py-2 text-left text-micro uppercase tracking-[0.25em] text-error-200 hover:border-error-400 hover:text-error-100" type="button" onclick={handleDeleteEdge}>
            <span class="block text-[0.7rem]">Delete connection</span>
          </button>
        {/if}
      </div>
      <div class="flex items-center justify-end gap-2">
        <button class="btn btn-3xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={closeGraphContextMenu}>Close</button>
      </div>
    {:else}
      <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Context</p>
      <p class="text-xs text-surface-500">Menu unavailable.</p>
      <div class="flex items-center justify-end gap-2">
        <button class="btn btn-3xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={openActionMenu}>Back</button>
        <button class="btn btn-3xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={closeGraphContextMenu}>Close</button>
      </div>
    {/if}
  </div>
{/if}

{#if nodeEditOpen}
  <div class="fixed z-[60] w-[22rem] max-w-[90vw]" style={nodeEditorStyle}>
    <div class="flex max-h-[calc(100vh-8rem)] max-h-[calc(100svh-8rem)] max-h-[calc(100dvh-8rem)] flex-col rounded border border-surface-800/80 bg-surface-950/95 shadow-2xl shadow-black/50">
      <header class="flex items-start justify-between gap-4 border-b border-surface-800/70 p-3">
        <div>
          <p class="text-[0.7rem] uppercase tracking-[0.3em] text-white">Node Details</p>
          <p class="mt-2 text-xs leading-snug text-surface-400">Rename or describe the selected node.</p>
        </div>
        <button class="btn btn-3xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={closeNodeEditor}>
          Close
        </button>
      </header>
      <div class="min-h-0 flex-1 space-y-3 overflow-auto p-3">
        <label class="flex flex-col gap-1 text-xs text-surface-200">
          <span class="text-micro-tight uppercase tracking-[0.25em] text-surface-500">Name</span>
          <input
            class="w-full rounded border border-surface-800 bg-surface-950 px-3 py-2 text-xs text-surface-100"
            type="text"
            placeholder="Node name"
            value={nodeNameDraft}
            oninput={(event) => (nodeNameDraft = (event.currentTarget as HTMLInputElement).value)}
          />
        </label>
        <label class="flex flex-col gap-1 text-xs text-surface-200">
          <span class="text-micro-tight uppercase tracking-[0.25em] text-surface-500">Summary</span>
          <textarea
            class="w-full rounded border border-surface-800 bg-surface-950 px-3 py-2 text-xs text-surface-100"
            rows="4"
            placeholder="Summary"
            value={nodeSummaryDraft}
            oninput={(event) => (nodeSummaryDraft = (event.currentTarget as HTMLTextAreaElement).value)}
          ></textarea>
        </label>
      </div>
      <footer class="flex items-center justify-end gap-2 border-t border-surface-800/70 p-3">
        <button class="btn btn-3xs preset-ghost uppercase tracking-[0.3em]" type="button" onclick={closeNodeEditor}>
          Cancel
        </button>
        <button class="btn btn-3xs preset-filled-primary-500 uppercase tracking-[0.3em]" type="button" onclick={applyNodeDraft}>
          Apply
        </button>
      </footer>
    </div>
  </div>
{/if}
