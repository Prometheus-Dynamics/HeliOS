<script lang="ts">
  import type { PipelinePageModalsViewContext } from './pipelinePageTypes';

  const { base } = $props<{ base: PipelinePageModalsViewContext }>();
  const registryDrawerOpen = $derived.by(() => base.registryDrawerOpen);
  const registryStores = $derived.by(() => base.registryStores);
  const refreshRegistry = $derived.by(() => base.refreshRegistry);
  const resetRegistryFilters = $derived.by(() => base.resetRegistryFilters);
  const updateRegistryFilters = $derived.by(() => base.updateRegistryFilters);
  const selectRegistryGroup = $derived.by(() => base.selectRegistryGroup);
  const registrySort = $derived.by(() => base.registrySort);
  const registryView = $derived.by(() => base.registryView);
  const addNodeFromRegistry = $derived.by(() => base.addNodeFromRegistry);
  const PipelineModalsComponent = $derived.by(() => base.PipelineModals);
  const iconModalOpen = $derived.by(() => base.iconModalOpen);
  const iconModalPipelineId = $derived.by(() => base.iconModalPipelineId);
  const pipelineLabelById = $derived.by(() => base.pipelineLabelById);
  const modalBindings = $derived.by(() => base.modalBindings);
  const iconModalError = $derived.by(() => base.iconModalError);
  const iconModalSaving = $derived.by(() => base.iconModalSaving);
  const closePipelineIconModal = $derived.by(() => base.closePipelineIconModal);
  const savePipelineIconSelection = $derived.by(() => base.savePipelineIconSelection);
  const pluginProjectModalOpen = $derived.by(() => base.pluginProjectModalOpen);
  const ideBindings = $derived.by(() => base.ideBindings);
  const pluginProjectError = $derived.by(() => base.pluginProjectError);
  const pluginProjectBusy = $derived.by(() => base.pluginProjectBusy);
  const closePluginProjectModal = $derived.by(() => base.closePluginProjectModal);
  const createPluginProject = $derived.by(() => base.createPluginProject);
  const createModalOpen = $derived.by(() => base.createModalOpen);
  const createMode = $derived.by(() => base.createMode);
  const createName = $derived.by(() => base.createName);
  const createSourcePipelineId = $derived.by(() => base.createSourcePipelineId);
  const createSourceTemplateId = $derived.by(() => base.createSourceTemplateId);
  const createBusy = $derived.by(() => base.createBusy);
  const createError = $derived.by(() => base.createError);
  const pipelines = $derived.by(() => base.pipelines);
  const templates = $derived.by(() => base.templates);
  const pipelineForSource = $derived.by(() => base.pipelineForSource);
  const templateForSource = $derived.by(() => base.templateForSource);
  const closeCreateModal = $derived.by(() => base.closeCreateModal);
  const createPipeline = $derived.by(() => base.createPipeline);
  const triggerPipelineImport = $derived.by(() => base.triggerPipelineImport);
  const handlePipelineImport = $derived.by(() => base.handlePipelineImport);
  const deleteModalOpen = $derived.by(() => base.deleteModalOpen);
  const deleteModalPipeline = $derived.by(() => base.deleteModalPipeline);
  const deleteModalBusy = $derived.by(() => base.deleteModalBusy);
  const deleteModalError = $derived.by(() => base.deleteModalError);
  const closeDeleteModal = $derived.by(() => base.closeDeleteModal);
  const confirmDeletePipeline = $derived.by(() => base.confirmDeletePipeline);
  const assignModalOpen = $derived.by(() => base.assignModalOpen);
  const selectedPipeline = $derived.by(() => base.selectedPipeline);
  const assignError = $derived.by(() => base.assignError);
  const assignBusy = $derived.by(() => base.assignBusy);
  const captureDevices = $derived.by(() => base.captureDevices);
  const selectedCaptureSessionId = $derived.by(() => base.selectedCaptureSessionId);
  const setSelectedCaptureSession = $derived.by(() => base.setSelectedCaptureSession);
  const closeAssignModal = $derived.by(() => base.closeAssignModal);
  const attachPipelineToDevice = $derived.by(() => base.attachPipelineToDevice);
  const shouldShowLoadingShell = $derived.by(
    () =>
      $registryDrawerOpen ||
      (iconModalOpen && Boolean(iconModalPipelineId)) ||
      pluginProjectModalOpen ||
      $createModalOpen ||
      $deleteModalOpen ||
      $assignModalOpen
  );
</script>

{#if PipelineModalsComponent}
  {@const Modals = PipelineModalsComponent}
  <Modals
    registryDrawerOpen={$registryDrawerOpen}
    registryStores={registryStores}
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
    onSelectCaptureSession={setSelectedCaptureSession}
    onCloseAssign={closeAssignModal}
    onAttachPipeline={attachPipelineToDevice}
  />
{:else if shouldShowLoadingShell}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/45 backdrop-blur-sm">
    <div class="rounded border border-surface-700/70 bg-surface-950/90 px-4 py-3 text-xs uppercase tracking-[0.24em] text-surface-300 shadow-2xl shadow-black/40">
      Loading pipeline tools…
    </div>
  </div>
{/if}
