<script lang="ts">
  import type { PipelineDataType, PipelineNodeValue } from '$lib/types/pipeline';
  import type { PipelineUiTabsItem, PipelineUiItem, PipelineUiNodeDescriptor } from '$lib/features/pipelines/pipelineUiTypes';
  import PipelineUiBlocks from '$lib/components/pipelines/PipelineUiBlocks.svelte';

  type Props = {
    item: PipelineUiTabsItem;
    nodeDescriptors: PipelineUiNodeDescriptor[];
    streamNodeOverrides: Record<string, Record<string, PipelineNodeValue>>;
    streamNodeErrors?: Record<string, Record<string, string | null>>;
    readNodeDraft: (nodeId: string, portKey: string) => string | null;
    updateStreamNodeValue: (nodeId: string, portKey: string, dataType: PipelineDataType | null, raw: string) => void;
    readLocalValue: (key: string, fallback: string) => string;
    setLocalValue: (key: string, value: string) => void;
    editMode?: boolean;
    selectedItemId?: string | null;
    onEditItem?: (item: PipelineUiItem, anchor?: { x: number; y: number }) => void;
    onMoveItem?: (sourceId: string, targetId: string, position: 'before' | 'after' | 'inside') => void;
    onInsertItem?: (type: PipelineUiItem['type'], targetId: string, position: 'before' | 'after' | 'inside') => void;
  };

  let {
    item,
    nodeDescriptors,
    streamNodeOverrides,
    streamNodeErrors = {},
    readNodeDraft,
    updateStreamNodeValue,
    readLocalValue,
    setLocalValue,
    editMode = false,
    selectedItemId = null,
    onEditItem,
    onMoveItem,
    onInsertItem
  }: Props = $props();

  let activeTabId = $state<string>('');

  const activeTab = $derived.by(() => {
    if (!item.tabs.length) return null;
    return item.tabs.find((tab) => tab.id === activeTabId) ?? item.tabs[0];
  });

  const activeItems = $derived.by<PipelineUiItem[]>(() => (activeTab ? [...activeTab.content] : []));

  $effect(() => {
    if (!item.tabs.length) return;
    if (!activeTabId || !item.tabs.find((tab) => tab.id === activeTabId)) {
      activeTabId = item.tabs[0].id;
    }
  });
</script>

<div class="rounded border border-surface-800/70 bg-surface-900/40 p-3">
  <div class="flex items-start justify-between gap-3">
    <div>
      {#if item.title}
        <p class="text-xs uppercase tracking-[0.3em] text-surface-500">{item.title}</p>
      {/if}
      {#if item.description}
        <p class="text-xs text-surface-400">{item.description}</p>
      {/if}
    </div>
  </div>

  <div class="mt-3 flex flex-wrap gap-2">
    {#each item.tabs as tab (tab.id)}
      <button
        class={`inline-flex h-7 items-center justify-center rounded border px-3 text-micro-tight uppercase tracking-[0.3em] transition ${
          activeTab?.id === tab.id
            ? 'bg-primary-500/20 text-primary-100 border-primary-500/60'
            : 'border-surface-700 text-surface-300 hover:text-primary-200 hover:border-primary-400/60'
        }`}
        type="button"
        onclick={() => (activeTabId = tab.id)}
      >
        {tab.title}
      </button>
    {/each}
  </div>

  <div class="mt-3">
    <PipelineUiBlocks
      items={activeItems}
      nodeDescriptors={nodeDescriptors}
      streamNodeOverrides={streamNodeOverrides}
      streamNodeErrors={streamNodeErrors}
      readNodeDraft={readNodeDraft}
      updateStreamNodeValue={updateStreamNodeValue}
      readLocalValue={readLocalValue}
      setLocalValue={setLocalValue}
      editMode={editMode}
      selectedItemId={selectedItemId}
      onEditItem={onEditItem}
      onMoveItem={onMoveItem}
      onInsertItem={onInsertItem}
    />
  </div>
</div>
