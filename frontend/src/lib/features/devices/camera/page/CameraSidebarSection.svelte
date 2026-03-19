<script lang="ts">
  import type { ComponentProps } from 'svelte';
  import type CameraCalibrationTab from '$lib/features/devices/camera/CameraCalibrationTab.svelte';
  import type CameraControlsTab from '$lib/features/devices/camera/CameraControlsTab.svelte';
  import type CameraMediaTab from '$lib/features/devices/camera/CameraMediaTab.svelte';
  import type CameraPipelinesTab from '$lib/features/devices/camera/CameraPipelinesTab.svelte';
  import type CameraPoseTab from '$lib/features/devices/camera/CameraPoseTab.svelte';
  import type CameraStreamTab from '$lib/features/devices/camera/CameraStreamTab.svelte';
  import type {
    StreamInfo,
    StreamPipelineEndpoint,
    StreamPipelineWire
  } from '$lib/ts-bindings/http/client';
  import CameraStreamSidebar from './CameraStreamSidebar.svelte';

  type StreamSidebarProps = ComponentProps<typeof CameraStreamSidebar>;
  type CameraSidebarTab = StreamSidebarProps['activeTab'];
  type StreamTabProps = ComponentProps<typeof CameraStreamTab>;
  type ControlsTabProps = ComponentProps<typeof CameraControlsTab>;
  type PipelinesTabProps = ComponentProps<typeof CameraPipelinesTab>;
  type CalibrationTabProps = ComponentProps<typeof CameraCalibrationTab>;
  type PoseTabProps = ComponentProps<typeof CameraPoseTab>;
  type MediaTabProps = ComponentProps<typeof CameraMediaTab>;
  type PipelineDurationFn = NonNullable<PipelinesTabProps['outputDurationForCell']>;

  type StreamBindings = {
    activeTab: StreamSidebarProps['activeTab'];
  } & Pick<
    StreamTabProps,
    | 'cameraAlias'
    | 'selectedBackendIndex'
    | 'selectedFormat'
    | 'selectedResolution'
    | 'selectedIntervalIdx'
    | 'selectedInterval'
    | 'libcameraTargetFps'
    | 'netcamTargetFps'
    | 'fileBackendFps'
    | 'fileBackendLoop'
    | 'fileBackendPathsText'
    | 'decoderImpl'
    | 'encoderImpl'
    | 'decoderEnabled'
    | 'encoderEnabled'
    | 'decoderSelectionTouched'
    | 'encoderSelectionTouched'
    | 'hostBuffer'
    | 'decoderFpsLimit'
    | 'decoderRotationDegrees'
    | 'decoderMirrorHorizontal'
    | 'shadowRecorderEnabled'
    | 'encoderFpsLimit'
    | 'encoderSettingsOpen'
    | 'encoderSettings'
    | 'streamCrop'
    | 'streamCropGuidesEnabled'
    | 'streamCropApplying'
    | 'streamCropError'
    | 'streamCropWarning'
    | 'streamCrosshair'
    | 'streamCrosshairEnabled'
    | 'streamCrosshairGuidesEnabled'
    | 'streamCrosshairApplying'
    | 'streamCrosshairError'
    | 'streamCrosshairWarning'
    | 'streamOrderingMode'
    | 'streamOrderingApplying'
    | 'streamOrderingError'
    | 'streamOrderingWarning'
  > &
    Pick<ControlsTabProps, 'controlsQuery' | 'controlState' | 'controlAppliedState' | 'controlBusy'>;

  type PipelineBindings = Pick<
    PipelinesTabProps,
    | 'pipelineGridRows'
    | 'pipelineGridColumns'
    | 'selectedPipelineOutput'
    | 'pipelineRemoveModalOpen'
    | 'pipelineRemoveCandidateId'
    | 'pipelineAssignModalOpen'
    | 'pipelineAssignQuery'
    | 'pipelineAssignDraft'
  >;

  type CalibrationState = {
    calibrationGuidedMode: CalibrationTabProps['guidedModeEnabled'];
    calibrationGuidedBusy: CalibrationTabProps['guidedModeBusy'];
    calibrationGuidedAccumulateLive: CalibrationTabProps['guidedAccumulateLive'];
    calibrationBoard: CalibrationTabProps['calibrationBoard'];
    calibrationLensModel: CalibrationTabProps['calibrationLensModel'];
    calibrationImages: CalibrationTabProps['calibrationImages'];
    calibrationOwnPhotosOnly: CalibrationTabProps['calibrationOwnPhotosOnly'];
    calibrationSelected: CalibrationTabProps['calibrationSelected'];
    calibrationLoading: CalibrationTabProps['calibrationLoading'];
    calibrationSolving: CalibrationTabProps['calibrationSolving'];
    calibrationApplying: CalibrationTabProps['calibrationApplying'];
    calibrationDeleting: CalibrationTabProps['calibrationDeleting'];
    calibrationIncludeOverlays: CalibrationTabProps['calibrationIncludeOverlays'];
    calibrationSolveError: CalibrationTabProps['calibrationSolveError'];
    calibrationResult: CalibrationTabProps['calibrationResult'];
    calibrationImportSourcesLoading: CalibrationTabProps['calibrationImportSourcesLoading'];
    calibrationImporting: CalibrationTabProps['calibrationImporting'];
    calibrationImportError: CalibrationTabProps['calibrationImportError'];
    calibrationImportSourceId: CalibrationTabProps['calibrationImportSourceId'];
    calibrationImportSources: CalibrationTabProps['calibrationImportSources'];
    calibrationPreviewOpen: CalibrationTabProps['calibrationPreviewOpen'];
    calibrationPreviewItem: CalibrationTabProps['calibrationPreviewItem'];
    ipaLoading: CalibrationTabProps['ipaLoading'];
    ipaStatus: CalibrationTabProps['ipaStatus'];
    ipaCt: CalibrationTabProps['ipaCt'];
    ipaCcm: CalibrationTabProps['ipaCcm'];
    ipaAdvanced: CalibrationTabProps['ipaAdvanced'];
    ipaTarget: CalibrationTabProps['ipaTarget'];
    ipaChartImage: CalibrationTabProps['ipaChartImage'];
    ipaChartModalOpen: CalibrationTabProps['ipaChartModalOpen'];
    ipaChartCorners: CalibrationTabProps['ipaChartCorners'];
    ipaChartNaturalSize: CalibrationTabProps['ipaChartNaturalSize'];
  };

  type LegacyPipelineWireManifest = StreamInfo['manifest'] & {
    pipelineWires?: unknown;
  };

  type CameraSidebarSectionCtx = {
    tabs: StreamSidebarProps['tabs'];
    streamBindings: StreamBindings;
    pipelineBindings: PipelineBindings;
    stream: StreamInfo | null;
    streamId: MediaTabProps['streamId'];
    pipelineState?: { assignedPipelineIds?: PipelinesTabProps['assignedPipelineIds'] } | null;
    activeMode: { format?: { resolution?: CalibrationTabProps['sourceResolution'] } | null } | null;
    calibrationState: CalibrationState;
    outputDurationForPipeline: (
      pipelineId: Parameters<PipelineDurationFn>[2],
      outputKey: Parameters<PipelineDurationFn>[3]
    ) => ReturnType<PipelineDurationFn>;
    resolvePoseCameraRef: (stream: StreamInfo | null | undefined, streamId: string) => PoseTabProps['cameraRef'];
    currentCalibrationParams: CalibrationTabProps['currentCalibrationParams'];
    setGuidedCalibrationMode: (enabled: boolean) => void | Promise<void>;
    resetGuidedCalibrationCoverage: CalibrationTabProps['resetGuidedCoverage'];
    setCalibrationOwnPhotosOnly: CalibrationTabProps['setCalibrationOwnPhotosOnly'];
    refreshCalibrationImages: CalibrationTabProps['refreshCalibrationImages'];
    refreshCalibrationImportSources: CalibrationTabProps['refreshCalibrationImportSources'];
    takeCalibrationSnapshot: CalibrationTabProps['takeCalibrationSnapshot'];
    deleteCalibrationSnapshot: CalibrationTabProps['deleteCalibrationSnapshot'];
    deleteCalibrationOverlays: CalibrationTabProps['deleteCalibrationOverlays'];
    solveCalibration: CalibrationTabProps['solveCalibration'];
    saveSolvedCalibration: CalibrationTabProps['saveSolvedCalibration'];
    copyCalibrationFromSelectedStream: CalibrationTabProps['copyCalibrationFromSelectedStream'];
    importCalibrationFromJsonFile: CalibrationTabProps['importCalibrationFromJsonFile'];
    openCalibrationPreview: CalibrationTabProps['openCalibrationPreview'];
    closeCalibrationPreview: CalibrationTabProps['closeCalibrationPreview'];
    refreshIpaStatus: CalibrationTabProps['refreshIpaStatus'];
    applyIpaCcm: CalibrationTabProps['applyIpaCcm'];
    openIpaChartSolverForImage: CalibrationTabProps['openIpaChartSolverForImage'];
    closeIpaChartSolver: CalibrationTabProps['closeIpaChartSolver'];
    addIpaChartCorner: CalibrationTabProps['addIpaChartCorner'];
    setFrameSourceForPipelineInstance: PipelinesTabProps['setFrameSourceForPipelineInstance'];
    solveIpaChartCcm: CalibrationTabProps['solveIpaChartCcm'];
    ipaChartNaturalSize: CalibrationTabProps['ipaChartNaturalSize'];
    ipaChartSolveBusy: CalibrationTabProps['ipaChartSolveBusy'];
    ipaChartSolveError: CalibrationTabProps['ipaChartSolveError'];
    ipaChartSolveResult: CalibrationTabProps['ipaChartSolveResult'];
  } & Pick<
    StreamTabProps,
    | 'applying'
    | 'encoders'
    | 'encoderSettingsAvailable'
    | 'currentDevice'
    | 'currentBackend'
    | 'backendLabel'
    | 'uniqueFormats'
    | 'resolutionsForFormat'
    | 'intervalsForSelection'
    | 'firstFormat'
    | 'firstResolution'
    | 'syncModeSelection'
    | 'fpsLabel'
    | 'intervalToFps'
    | 'decodersForCaptureFormat'
    | 'applyStreamPreset'
    | 'applyStreamCrop'
    | 'applyStreamCrosshair'
    | 'applyStreamOrdering'
    | 'effectiveModes'
    | 'resolutionKey'
  > &
    Pick<
      ControlsTabProps,
      | 'controls'
      | 'filteredControls'
      | 'menuOptions'
      | 'applyControl'
      | 'scheduleControlApply'
      | 'displayValue'
      | 'extractValue'
      | 'controlMin'
      | 'controlMax'
      | 'controlStep'
      | 'accessLabel'
      | 'accessBadgeClass'
    > &
    Pick<
      PipelinesTabProps,
      | 'pipelineGraphError'
      | 'openPipelineAssignModal'
      | 'pipelineGraphLoading'
      | 'assignedPipelineIds'
      | 'handlePipelineDragStart'
      | 'RAW_PIPELINE_ID'
      | 'RAW_PIPELINE_UUID'
      | 'pipelineLabel'
      | 'openPipelineTuningPanel'
      | 'openPipelineRemoveModal'
      | 'pipelineGridIsSingle'
      | 'setPipelineGridDimensions'
      | 'pipelineGridRowIndices'
      | 'pipelineGridColumnIndices'
      | 'pipelineForCell'
      | 'outputSelectionForPipeline'
      | 'outputKeyForCell'
      | 'pipelineOutputOptionsCache'
      | 'ensurePipelineOutputsLoaded'
      | 'ensurePipelineGraphAndOutputs'
      | 'schedulePipelineLayoutApply'
      | 'allowDrop'
      | 'dropOnCell'
      | 'clearCell'
      | 'refreshPipelineGraphs'
      | 'setOutputSelectionForPipeline'
      | 'setOutputKeyForCell'
      | 'setLivePipelineOutput'
      | 'pipelineAssignFilteredGraphs'
      | 'pipelineGraphs'
      | 'closePipelineRemoveModal'
      | 'confirmPipelineRemove'
      | 'closePipelineAssignModal'
      | 'savePipelineAssignModal'
      | 'listPipelineTemplatesForAssign'
      | 'createPipelineFromTemplateAndAssign'
    > &
    Pick<CalibrationTabProps, 'apiPath'>;

  const asRecord = (value: unknown): Record<string, unknown> | null =>
    value && typeof value === 'object' ? (value as Record<string, unknown>) : null;

  const isPipelineEndpoint = (value: unknown): value is StreamPipelineEndpoint => {
    const record = asRecord(value);
    return typeof record?.pipeline_id === 'string';
  };

  const isPipelineWire = (value: unknown): value is StreamPipelineWire => {
    const record = asRecord(value);
    return isPipelineEndpoint(record?.from) && isPipelineEndpoint(record?.to);
  };

  let { ctx = $bindable() }: { ctx: CameraSidebarSectionCtx } = $props();

  let CameraStreamTabComponent = $state<(typeof import('$lib/features/devices/camera/CameraStreamTab.svelte'))['default'] | null>(null);
  let CameraMediaTabComponent = $state<(typeof import('$lib/features/devices/camera/CameraMediaTab.svelte'))['default'] | null>(null);
  let CameraControlsTabComponent = $state<(typeof import('$lib/features/devices/camera/CameraControlsTab.svelte'))['default'] | null>(null);
  let CameraPipelinesTabComponent = $state<(typeof import('$lib/features/devices/camera/CameraPipelinesTab.svelte'))['default'] | null>(null);
  let CameraPoseTabComponent = $state<(typeof import('$lib/features/devices/camera/CameraPoseTab.svelte'))['default'] | null>(null);
  let CameraCalibrationTabComponent = $state<(typeof import('$lib/features/devices/camera/CameraCalibrationTab.svelte'))['default'] | null>(null);

  const tabLoaders: Partial<Record<CameraSidebarTab, Promise<void>>> = {};

  function loadTabOnce(tab: CameraSidebarTab, loader: () => Promise<void>): Promise<void> {
    const inFlight = tabLoaders[tab];
    if (inFlight) {
      return inFlight;
    }
    const next = loader().finally(() => {
      tabLoaders[tab] = undefined;
    });
    tabLoaders[tab] = next;
    return next;
  }

  async function ensureCameraTabLoaded(tab: CameraSidebarTab): Promise<void> {
    switch (tab) {
      case 'stream':
        if (CameraStreamTabComponent) return;
        await loadTabOnce(tab, async () => {
          const module = await import('$lib/features/devices/camera/CameraStreamTab.svelte');
          CameraStreamTabComponent = module.default;
        });
        return;
      case 'media':
        if (CameraMediaTabComponent) return;
        await loadTabOnce(tab, async () => {
          const module = await import('$lib/features/devices/camera/CameraMediaTab.svelte');
          CameraMediaTabComponent = module.default;
        });
        return;
      case 'controls':
        if (CameraControlsTabComponent) return;
        await loadTabOnce(tab, async () => {
          const module = await import('$lib/features/devices/camera/CameraControlsTab.svelte');
          CameraControlsTabComponent = module.default;
        });
        return;
      case 'pipelines':
        if (CameraPipelinesTabComponent) return;
        await loadTabOnce(tab, async () => {
          const module = await import('$lib/features/devices/camera/CameraPipelinesTab.svelte');
          CameraPipelinesTabComponent = module.default;
        });
        return;
      case 'pose':
        if (CameraPoseTabComponent) return;
        await loadTabOnce(tab, async () => {
          const module = await import('$lib/features/devices/camera/CameraPoseTab.svelte');
          CameraPoseTabComponent = module.default;
        });
        return;
      case 'calibration':
        if (CameraCalibrationTabComponent) return;
        await loadTabOnce(tab, async () => {
          const module = await import('$lib/features/devices/camera/CameraCalibrationTab.svelte');
          CameraCalibrationTabComponent = module.default;
        });
        return;
    }
  }

  $effect(() => {
    void ensureCameraTabLoaded(ctx.streamBindings.activeTab);
  });

  const pipelineWires = $derived.by<StreamPipelineWire[]>(() => {
    const manifest = (ctx.stream?.manifest as LegacyPipelineWireManifest | null) ?? null;
    if (Array.isArray(manifest?.pipeline_wires)) {
      return manifest.pipeline_wires;
    }
    return Array.isArray(manifest?.pipelineWires) ? manifest.pipelineWires.filter(isPipelineWire) : [];
  });

  const activeTabLabel = $derived.by(
    () => ctx.tabs.find((tab) => tab.id === ctx.streamBindings.activeTab)?.label ?? ctx.streamBindings.activeTab
  );
</script>

<CameraStreamSidebar tabs={ctx.tabs} bind:activeTab={ctx.streamBindings.activeTab}>
  {#snippet content()}
    {#if ctx.streamBindings.activeTab === 'stream'}
      {#if CameraStreamTabComponent}
        {@const CameraStreamTab = CameraStreamTabComponent}
        <CameraStreamTab
          bind:cameraAlias={ctx.streamBindings.cameraAlias}
          bind:selectedBackendIndex={ctx.streamBindings.selectedBackendIndex}
          bind:selectedFormat={ctx.streamBindings.selectedFormat}
          bind:selectedResolution={ctx.streamBindings.selectedResolution}
          bind:selectedIntervalIdx={ctx.streamBindings.selectedIntervalIdx}
          bind:selectedInterval={ctx.streamBindings.selectedInterval}
          bind:libcameraTargetFps={ctx.streamBindings.libcameraTargetFps}
          bind:netcamTargetFps={ctx.streamBindings.netcamTargetFps}
          bind:fileBackendFps={ctx.streamBindings.fileBackendFps}
          bind:fileBackendLoop={ctx.streamBindings.fileBackendLoop}
          bind:fileBackendPathsText={ctx.streamBindings.fileBackendPathsText}
          bind:decoderImpl={ctx.streamBindings.decoderImpl}
          bind:encoderImpl={ctx.streamBindings.encoderImpl}
          bind:decoderEnabled={ctx.streamBindings.decoderEnabled}
          bind:encoderEnabled={ctx.streamBindings.encoderEnabled}
          bind:decoderSelectionTouched={ctx.streamBindings.decoderSelectionTouched}
          bind:encoderSelectionTouched={ctx.streamBindings.encoderSelectionTouched}
          bind:hostBuffer={ctx.streamBindings.hostBuffer}
          bind:decoderFpsLimit={ctx.streamBindings.decoderFpsLimit}
          bind:decoderRotationDegrees={ctx.streamBindings.decoderRotationDegrees}
          bind:decoderMirrorHorizontal={ctx.streamBindings.decoderMirrorHorizontal}
          bind:shadowRecorderEnabled={ctx.streamBindings.shadowRecorderEnabled}
          bind:encoderFpsLimit={ctx.streamBindings.encoderFpsLimit}
          bind:encoderSettingsOpen={ctx.streamBindings.encoderSettingsOpen}
          bind:encoderSettings={ctx.streamBindings.encoderSettings}
          bind:streamCrop={ctx.streamBindings.streamCrop}
          bind:streamCropGuidesEnabled={ctx.streamBindings.streamCropGuidesEnabled}
          bind:streamCropApplying={ctx.streamBindings.streamCropApplying}
          bind:streamCropError={ctx.streamBindings.streamCropError}
          bind:streamCropWarning={ctx.streamBindings.streamCropWarning}
          bind:streamCrosshair={ctx.streamBindings.streamCrosshair}
          bind:streamCrosshairEnabled={ctx.streamBindings.streamCrosshairEnabled}
          bind:streamCrosshairGuidesEnabled={ctx.streamBindings.streamCrosshairGuidesEnabled}
          bind:streamCrosshairApplying={ctx.streamBindings.streamCrosshairApplying}
          bind:streamCrosshairError={ctx.streamBindings.streamCrosshairError}
          bind:streamCrosshairWarning={ctx.streamBindings.streamCrosshairWarning}
          bind:streamOrderingMode={ctx.streamBindings.streamOrderingMode}
          bind:streamOrderingApplying={ctx.streamBindings.streamOrderingApplying}
          bind:streamOrderingError={ctx.streamBindings.streamOrderingError}
          bind:streamOrderingWarning={ctx.streamBindings.streamOrderingWarning}
          applying={ctx.applying}
          encoders={ctx.encoders}
          encoderSettingsAvailable={ctx.encoderSettingsAvailable}
          currentDevice={ctx.currentDevice}
          currentBackend={ctx.currentBackend}
          backendLabel={ctx.backendLabel}
          uniqueFormats={ctx.uniqueFormats}
          resolutionsForFormat={ctx.resolutionsForFormat}
          intervalsForSelection={ctx.intervalsForSelection}
          firstFormat={ctx.firstFormat}
          firstResolution={ctx.firstResolution}
          syncModeSelection={ctx.syncModeSelection}
          fpsLabel={ctx.fpsLabel}
          intervalToFps={ctx.intervalToFps}
          decodersForCaptureFormat={ctx.decodersForCaptureFormat}
          applyStreamPreset={ctx.applyStreamPreset}
          applyStreamCrop={ctx.applyStreamCrop}
          applyStreamCrosshair={ctx.applyStreamCrosshair}
          applyStreamOrdering={ctx.applyStreamOrdering}
          effectiveModes={ctx.effectiveModes}
          resolutionKey={ctx.resolutionKey}
        />
      {:else}
        <div class="rounded border border-surface-800/60 bg-surface-950/50 px-3 py-4 text-xs uppercase tracking-[0.24em] text-surface-400">
          Loading {activeTabLabel}…
        </div>
      {/if}
    {:else if ctx.streamBindings.activeTab === 'media'}
      {#if CameraMediaTabComponent}
        {@const CameraMediaTab = CameraMediaTabComponent}
        <CameraMediaTab streamId={ctx.stream?.id ?? ctx.streamId} />
      {:else}
        <div class="rounded border border-surface-800/60 bg-surface-950/50 px-3 py-4 text-xs uppercase tracking-[0.24em] text-surface-400">
          Loading {activeTabLabel}…
        </div>
      {/if}
    {:else if ctx.streamBindings.activeTab === 'controls'}
      {#if CameraControlsTabComponent}
        {@const CameraControlsTab = CameraControlsTabComponent}
        <CameraControlsTab
          controls={ctx.controls}
          bind:controlsQuery={ctx.streamBindings.controlsQuery}
          bind:controlState={ctx.streamBindings.controlState}
          bind:controlAppliedState={ctx.streamBindings.controlAppliedState}
          bind:controlBusy={ctx.streamBindings.controlBusy}
          filteredControls={ctx.filteredControls}
          menuOptions={ctx.menuOptions}
          applyControl={ctx.applyControl}
          scheduleControlApply={ctx.scheduleControlApply}
          displayValue={ctx.displayValue}
          extractValue={ctx.extractValue}
          controlMin={ctx.controlMin}
          controlMax={ctx.controlMax}
          controlStep={ctx.controlStep}
          accessLabel={ctx.accessLabel}
          accessBadgeClass={ctx.accessBadgeClass}
        />
      {:else}
        <div class="rounded border border-surface-800/60 bg-surface-950/50 px-3 py-4 text-xs uppercase tracking-[0.24em] text-surface-400">
          Loading {activeTabLabel}…
        </div>
      {/if}
    {:else if ctx.streamBindings.activeTab === 'pipelines'}
      {#if CameraPipelinesTabComponent}
        {@const CameraPipelinesTab = CameraPipelinesTabComponent}
        <CameraPipelinesTab
          pipelineGraphError={ctx.pipelineGraphError}
          openPipelineAssignModal={ctx.openPipelineAssignModal}
          pipelineGraphLoading={ctx.pipelineGraphLoading}
          assignedPipelineIds={ctx.pipelineState?.assignedPipelineIds ?? ctx.assignedPipelineIds ?? []}
          handlePipelineDragStart={ctx.handlePipelineDragStart}
          RAW_PIPELINE_ID={ctx.RAW_PIPELINE_ID}
          RAW_PIPELINE_UUID={ctx.RAW_PIPELINE_UUID}
          pipelineLabel={ctx.pipelineLabel}
          openPipelineTuningPanel={ctx.openPipelineTuningPanel}
          openPipelineRemoveModal={ctx.openPipelineRemoveModal}
          pipelineGridIsSingle={ctx.pipelineGridIsSingle}
          setPipelineGridDimensions={ctx.setPipelineGridDimensions}
          outputsIconEnabled={true}
          streamId={ctx.stream?.id ?? ctx.streamId}
          streamLabel={ctx.streamBindings.cameraAlias}
          bind:pipelineGridRows={ctx.pipelineBindings.pipelineGridRows}
          bind:pipelineGridColumns={ctx.pipelineBindings.pipelineGridColumns}
          pipelineGridRowIndices={ctx.pipelineGridRowIndices}
          pipelineGridColumnIndices={ctx.pipelineGridColumnIndices}
          pipelineForCell={ctx.pipelineForCell}
          outputSelectionForPipeline={ctx.outputSelectionForPipeline}
          outputDurationForCell={(row, column, pipelineId, outputKey) => ctx.outputDurationForPipeline(pipelineId, outputKey)}
          outputKeyForCell={ctx.outputKeyForCell}
          {pipelineWires}
          setFrameSourceForPipelineInstance={ctx.setFrameSourceForPipelineInstance}
          pipelineOutputOptionsCache={ctx.pipelineOutputOptionsCache}
          ensurePipelineOutputsLoaded={ctx.ensurePipelineOutputsLoaded}
          ensurePipelineGraphAndOutputs={ctx.ensurePipelineGraphAndOutputs}
          schedulePipelineLayoutApply={ctx.schedulePipelineLayoutApply}
          allowDrop={ctx.allowDrop}
          dropOnCell={ctx.dropOnCell}
          clearCell={ctx.clearCell}
          refreshPipelineGraphs={ctx.refreshPipelineGraphs}
          bind:selectedPipelineOutput={ctx.pipelineBindings.selectedPipelineOutput}
          setOutputSelectionForPipeline={ctx.setOutputSelectionForPipeline}
          setOutputKeyForCell={ctx.setOutputKeyForCell}
          setLivePipelineOutput={ctx.setLivePipelineOutput}
          bind:pipelineRemoveModalOpen={ctx.pipelineBindings.pipelineRemoveModalOpen}
          bind:pipelineRemoveCandidateId={ctx.pipelineBindings.pipelineRemoveCandidateId}
          closePipelineRemoveModal={ctx.closePipelineRemoveModal}
          confirmPipelineRemove={ctx.confirmPipelineRemove}
          bind:pipelineAssignModalOpen={ctx.pipelineBindings.pipelineAssignModalOpen}
          bind:pipelineAssignQuery={ctx.pipelineBindings.pipelineAssignQuery}
          bind:pipelineAssignDraft={ctx.pipelineBindings.pipelineAssignDraft}
          pipelineAssignFilteredGraphs={ctx.pipelineAssignFilteredGraphs}
          pipelineGraphs={ctx.pipelineGraphs}
          closePipelineAssignModal={ctx.closePipelineAssignModal}
          savePipelineAssignModal={ctx.savePipelineAssignModal}
          listPipelineTemplatesForAssign={ctx.listPipelineTemplatesForAssign}
          createPipelineFromTemplateAndAssign={ctx.createPipelineFromTemplateAndAssign}
        />
      {:else}
        <div class="rounded border border-surface-800/60 bg-surface-950/50 px-3 py-4 text-xs uppercase tracking-[0.24em] text-surface-400">
          Loading {activeTabLabel}…
        </div>
      {/if}
    {:else if ctx.streamBindings.activeTab === 'pose'}
      {#if CameraPoseTabComponent}
        {@const CameraPoseTab = CameraPoseTabComponent}
        <CameraPoseTab cameraRef={ctx.resolvePoseCameraRef(ctx.stream, ctx.streamId)} />
      {:else}
        <div class="rounded border border-surface-800/60 bg-surface-950/50 px-3 py-4 text-xs uppercase tracking-[0.24em] text-surface-400">
          Loading {activeTabLabel}…
        </div>
      {/if}
    {:else if ctx.streamBindings.activeTab === 'calibration'}
      {#if CameraCalibrationTabComponent}
        {@const CameraCalibrationTab = CameraCalibrationTabComponent}
        <CameraCalibrationTab
          streamId={ctx.streamId}
          streamUuid={ctx.stream?.id ?? null}
          streamIdEffective={ctx.stream?.id ?? ctx.streamId}
          apiPath={ctx.apiPath}
          sourceResolution={ctx.activeMode?.format?.resolution ?? null}
          currentCalibrationParams={ctx.currentCalibrationParams}
          guidedModeEnabled={ctx.calibrationState.calibrationGuidedMode}
          guidedModeBusy={ctx.calibrationState.calibrationGuidedBusy}
          setGuidedModeEnabled={(next) => void ctx.setGuidedCalibrationMode(Boolean(next))}
          resetGuidedCoverage={ctx.resetGuidedCalibrationCoverage}
          guidedAccumulateLive={ctx.calibrationState.calibrationGuidedAccumulateLive}
          setGuidedAccumulateLive={(next) => (ctx.calibrationState.calibrationGuidedAccumulateLive = Boolean(next))}
          calibrationBoard={ctx.calibrationState.calibrationBoard}
          calibrationLensModel={ctx.calibrationState.calibrationLensModel}
          setCalibrationLensModel={(value) => (ctx.calibrationState.calibrationLensModel = value)}
          calibrationImages={ctx.calibrationState.calibrationImages}
          calibrationOwnPhotosOnly={ctx.calibrationState.calibrationOwnPhotosOnly}
          setCalibrationOwnPhotosOnly={(value) => void ctx.setCalibrationOwnPhotosOnly(Boolean(value))}
          calibrationSelected={ctx.calibrationState.calibrationSelected}
          calibrationLoading={ctx.calibrationState.calibrationLoading}
          calibrationSolving={ctx.calibrationState.calibrationSolving}
          calibrationApplying={ctx.calibrationState.calibrationApplying}
          calibrationDeleting={ctx.calibrationState.calibrationDeleting}
          calibrationIncludeOverlays={ctx.calibrationState.calibrationIncludeOverlays}
          calibrationSolveError={ctx.calibrationState.calibrationSolveError}
          calibrationResult={ctx.calibrationState.calibrationResult}
          calibrationImportSourcesLoading={ctx.calibrationState.calibrationImportSourcesLoading}
          calibrationImporting={ctx.calibrationState.calibrationImporting}
          calibrationImportError={ctx.calibrationState.calibrationImportError}
          calibrationImportSourceId={ctx.calibrationState.calibrationImportSourceId}
          calibrationImportSources={ctx.calibrationState.calibrationImportSources}
          refreshCalibrationImages={ctx.refreshCalibrationImages}
          refreshCalibrationImportSources={ctx.refreshCalibrationImportSources}
          takeCalibrationSnapshot={ctx.takeCalibrationSnapshot}
          deleteCalibrationSnapshot={ctx.deleteCalibrationSnapshot}
          deleteCalibrationOverlays={ctx.deleteCalibrationOverlays}
          solveCalibration={ctx.solveCalibration}
          saveSolvedCalibration={ctx.saveSolvedCalibration}
          copyCalibrationFromSelectedStream={ctx.copyCalibrationFromSelectedStream}
          importCalibrationFromJsonFile={ctx.importCalibrationFromJsonFile}
          calibrationPreviewOpen={ctx.calibrationState.calibrationPreviewOpen}
          calibrationPreviewItem={ctx.calibrationState.calibrationPreviewItem}
          openCalibrationPreview={ctx.openCalibrationPreview}
          closeCalibrationPreview={ctx.closeCalibrationPreview}
          setCalibrationSelected={(next) => (ctx.calibrationState.calibrationSelected = next)}
          setCalibrationIncludeOverlays={(value) => (ctx.calibrationState.calibrationIncludeOverlays = value)}
          setCalibrationImportSourceId={(value) => (ctx.calibrationState.calibrationImportSourceId = value)}
          ipaLoading={ctx.calibrationState.ipaLoading}
          ipaStatus={ctx.calibrationState.ipaStatus}
          refreshIpaStatus={ctx.refreshIpaStatus}
          applyIpaCcm={ctx.applyIpaCcm}
          openIpaChartSolverForImage={ctx.openIpaChartSolverForImage}
          ipaCt={ctx.calibrationState.ipaCt}
          setIpaCt={(value) => (ctx.calibrationState.ipaCt = value)}
          ipaCcm={ctx.calibrationState.ipaCcm}
          ipaAdvanced={ctx.calibrationState.ipaAdvanced}
          setIpaAdvanced={(value) => (ctx.calibrationState.ipaAdvanced = value)}
          ipaTarget={ctx.calibrationState.ipaTarget}
          setIpaTarget={(value) => (ctx.calibrationState.ipaTarget = value)}
          setIpaCcm={(value) => (ctx.calibrationState.ipaCcm = value)}
          ipaChartImage={ctx.calibrationState.ipaChartImage}
          setIpaChartImage={(value) => (ctx.calibrationState.ipaChartImage = value)}
          ipaChartModalOpen={ctx.calibrationState.ipaChartModalOpen}
          closeIpaChartSolver={ctx.closeIpaChartSolver}
          addIpaChartCorner={ctx.addIpaChartCorner}
          ipaChartCorners={ctx.calibrationState.ipaChartCorners}
          setIpaChartCorners={(value) => (ctx.calibrationState.ipaChartCorners = value)}
          ipaChartNaturalSize={ctx.ipaChartNaturalSize}
          setIpaChartNaturalSize={(value) => (ctx.calibrationState.ipaChartNaturalSize = value)}
          solveIpaChartCcm={ctx.solveIpaChartCcm}
          ipaChartSolveBusy={ctx.ipaChartSolveBusy}
          ipaChartSolveError={ctx.ipaChartSolveError}
          ipaChartSolveResult={ctx.ipaChartSolveResult}
        />
      {:else}
        <div class="rounded border border-surface-800/60 bg-surface-950/50 px-3 py-4 text-xs uppercase tracking-[0.24em] text-surface-400">
          Loading {activeTabLabel}…
        </div>
      {/if}
    {/if}
  {/snippet}
</CameraStreamSidebar>
