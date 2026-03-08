<script lang="ts">
  import type { PipelineDataType, PipelineNodeValue } from '$lib/types/pipeline';
  import type { PipelineUiItem, PipelineUiNodeDescriptor, PipelineUiTab } from '$lib/features/pipelines/pipelineUiTypes';
  import PipelineUiBlocks from '$lib/components/pipelines/PipelineUiBlocks.svelte';
  import OverridesActions from './OverridesActions.svelte';

  type BindingWarning = {
    id: string;
    label: string;
    reason: string;
  };

  type Props = {
    streamId: string | null;
    streamError: string | null;
    editMode: boolean;
    tabs: ReadonlyArray<PipelineUiTab> | null;
    isSearching: boolean;
    activeTabId: string | null;
    onTabSelect: (id: string) => void;
    bindingWarnings: BindingWarning[];
    displayItems: PipelineUiItem[];
    nodeDescriptors: PipelineUiNodeDescriptor[];
    streamNodeOverrides: Record<string, Record<string, PipelineNodeValue>>;
    streamNodeErrors: Record<string, Record<string, string | null>>;
    readNodeDraft: (nodeId: string, portKey: string) => string | null;
    updateStreamNodeValue: (nodeId: string, portKey: string, dataType: PipelineDataType, raw: string) => void;
    readLocalValue: (key: string, fallback: string) => string;
    setLocalValue: (key: string, value: string) => void;
    selectedItemId: string | null;
    onEditItem?: (item: PipelineUiItem, anchor?: { x: number; y: number }) => void;
    onMoveItem: (sourceId: string, targetId: string, position: 'before' | 'after' | 'inside') => void;
    onInsertItem: (type: PipelineUiItem['type'], targetId: string, position: 'before' | 'after' | 'inside') => void;
    onDragOver: (event: DragEvent) => void;
    onDrop: (event: DragEvent) => void;
  };

  const {
    streamId,
    streamError,
    editMode,
    tabs,
    isSearching,
    activeTabId,
    onTabSelect,
    bindingWarnings,
    displayItems,
    nodeDescriptors,
    streamNodeOverrides,
    streamNodeErrors,
    readNodeDraft,
    updateStreamNodeValue,
    readLocalValue,
    setLocalValue,
    selectedItemId,
    onEditItem,
    onMoveItem,
    onInsertItem,
    onDragOver,
    onDrop
  }: Props = $props();
</script>

<div class="space-y-3">
  <OverridesActions
    {streamError}
    {editMode}
    {bindingWarnings}
    {tabs}
    {isSearching}
    {activeTabId}
    onTabSelect={onTabSelect}
  />

  <div
    class={editMode ? 'rounded border border-dashed border-surface-800/70 bg-surface-900/30 p-2' : ''}
    role="group"
    ondragover={onDragOver}
    ondrop={onDrop}
  >
    <PipelineUiBlocks
      items={displayItems}
      {nodeDescriptors}
      {streamNodeOverrides}
      {streamNodeErrors}
      {readNodeDraft}
      {updateStreamNodeValue}
      {readLocalValue}
      {setLocalValue}
      editMode={editMode && !isSearching}
      {selectedItemId}
      {onEditItem}
      onMoveItem={onMoveItem}
      onInsertItem={onInsertItem}
    />
    {#if isSearching && displayItems.length === 0}
      <div class="mt-3 rounded border border-surface-800/60 bg-surface-900/60 px-3 py-2 text-xs text-surface-400">
        No controls match your search.
      </div>
    {/if}
  </div>

  {#if !streamId}
    <p class="text-micro-tight text-surface-500">No stream selected.</p>
  {/if}
</div>
