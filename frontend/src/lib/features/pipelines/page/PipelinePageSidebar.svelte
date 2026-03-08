<script lang="ts">
  import type { PipelineOverviewPipeline } from '$lib/types/pipeline';
  import type { Readable, Writable } from 'svelte/store';

  type PipelineListEntry = {
    id: string;
    name: string;
    revision?: string | null;
    issueCount?: number | null;
    appearance?: unknown;
  };

  type PipelinePageSidebarCtx = {
    PipelineListPanel: (typeof import('./PipelineListPanel.svelte'))['default'];
    ideBindings: { customNodeSearch: string };
    pipelinesRefreshing: boolean;
    isInitialLoading: boolean;
    pipelineListItems: Readable<PipelineListEntry[]>;
    pipelineMap: Readable<Record<string, PipelineOverviewPipeline>>;
    selectedPipelineId: Readable<string | null>;
    pipelineSearch: Writable<string>;
    openCreateModal: () => void;
    setSelectedPipeline: (pipelineId: string) => void;
    openPipelineIconModal: (pipelineId: string) => void;
    handlePipelineCardKeydown: (event: KeyboardEvent, pipelineId: string) => void;
    openDeleteModal?: (id: string) => void;
  };

  const { ctx } = $props<{ ctx: Record<string, unknown> }>();
  const getPageCtx = (): PipelinePageSidebarCtx => ctx as PipelinePageSidebarCtx;
  const PipelineListPanel = $derived.by(() => getPageCtx().PipelineListPanel);
  const ideBindings = $derived.by(() => getPageCtx().ideBindings);
  const pipelinesRefreshing = $derived.by(() => getPageCtx().pipelinesRefreshing);
  const isInitialLoading = $derived.by(() => getPageCtx().isInitialLoading);
  const pipelineListItems = $derived.by(() => getPageCtx().pipelineListItems);
  const pipelineMap = $derived.by(() => getPageCtx().pipelineMap);
  const selectedPipelineId = $derived.by(() => getPageCtx().selectedPipelineId);
  const pipelineSearch = $derived.by(() => getPageCtx().pipelineSearch);
  const openCreateModal = $derived.by(() => getPageCtx().openCreateModal);
  const setSelectedPipeline = $derived.by(() => getPageCtx().setSelectedPipeline);
  const openPipelineIconModal = $derived.by(() => getPageCtx().openPipelineIconModal);
  const handlePipelineCardKeydown = $derived.by(() => getPageCtx().handlePipelineCardKeydown);

  const handleOpenDeleteModal = (id: string) => {
    getPageCtx().openDeleteModal?.(id);
  };
</script>

<PipelineListPanel
  bind:customNodeSearch={ideBindings.customNodeSearch}
  {pipelinesRefreshing}
  {isInitialLoading}
  pipelineListItems={$pipelineListItems}
  pipelineMap={$pipelineMap}
  selectedPipelineId={$selectedPipelineId}
  {pipelineSearch}
  onOpenCreateModal={openCreateModal}
  onSelectPipeline={setSelectedPipeline}
  onOpenPipelineIcon={openPipelineIconModal}
  onOpenDeleteModal={handleOpenDeleteModal}
  onPipelineCardKeydown={handlePipelineCardKeydown}
/>
