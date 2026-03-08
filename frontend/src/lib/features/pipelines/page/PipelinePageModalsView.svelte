<script lang="ts">
  import type { ComponentProps } from 'svelte';
  import type { Readable, Writable } from 'svelte/store';
  import PipelineModals from '$lib/features/pipelines/page/PipelineModals.svelte';

  type PipelineModalsProps = ComponentProps<typeof PipelineModals>;
  type RegistryFilterUpdate = {
    search?: string;
    tag?: string | null;
    category?: string | null;
    provider?: string | null;
  };
  type PipelinePageModalsViewCtx = {
    PipelineModals: typeof PipelineModals;
    assignBusy: Readable<boolean>;
    assignError: Readable<PipelineModalsProps['assignError']>;
    assignModalOpen: Readable<boolean>;
    addNodeFromRegistry: NonNullable<PipelineModalsProps['onAddRegistryEntry']>;
    attachPipelineToDevice: NonNullable<PipelineModalsProps['onAttachPipeline']>;
    captureDevices: Readable<PipelineModalsProps['captureDevices']>;
    closeAssignModal: NonNullable<PipelineModalsProps['onCloseAssign']>;
    closeCreateModal: NonNullable<PipelineModalsProps['onCloseCreate']>;
    closeDeleteModal: NonNullable<PipelineModalsProps['onCloseDelete']>;
    closePipelineIconModal: NonNullable<PipelineModalsProps['onCloseIconModal']>;
    closePluginProjectModal: NonNullable<PipelineModalsProps['onClosePluginProject']>;
    confirmDeletePipeline: NonNullable<PipelineModalsProps['onConfirmDelete']>;
    createPluginProject: NonNullable<PipelineModalsProps['onCreatePluginProject']>;
    createBusy: Readable<boolean>;
    createError: Readable<PipelineModalsProps['createError']>;
    createModalOpen: Readable<boolean>;
    createMode: PipelineModalsProps['createMode'];
    createName: PipelineModalsProps['createName'];
    createPipeline: NonNullable<PipelineModalsProps['onCreate']>;
    createSourcePipelineId: PipelineModalsProps['createSourcePipelineId'];
    createSourceTemplateId: PipelineModalsProps['createSourceTemplateId'];
    deleteModalBusy: Readable<boolean>;
    deleteModalError: Readable<PipelineModalsProps['deleteModalError']>;
    deleteModalOpen: Readable<boolean>;
    deleteModalPipeline: Readable<PipelineModalsProps['deleteModalPipeline']>;
    handlePipelineImport: NonNullable<PipelineModalsProps['onFileChange']>;
    iconModalError: PipelineModalsProps['iconModalError'];
    iconModalOpen: boolean;
    iconModalPipelineId: string | null;
    iconModalSaving: boolean;
    ideBindings: {
      pluginProjectLanguage: string;
      pluginProjectName: string;
    };
    modalBindings: {
      iconModalColor: string;
      iconModalIconId: string;
      importInput: HTMLInputElement | null;
    };
    pipelineForSource: PipelineModalsProps['pipelineForSource'];
    pipelineLabelById: (pipelineId: string) => string;
    pipelines: Readable<PipelineModalsProps['pipelines']>;
    pluginProjectBusy: boolean;
    pluginProjectError: PipelineModalsProps['pluginProjectError'];
    pluginProjectModalOpen: boolean;
    refreshRegistry: NonNullable<PipelineModalsProps['onRefreshRegistry']>;
    registryDrawerOpen: Writable<boolean>;
    registrySort: Writable<NonNullable<Parameters<NonNullable<PipelineModalsProps['onChangeRegistrySort']>>[0]>>;
    registryStores: PipelineModalsProps['registryStores'];
    registryView: Writable<NonNullable<Parameters<NonNullable<PipelineModalsProps['onChangeRegistryView']>>[0]>>;
    resetRegistryFilters: NonNullable<PipelineModalsProps['onResetRegistry']>;
    savePipelineIconSelection: NonNullable<PipelineModalsProps['onSaveIcon']>;
    selectRegistryGroup: (group: string) => void;
    selectedCaptureSessionId: Readable<PipelineModalsProps['selectedCaptureSessionId']>;
    selectedPipeline: Readable<PipelineModalsProps['selectedPipeline']>;
    setSelectedCaptureSession: NonNullable<PipelineModalsProps['onSelectCaptureSession']>;
    templateForSource: PipelineModalsProps['templateForSource'];
    templates: Readable<PipelineModalsProps['templates']>;
    triggerPipelineImport: NonNullable<PipelineModalsProps['onTriggerImport']>;
    updateRegistryFilters: (update: RegistryFilterUpdate) => void;
  };

  const { ctx } = $props<{ ctx: Record<string, unknown> }>();
  const getPageCtx = (): PipelinePageModalsViewCtx => ctx as PipelinePageModalsViewCtx;
  const registryDrawerOpen = $derived.by(() => getPageCtx().registryDrawerOpen);
  const registryStores = $derived.by(() => getPageCtx().registryStores);
  const refreshRegistry = $derived.by(() => getPageCtx().refreshRegistry);
  const resetRegistryFilters = $derived.by(() => getPageCtx().resetRegistryFilters);
  const updateRegistryFilters = $derived.by(() => getPageCtx().updateRegistryFilters);
  const selectRegistryGroup = $derived.by(() => getPageCtx().selectRegistryGroup);
  const registrySort = $derived.by(() => getPageCtx().registrySort);
  const registryView = $derived.by(() => getPageCtx().registryView);
  const addNodeFromRegistry = $derived.by(() => getPageCtx().addNodeFromRegistry);
  const iconModalOpen = $derived.by(() => getPageCtx().iconModalOpen);
  const iconModalPipelineId = $derived.by(() => getPageCtx().iconModalPipelineId);
  const pipelineLabelById = $derived.by(() => getPageCtx().pipelineLabelById);
  const modalBindings = $derived.by(() => getPageCtx().modalBindings);
  const iconModalError = $derived.by(() => getPageCtx().iconModalError);
  const iconModalSaving = $derived.by(() => getPageCtx().iconModalSaving);
  const closePipelineIconModal = $derived.by(() => getPageCtx().closePipelineIconModal);
  const savePipelineIconSelection = $derived.by(() => getPageCtx().savePipelineIconSelection);
  const pluginProjectModalOpen = $derived.by(() => getPageCtx().pluginProjectModalOpen);
  const ideBindings = $derived.by(() => getPageCtx().ideBindings);
  const pluginProjectError = $derived.by(() => getPageCtx().pluginProjectError);
  const pluginProjectBusy = $derived.by(() => getPageCtx().pluginProjectBusy);
  const closePluginProjectModal = $derived.by(() => getPageCtx().closePluginProjectModal);
  const createPluginProject = $derived.by(() => getPageCtx().createPluginProject);
  const createModalOpen = $derived.by(() => getPageCtx().createModalOpen);
  const createMode = $derived.by(() => getPageCtx().createMode);
  const createName = $derived.by(() => getPageCtx().createName);
  const createSourcePipelineId = $derived.by(() => getPageCtx().createSourcePipelineId);
  const createSourceTemplateId = $derived.by(() => getPageCtx().createSourceTemplateId);
  const createBusy = $derived.by(() => getPageCtx().createBusy);
  const createError = $derived.by(() => getPageCtx().createError);
  const pipelines = $derived.by(() => getPageCtx().pipelines);
  const templates = $derived.by(() => getPageCtx().templates);
  const pipelineForSource = $derived.by(() => getPageCtx().pipelineForSource);
  const templateForSource = $derived.by(() => getPageCtx().templateForSource);
  const closeCreateModal = $derived.by(() => getPageCtx().closeCreateModal);
  const createPipeline = $derived.by(() => getPageCtx().createPipeline);
  const triggerPipelineImport = $derived.by(() => getPageCtx().triggerPipelineImport);
  const handlePipelineImport = $derived.by(() => getPageCtx().handlePipelineImport);
  const deleteModalOpen = $derived.by(() => getPageCtx().deleteModalOpen);
  const deleteModalPipeline = $derived.by(() => getPageCtx().deleteModalPipeline);
  const deleteModalBusy = $derived.by(() => getPageCtx().deleteModalBusy);
  const deleteModalError = $derived.by(() => getPageCtx().deleteModalError);
  const closeDeleteModal = $derived.by(() => getPageCtx().closeDeleteModal);
  const confirmDeletePipeline = $derived.by(() => getPageCtx().confirmDeletePipeline);
  const assignModalOpen = $derived.by(() => getPageCtx().assignModalOpen);
  const selectedPipeline = $derived.by(() => getPageCtx().selectedPipeline);
  const assignError = $derived.by(() => getPageCtx().assignError);
  const assignBusy = $derived.by(() => getPageCtx().assignBusy);
  const captureDevices = $derived.by(() => getPageCtx().captureDevices);
  const selectedCaptureSessionId = $derived.by(() => getPageCtx().selectedCaptureSessionId);
  const setSelectedCaptureSession = $derived.by(() => getPageCtx().setSelectedCaptureSession);
  const closeAssignModal = $derived.by(() => getPageCtx().closeAssignModal);
  const attachPipelineToDevice = $derived.by(() => getPageCtx().attachPipelineToDevice);
</script>

<PipelineModals
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
