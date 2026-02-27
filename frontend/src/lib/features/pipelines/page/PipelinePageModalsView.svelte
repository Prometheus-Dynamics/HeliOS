<script lang="ts">
  const { ctx } = $props<{ ctx: any }>();

  let {
    PipelineModals,
    assignBusy,
    assignError,
    assignModalOpen,
    addNodeFromRegistry,
    attachPipelineToDevice,
    captureDevices,
    closeAssignModal,
    closeCreateModal,
    closeDeleteModal,
    closePipelineIconModal,
    closePluginProjectModal,
    confirmDeletePipeline,
    createPluginProject,
    createBusy,
    createError,
    createModalOpen,
    createMode,
    createName,
    createPipeline,
    createSourcePipelineId,
    createSourceTemplateId,
    deleteModalBusy,
    deleteModalError,
    deleteModalOpen,
    deleteModalPipeline,
    describePortType,
    formatPipelineValue,
    handlePipelineImport,
    iconModalColor,
    iconModalError,
    iconModalIconId,
    iconModalOpen,
    iconModalPipelineId,
    iconModalSaving,
    pipelineForSource,
    pipelineInputEntries,
    pipelineLabelById,
    pipelineOutputEntries,
    pipelines,
    pluginProjectBusy,
    pluginProjectError,
    pluginProjectLanguage,
    pluginProjectModalOpen,
    pluginProjectName,
    refreshRegistry,
    registryDrawerOpen,
    registryHelpers,
    registrySort,
    registryStores,
    registryView,
    resetRegistryFilters,
    savePipelineIconSelection,
    selectRegistryGroup,
    selectedCaptureSessionId,
    selectedPipeline,
    setSelectedCaptureSession,
    templateForSource,
    templates,
    triggerPipelineImport,
    updateRegistryFilters
  } = ctx;
  const { ideBindings, modalBindings } = ctx;
</script>

<PipelineModals
  registryDrawerOpen={$registryDrawerOpen}
  registryStores={registryStores}
  registryHelpers={registryHelpers}
  onCloseRegistry={() => registryDrawerOpen.set(false)}
  onRefreshRegistry={refreshRegistry}
  onResetRegistry={resetRegistryFilters}
  onSearchRegistry={(term) => updateRegistryFilters({ search: term })}
  onSelectRegistryTag={(tag) => updateRegistryFilters({ tag })}
  onSelectRegistryCategory={(category) => updateRegistryFilters({ category })}
  onSelectRegistryProvider={(provider) => updateRegistryFilters({ provider })}
  onSelectRegistryGroup={(group) => selectRegistryGroup(group ?? 'all')}
  onChangeRegistrySort={(sort) => registrySort.set(sort)}
  onChangeRegistryView={(view) => registryView.set(view)}
  onAddRegistryEntry={(entryId) => addNodeFromRegistry(entryId)}
  iconModalOpen={iconModalOpen && Boolean(iconModalPipelineId)}
  iconModalPipelineLabel={iconModalPipelineId ? pipelineLabelById(iconModalPipelineId) : ''}
  bind:iconId={modalBindings.iconModalIconId}
  bind:color={modalBindings.iconModalColor}
  iconModalError={iconModalError}
  iconModalSaving={iconModalSaving}
  onCloseIconModal={closePipelineIconModal}
  onSaveIcon={savePipelineIconSelection}
  pluginProjectModalOpen={pluginProjectModalOpen}
  bind:projectName={ideBindings.pluginProjectName}
  bind:projectLanguage={ideBindings.pluginProjectLanguage}
  pluginProjectError={pluginProjectError}
  pluginProjectBusy={pluginProjectBusy}
  onClosePluginProject={closePluginProjectModal}
  onCreatePluginProject={createPluginProject}
  createModalOpen={$createModalOpen}
  createMode={createMode}
  createName={createName}
  createSourcePipelineId={createSourcePipelineId}
  createSourceTemplateId={createSourceTemplateId}
  createBusy={$createBusy}
  createError={$createError}
  pipelines={$pipelines}
  templates={$templates}
  pipelineForSource={pipelineForSource}
  templateForSource={templateForSource}
  onCloseCreate={closeCreateModal}
  onCreate={createPipeline}
  onTriggerImport={triggerPipelineImport}
  onFileChange={handlePipelineImport}
  bind:importInput={modalBindings.importInput}
  deleteModalOpen={$deleteModalOpen}
  deleteModalPipeline={$deleteModalPipeline}
  deleteModalBusy={$deleteModalBusy}
  deleteModalError={$deleteModalError}
  onCloseDelete={closeDeleteModal}
  onConfirmDelete={confirmDeletePipeline}
  assignModalOpen={$assignModalOpen}
  selectedPipeline={$selectedPipeline}
  assignError={$assignError}
  assignBusy={$assignBusy}
  captureDevices={$captureDevices}
  selectedCaptureSessionId={$selectedCaptureSessionId}
  pipelineInputEntries={$pipelineInputEntries}
  pipelineOutputEntries={$pipelineOutputEntries}
  onSelectCaptureSession={setSelectedCaptureSession}
  onCloseAssign={closeAssignModal}
  onAttachPipeline={attachPipelineToDevice}
  describePortType={describePortType}
  formatPipelineValue={formatPipelineValue}
/>
