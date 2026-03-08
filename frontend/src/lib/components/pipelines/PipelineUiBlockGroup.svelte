<script lang="ts">
  import type { Component } from 'svelte';
  import type { PipelineDataType, PipelineNodeValue } from '$lib/types/pipeline';
  import type { PipelineUiGroup, PipelineUiItem, PipelineUiNodeDescriptor } from '$lib/features/pipelines/pipelineUiTypes';
  import FaIcon from '$lib/components/icons/FaIcon.svelte';
  import { faGear, faGripVertical } from '@fortawesome/free-solid-svg-icons';

  type RendererProps = {
    items: ReadonlyArray<PipelineUiItem>;
    nodeDescriptors: PipelineUiNodeDescriptor[];
    streamNodeOverrides: Record<string, Record<string, PipelineNodeValue>>;
    streamNodeErrors: Record<string, Record<string, string | null>>;
    readNodeDraft: (nodeId: string, portKey: string) => string | null;
    updateStreamNodeValue: (nodeId: string, portKey: string, dataType: PipelineDataType | null, raw: string) => void;
    readLocalValue: (key: string, fallback: string) => string;
    setLocalValue: (key: string, value: string) => void;
    grid: boolean;
    editMode: boolean;
    selectedItemId: string | null;
    onEditItem?: (item: PipelineUiItem, anchor?: { x: number; y: number }) => void;
    onMoveItem?: (sourceId: string, targetId: string, position: 'before' | 'after' | 'inside') => void;
    onInsertItem?: (type: PipelineUiItem['type'], targetId: string, position: 'before' | 'after' | 'inside') => void;
  };

  const {
    item,
    Renderer,
    nodeDescriptors,
    streamNodeOverrides,
    streamNodeErrors,
    readNodeDraft,
    updateStreamNodeValue,
    readLocalValue,
    setLocalValue,
    grid,
    editMode,
    selectedItemId,
    draggingItemId,
    dragOverId,
    dragOverPosition,
    dragInsideId,
    onEditItem,
    onMoveItem,
    onInsertItem,
    onDragOver,
    onDragLeave,
    onDrop,
    onDragStart,
    onDragEnd,
    onInsideDragOver,
    onInsideDragLeave,
    onInsideDrop
  } = $props<{
    item: PipelineUiGroup;
    Renderer: Component<RendererProps>;
    nodeDescriptors: PipelineUiNodeDescriptor[];
    streamNodeOverrides: Record<string, Record<string, PipelineNodeValue>>;
    streamNodeErrors: Record<string, Record<string, string | null>>;
    readNodeDraft: (nodeId: string, portKey: string) => string | null;
    updateStreamNodeValue: (nodeId: string, portKey: string, dataType: PipelineDataType | null, raw: string) => void;
    readLocalValue: (key: string, fallback: string) => string;
    setLocalValue: (key: string, value: string) => void;
    grid: boolean;
    editMode: boolean;
    selectedItemId: string | null;
    draggingItemId: string | null;
    dragOverId: string | null;
    dragOverPosition: 'before' | 'after' | null;
    dragInsideId: string | null;
    onEditItem?: (item: PipelineUiItem, anchor?: { x: number; y: number }) => void;
    onMoveItem?: (sourceId: string, targetId: string, position: 'before' | 'after' | 'inside') => void;
    onInsertItem?: (type: PipelineUiItem['type'], targetId: string, position: 'before' | 'after' | 'inside') => void;
    onDragOver: (event: DragEvent, item: PipelineUiItem) => void;
    onDragLeave: (event: DragEvent, item: PipelineUiItem) => void;
    onDrop: (event: DragEvent, item: PipelineUiItem) => void;
    onDragStart: (event: DragEvent, item: PipelineUiItem) => void;
    onDragEnd: () => void;
    onInsideDragOver: (event: DragEvent, item: PipelineUiItem) => void;
    onInsideDragLeave: (event: DragEvent, item: PipelineUiItem) => void;
    onInsideDrop: (event: DragEvent, item: PipelineUiItem) => void;
  }>();

  const wrapperClass = $derived.by(
    () =>
      `${grid ? 'md:col-span-2' : ''} relative group ${editMode ? 'rounded border border-dashed border-surface-700/70 p-2' : ''} ${selectedItemId === item.id ? 'ring-1 ring-primary-400/70' : ''} ${draggingItemId === item.id ? 'opacity-40' : ''}`
  );
</script>

<div data-ui-item role="group" class={wrapperClass} ondragover={(event) => onDragOver(event, item)} ondragleave={(event) => onDragLeave(event, item)} ondrop={(event) => onDrop(event, item)}>
  {#if editMode && dragOverId === item.id}
    {#if grid}
      <div class={`pointer-events-none absolute top-0 bottom-0 w-0.5 bg-primary-400 ${dragOverPosition === 'before' ? 'left-0' : 'right-0'}`}></div>
    {:else}
      <div class={`pointer-events-none absolute left-0 right-0 h-0.5 bg-primary-400 ${dragOverPosition === 'before' ? 'top-0' : 'bottom-0'}`}></div>
    {/if}
  {/if}
  {#if editMode && item.id}
    <div class={`absolute right-2 top-2 z-10 flex items-center gap-1 transition ${editMode ? 'opacity-100' : 'opacity-0 group-hover:opacity-100'}`}>
      {#if onMoveItem}
        <button
          class="rounded border border-surface-700/70 bg-surface-900/80 p-1 text-surface-200"
          type="button"
          aria-label="Drag group"
          draggable={true}
          ondragstart={(event) => onDragStart(event, item)}
          ondragend={onDragEnd}
        >
          <FaIcon icon={faGripVertical} class="h-3 w-3" />
        </button>
      {/if}
      {#if onEditItem}
        <button
          class="rounded border border-surface-700/70 bg-surface-900/80 p-1 text-surface-200"
          type="button"
          aria-label="Edit group"
          onclick={(event) =>
            onEditItem?.(item, { x: (event as MouseEvent).clientX, y: (event as MouseEvent).clientY })
          }
        >
          <FaIcon icon={faGear} class="h-3 w-3" />
        </button>
      {/if}
    </div>
  {/if}
  <div class="rounded border border-surface-800/70 bg-surface-900/40 p-3">
    <div class="min-w-0">
      <p class="truncate text-xs font-semibold text-surface-100">{item.title}</p>
      {#if item.description}
        <p class="truncate text-micro-tight text-surface-600">{item.description}</p>
      {/if}
    </div>
    <div class="mt-3">
      <Renderer
        items={item.items}
        nodeDescriptors={nodeDescriptors}
        streamNodeOverrides={streamNodeOverrides}
        streamNodeErrors={streamNodeErrors}
        readNodeDraft={readNodeDraft}
        updateStreamNodeValue={updateStreamNodeValue}
        readLocalValue={readLocalValue}
        setLocalValue={setLocalValue}
        grid={true}
        editMode={editMode}
        selectedItemId={selectedItemId}
        onEditItem={onEditItem}
        onMoveItem={onMoveItem}
        onInsertItem={onInsertItem}
      />
      {#if editMode && item.id}
        <div
          class={`mt-2 rounded border border-dashed px-2 py-2 text-micro-tight ${
            dragInsideId === item.id || item.items.length === 0
              ? 'border-primary-400/70 bg-primary-500/10 text-primary-100'
              : 'border-surface-800/40 text-transparent'
          }`}
          role="group"
          aria-label="Drop inside group"
          ondragover={(event) => onInsideDragOver(event, item)}
          ondragleave={(event) => onInsideDragLeave(event, item)}
          ondrop={(event) => onInsideDrop(event, item)}
        >
          Drop inside group
        </div>
      {/if}
    </div>
  </div>
</div>
