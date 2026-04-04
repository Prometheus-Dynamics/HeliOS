<script lang="ts">
  import type { PipelinePageSidebarContext } from './pipelinePageTypes';

  const { base } = $props<{ base: PipelinePageSidebarContext }>();
  const PipelineListPanel = $derived.by(() => base.PipelineListPanel);
  const ideBindings = $derived.by(() => base.ideBindings);
  const pipelinesRefreshing = $derived.by(() => base.pipelinesRefreshing);
  const isInitialLoading = $derived.by(() => base.isInitialLoading);
  const pipelineListItems = $derived.by(() => base.pipelineListItems);
  const pipelineMap = $derived.by(() => base.pipelineMap);
  const selectedPipelineId = $derived.by(() => base.selectedPipelineId);
  const pipelineSearch = $derived.by(() => base.pipelineSearch);
  const openCreateModal = $derived.by(() => base.openCreateModal);
  const setSelectedPipeline = $derived.by(() => base.setSelectedPipeline);
  const openPipelineIconModal = $derived.by(() => base.openPipelineIconModal);
  const handlePipelineCardKeydown = $derived.by(() => base.handlePipelineCardKeydown);

  const handleOpenDeleteModal = (id: string) => {
    base.openDeleteModal?.(id);
  };
</script>

{#if PipelineListPanel}
  {@const ListPanel = PipelineListPanel}
  <ListPanel
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
{:else}
  <aside class="flex min-h-0 min-w-0 flex-col rounded border border-surface-800/60 bg-surface-950/60 p-4 text-xs uppercase tracking-[0.24em] text-surface-400 xl:w-[22rem]">
    Loading pipelines…
  </aside>
{/if}
