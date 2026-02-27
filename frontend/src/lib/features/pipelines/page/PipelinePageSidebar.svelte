<script lang="ts">
  const { ctx } = $props<{ ctx: any }>();

  let {
    PipelineListPanel,
    activeTab,
    ideBindings,
    visiblePlugins,
    pipelinesRefreshing,
    isInitialLoading,
    pipelineListItems,
    pipelineMap,
    selectedPipelineId,
    pipelineSearch,
    openPluginProjectModal,
    openPluginInIde,
    openCreateModal,
    setSelectedPipeline,
    openPipelineIconModal,
    handlePipelineCardKeydown
  } = ctx;

  const handleOpenDeleteModal = (id: string) => {
    ctx.openDeleteModal?.(id);
  };
</script>

<PipelineListPanel
  activeTab={$activeTab}
  bind:customNodeSearch={ideBindings.customNodeSearch}
  visiblePlugins={visiblePlugins}
  pipelinesRefreshing={pipelinesRefreshing}
  isInitialLoading={isInitialLoading}
  pipelineListItems={$pipelineListItems}
  pipelineMap={$pipelineMap}
  selectedPipelineId={$selectedPipelineId}
  pipelineSearch={pipelineSearch}
  onOpenPluginProject={openPluginProjectModal}
  onOpenPluginInIde={openPluginInIde}
  onOpenCreateModal={openCreateModal}
  onSelectPipeline={setSelectedPipeline}
  onOpenPipelineIcon={openPipelineIconModal}
  onOpenDeleteModal={handleOpenDeleteModal}
  onPipelineCardKeydown={handlePipelineCardKeydown}
/>
