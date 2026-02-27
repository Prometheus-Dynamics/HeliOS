<script lang="ts">
  import type { PipelineDataType, PipelineNodeValue } from '$lib/types/pipeline';
  import type { PipelineUiItem, PipelineUiNodeDescriptor, PipelineUiControl as PipelineUiControlType, PipelineUiTabsItem, PipelineUiStackItem } from '$lib/features/pipelines/pipelineUiTypes';
  import Self from '$lib/components/pipelines/PipelineUiBlocks.svelte';
  import PipelineUiBlockLayout from '$lib/components/pipelines/PipelineUiBlockLayout.svelte';
  import PipelineUiBlockControl from '$lib/components/pipelines/PipelineUiBlockControl.svelte';
  import PipelineUiBlockTabs from '$lib/components/pipelines/PipelineUiBlockTabs.svelte';
  import PipelineUiBlockGroup from '$lib/components/pipelines/PipelineUiBlockGroup.svelte';
  import PipelineUiBlockAccordion from '$lib/components/pipelines/PipelineUiBlockAccordion.svelte';
  import PipelineUiBlockStack from '$lib/components/pipelines/PipelineUiBlockStack.svelte';

  type Props = {
    items: ReadonlyArray<PipelineUiItem>;
    nodeDescriptors: PipelineUiNodeDescriptor[];
    streamNodeOverrides: Record<string, Record<string, PipelineNodeValue>>;
    streamNodeErrors?: Record<string, Record<string, string | null>>;
    readNodeDraft: (nodeId: string, portKey: string) => string | null;
    updateStreamNodeValue: (nodeId: string, portKey: string, dataType: PipelineDataType | null, raw: string) => void;
    readLocalValue: (key: string, fallback: string) => string;
    setLocalValue: (key: string, value: string) => void;
    grid?: boolean;
    editMode?: boolean;
    selectedItemId?: string | null;
    onEditItem?: (item: PipelineUiItem, anchor?: { x: number; y: number }) => void;
    onMoveItem?: (sourceId: string, targetId: string, position: 'before' | 'after' | 'inside') => void;
    onInsertItem?: (type: PipelineUiItem['type'], targetId: string, position: 'before' | 'after' | 'inside') => void;
  };

  let {
    items,
    nodeDescriptors,
    streamNodeOverrides,
    streamNodeErrors = {},
    readNodeDraft,
    updateStreamNodeValue,
    readLocalValue,
    setLocalValue,
    grid = false,
    editMode = false,
    selectedItemId = null,
    onEditItem,
    onMoveItem,
    onInsertItem
  }: Props = $props();

  const isControl = (item: PipelineUiItem): item is PipelineUiControlType =>
    item.type === 'slider' ||
    item.type === 'dual_slider' ||
    item.type === 'toggle' ||
    item.type === 'layout_toggle' ||
    item.type === 'select' ||
    item.type === 'input' ||
    item.type === 'color' ||
    item.type === 'hsv' ||
    item.type === 'hsv_range';

  const isTabs = (item: PipelineUiItem): item is PipelineUiTabsItem => item.type === 'tabs';
  const isStack = (item: PipelineUiItem): item is PipelineUiStackItem => item.type === 'stack';
  const LIBRARY_TYPES: PipelineUiItem['type'][] = [
    'title',
    'text',
    'divider',
    'slider',
    'dual_slider',
    'select',
    'toggle',
    'layout_toggle',
    'input',
    'color',
    'hsv',
    'hsv_range',
    'group',
    'accordion',
    'tabs',
    'stack'
  ];

  let dragOverId = $state<string | null>(null);
  let dragOverPosition = $state<'before' | 'after' | null>(null);
  let dragInsideId = $state<string | null>(null);
  let draggingItemId = $state<string | null>(null);
  const displayItems = $derived.by(() => buildPreviewItems(items));

  function readLibraryDragType(event: DragEvent): PipelineUiItem['type'] | null {
    const type =
      event.dataTransfer?.getData('application/helios-ui-item') || event.dataTransfer?.getData('text/plain');
    if (type && LIBRARY_TYPES.includes(type as PipelineUiItem['type'])) {
      return type as PipelineUiItem['type'];
    }
    if (typeof window !== 'undefined') {
      const fallback = (window as Window & { __heliosPipelineUiLibraryDragType?: string }).__heliosPipelineUiLibraryDragType;
      if (fallback && LIBRARY_TYPES.includes(fallback as PipelineUiItem['type'])) {
        return fallback as PipelineUiItem['type'];
      }
    }
    return null;
  }

  function handleDragStart(event: DragEvent, item: PipelineUiItem): void {
    if (!editMode || !onMoveItem || !item.id) return;
    event.dataTransfer?.setData('application/helios-ui-move', item.id);
    event.dataTransfer?.setData('text/plain', item.id);
    if (event.dataTransfer) {
      event.dataTransfer.effectAllowed = 'move';
    }
    const dragRoot = (event.currentTarget as HTMLElement).closest('[data-ui-item]') as HTMLElement | null;
    if (dragRoot && event.dataTransfer) {
      const rect = dragRoot.getBoundingClientRect();
      event.dataTransfer.setDragImage(dragRoot, rect.width / 2, rect.height / 2);
    }
    draggingItemId = item.id;
  }

  function handleDragEnd(): void {
    draggingItemId = null;
    dragOverId = null;
    dragOverPosition = null;
    dragInsideId = null;
  }

  function handleDragOver(event: DragEvent, item: PipelineUiItem): void {
    if (!editMode || (!onMoveItem && !onInsertItem) || !item.id) return;
    event.preventDefault();
    event.stopPropagation();
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const ratio = grid
      ? (event.clientX - rect.left) / rect.width
      : (event.clientY - rect.top) / rect.height;
    const targetSide = ratio < 0.5 ? 'before' : 'after';
    const lowSwitch = 0.3;
    const highSwitch = 0.7;
    if (dragOverId !== item.id) {
      dragOverId = item.id;
      dragOverPosition = targetSide;
    } else if (!dragOverPosition) {
      dragOverPosition = targetSide;
    } else if (dragOverPosition === 'before' && ratio >= highSwitch) {
      dragOverPosition = 'after';
    } else if (dragOverPosition === 'after' && ratio <= lowSwitch) {
      dragOverPosition = 'before';
    }
    dragInsideId = null;
    if (event.dataTransfer) {
      event.dataTransfer.dropEffect = readLibraryDragType(event) ? 'copy' : 'move';
    }
  }

  function handleDragLeave(event: DragEvent, item: PipelineUiItem): void {
    if (!editMode || (!onMoveItem && !onInsertItem) || !item.id) return;
    event.stopPropagation();
    const nextTarget = event.relatedTarget as Node | null;
    if (!nextTarget) {
      return;
    }
    if ((event.currentTarget as HTMLElement).contains(nextTarget)) {
      return;
    }
  }

  function handleDrop(event: DragEvent, item: PipelineUiItem): void {
    if (!editMode || (!onMoveItem && !onInsertItem) || !item.id) return;
    event.preventDefault();
    event.stopPropagation();
    const position = dragOverPosition ?? 'after';
    const libraryType = readLibraryDragType(event);
    const moveId = event.dataTransfer?.getData('application/helios-ui-move');
    const fallbackMove = event.dataTransfer?.getData('text/plain');
    const targetId = dragOverId ?? item.id;
    if (libraryType && onInsertItem) {
      onInsertItem(libraryType, targetId, position);
    } else if (onMoveItem) {
      const sourceId = moveId || fallbackMove || draggingItemId;
      if (sourceId && sourceId !== targetId) {
        onMoveItem(sourceId, targetId, position);
      }
    }
    dragOverId = null;
    dragOverPosition = null;
    dragInsideId = null;
  }

  function handleInsideDragOver(event: DragEvent, item: PipelineUiItem): void {
    if (!editMode || (!onMoveItem && !onInsertItem) || !item.id) return;
    event.preventDefault();
    event.stopPropagation();
    dragInsideId = item.id;
    dragOverId = null;
    dragOverPosition = null;
    if (event.dataTransfer) {
      event.dataTransfer.dropEffect = readLibraryDragType(event) ? 'copy' : 'move';
    }
  }

  function buildPreviewItems(list: ReadonlyArray<PipelineUiItem>): PipelineUiItem[] {
    if (!draggingItemId || !dragOverId || !dragOverPosition) return [...list];
    if (draggingItemId === dragOverId) return [...list];
    const sourceIndex = list.findIndex((item) => item.id === draggingItemId);
    const targetIndex = list.findIndex((item) => item.id === dragOverId);
    if (sourceIndex === -1 || targetIndex === -1) return [...list];
    const dragged = list[sourceIndex];
    const without = list.filter((item) => item.id !== draggingItemId);
    const targetIndexWithout = without.findIndex((item) => item.id === dragOverId);
    if (targetIndexWithout === -1) return [...list];
    const insertIndex = targetIndexWithout + (dragOverPosition === 'after' ? 1 : 0);
    const next = [...without];
    next.splice(insertIndex, 0, dragged);
    return next;
  }

  function handleInsideDragLeave(event: DragEvent, item: PipelineUiItem): void {
    if (!editMode || (!onMoveItem && !onInsertItem) || !item.id) return;
    event.stopPropagation();
    const nextTarget = event.relatedTarget as Node | null;
    if (!nextTarget) {
      return;
    }
    if ((event.currentTarget as HTMLElement).contains(nextTarget)) {
      return;
    }
  }

  function handleInsideDrop(event: DragEvent, item: PipelineUiItem): void {
    if (!editMode || (!onMoveItem && !onInsertItem) || !item.id) return;
    event.preventDefault();
    event.stopPropagation();
    const libraryType = readLibraryDragType(event);
    if (libraryType && onInsertItem) {
      onInsertItem(libraryType, item.id, 'inside');
    } else if (onMoveItem) {
      const sourceId =
        event.dataTransfer?.getData('application/helios-ui-move') ||
        event.dataTransfer?.getData('text/plain') ||
        draggingItemId;
      if (sourceId && sourceId !== item.id) {
        onMoveItem(sourceId, item.id, 'inside');
      }
    }
    dragInsideId = null;
    dragOverId = null;
    dragOverPosition = null;
  }
</script>

<div class={grid ? 'grid gap-2 md:grid-cols-2' : 'space-y-2'}>
  {#each displayItems as item, index (item.id ?? `${item.type}-${index}`)}
    {#if item.type === 'title' || item.type === 'text' || item.type === 'divider'}
      <PipelineUiBlockLayout
        {item}
        {grid}
        {editMode}
        {selectedItemId}
        {draggingItemId}
        {dragOverId}
        {dragOverPosition}
        onEditItem={onEditItem}
        onMoveItem={onMoveItem}
        onDragOver={handleDragOver}
        onDragLeave={handleDragLeave}
        onDrop={handleDrop}
        onDragStart={handleDragStart}
        onDragEnd={handleDragEnd}
      />
    {:else if item.type === 'group'}
      <PipelineUiBlockGroup
        {item}
        Renderer={Self}
        {nodeDescriptors}
        {streamNodeOverrides}
        {streamNodeErrors}
        {readNodeDraft}
        {updateStreamNodeValue}
        {readLocalValue}
        {setLocalValue}
        {grid}
        {editMode}
        {selectedItemId}
        {draggingItemId}
        {dragOverId}
        {dragOverPosition}
        {dragInsideId}
        onEditItem={onEditItem}
        onMoveItem={onMoveItem}
        onInsertItem={onInsertItem}
        onDragOver={handleDragOver}
        onDragLeave={handleDragLeave}
        onDrop={handleDrop}
        onDragStart={handleDragStart}
        onDragEnd={handleDragEnd}
        onInsideDragOver={handleInsideDragOver}
        onInsideDragLeave={handleInsideDragLeave}
        onInsideDrop={handleInsideDrop}
      />
    {:else if item.type === 'accordion'}
      <PipelineUiBlockAccordion
        {item}
        Renderer={Self}
        {nodeDescriptors}
        {streamNodeOverrides}
        {streamNodeErrors}
        {readNodeDraft}
        {updateStreamNodeValue}
        {readLocalValue}
        {setLocalValue}
        {grid}
        {editMode}
        {selectedItemId}
        {draggingItemId}
        {dragOverId}
        {dragOverPosition}
        {dragInsideId}
        onEditItem={onEditItem}
        onMoveItem={onMoveItem}
        onInsertItem={onInsertItem}
        onDragOver={handleDragOver}
        onDragLeave={handleDragLeave}
        onDrop={handleDrop}
        onDragStart={handleDragStart}
        onDragEnd={handleDragEnd}
        onInsideDragOver={handleInsideDragOver}
        onInsideDragLeave={handleInsideDragLeave}
        onInsideDrop={handleInsideDrop}
      />
    {:else if isControl(item)}
      <PipelineUiBlockControl
        {item}
        {nodeDescriptors}
        {streamNodeOverrides}
        {streamNodeErrors}
        {readNodeDraft}
        {updateStreamNodeValue}
        {readLocalValue}
        {setLocalValue}
        {grid}
        {editMode}
        {selectedItemId}
        {draggingItemId}
        {dragOverId}
        {dragOverPosition}
        onEditItem={onEditItem}
        onMoveItem={onMoveItem}
        onDragOver={handleDragOver}
        onDragLeave={handleDragLeave}
        onDrop={handleDrop}
        onDragStart={handleDragStart}
        onDragEnd={handleDragEnd}
      />
    {:else if isTabs(item)}
      <PipelineUiBlockTabs
        {item}
        {nodeDescriptors}
        {streamNodeOverrides}
        {streamNodeErrors}
        {readNodeDraft}
        {updateStreamNodeValue}
        {readLocalValue}
        {setLocalValue}
        {grid}
        {editMode}
        {selectedItemId}
        {draggingItemId}
        {dragOverId}
        {dragOverPosition}
        onEditItem={onEditItem}
        onMoveItem={onMoveItem}
        onInsertItem={onInsertItem}
        onDragOver={handleDragOver}
        onDragLeave={handleDragLeave}
        onDrop={handleDrop}
        onDragStart={handleDragStart}
        onDragEnd={handleDragEnd}
      />
    {:else if isStack(item)}
      <PipelineUiBlockStack
        {item}
        Renderer={Self}
        {nodeDescriptors}
        {streamNodeOverrides}
        {streamNodeErrors}
        {readNodeDraft}
        {updateStreamNodeValue}
        {readLocalValue}
        {setLocalValue}
        {grid}
        {editMode}
        {selectedItemId}
        {draggingItemId}
        {dragOverId}
        {dragOverPosition}
        {dragInsideId}
        onEditItem={onEditItem}
        onMoveItem={onMoveItem}
        onInsertItem={onInsertItem}
        onDragOver={handleDragOver}
        onDragLeave={handleDragLeave}
        onDrop={handleDrop}
        onDragStart={handleDragStart}
        onDragEnd={handleDragEnd}
        onInsideDragOver={handleInsideDragOver}
        onInsideDragLeave={handleInsideDragLeave}
        onInsideDrop={handleInsideDrop}
      />
    {/if}
  {/each}
</div>
