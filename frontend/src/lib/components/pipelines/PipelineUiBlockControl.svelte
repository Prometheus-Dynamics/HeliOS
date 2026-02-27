<script lang="ts">
  import type { PipelineDataType, PipelineNodeValue } from '$lib/types/pipeline';
  import type { PipelineUiItem, PipelineUiNodeDescriptor } from '$lib/features/pipelines/pipelineUiTypes';
  import PipelineUiControl from '$lib/components/pipelines/PipelineUiControl.svelte';
  import FaIcon from '$lib/components/icons/FaIcon.svelte';
  import { faGear, faGripVertical } from '@fortawesome/free-solid-svg-icons';

  const {
    item,
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
    onEditItem,
    onMoveItem,
    onDragOver,
    onDragLeave,
    onDrop,
    onDragStart,
    onDragEnd
  } = $props<{
    item: PipelineUiItem;
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
    onEditItem?: (item: PipelineUiItem, anchor?: { x: number; y: number }) => void;
    onMoveItem?: (sourceId: string, targetId: string, position: 'before' | 'after' | 'inside') => void;
    onDragOver: (event: DragEvent, item: PipelineUiItem) => void;
    onDragLeave: (event: DragEvent, item: PipelineUiItem) => void;
    onDrop: (event: DragEvent, item: PipelineUiItem) => void;
    onDragStart: (event: DragEvent, item: PipelineUiItem) => void;
    onDragEnd: () => void;
  }>();

  const wrapperClass = `relative group ${selectedItemId === item.id ? 'ring-1 ring-primary-400/70' : ''} ${draggingItemId === item.id ? 'opacity-40' : ''}`;
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
          aria-label="Drag control"
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
          aria-label="Edit control"
          onclick={(event) =>
            onEditItem?.(item, { x: (event as MouseEvent).clientX, y: (event as MouseEvent).clientY })
          }
        >
          <FaIcon icon={faGear} class="h-3 w-3" />
        </button>
      {/if}
    </div>
  {/if}
  <PipelineUiControl
    control={item as any}
    nodeDescriptors={nodeDescriptors}
    streamNodeOverrides={streamNodeOverrides}
    streamNodeErrors={streamNodeErrors}
    readNodeDraft={readNodeDraft}
    updateStreamNodeValue={updateStreamNodeValue}
    readLocalValue={readLocalValue}
    setLocalValue={setLocalValue}
  />
</div>
