<script lang="ts">
  import CameraCalibrationTab from '$lib/features/devices/camera/CameraCalibrationTab.svelte';
  import CameraControlsTab from '$lib/features/devices/camera/CameraControlsTab.svelte';
  import CameraMediaTab from '$lib/features/devices/camera/CameraMediaTab.svelte';
  import CameraPipelinesTab from '$lib/features/devices/camera/CameraPipelinesTab.svelte';
  import CameraPoseTab from '$lib/features/devices/camera/CameraPoseTab.svelte';
  import CameraStreamTab from '$lib/features/devices/camera/CameraStreamTab.svelte';
  import CameraStreamSidebar from './CameraStreamSidebar.svelte';

  const { ctx } = $props<{ ctx: any }>();
  let streamBindings = $state(ctx.streamBindings);
  let pipelineBindings = $state(ctx.pipelineBindings);
</script>

<CameraStreamSidebar tabs={ctx.tabs} bind:activeTab={streamBindings.activeTab}>
  {#snippet content()}
    {#if streamBindings.activeTab === 'stream'}
      <CameraStreamTab
        bind:cameraAlias={streamBindings.cameraAlias}
        bind:selectedBackendIndex={streamBindings.selectedBackendIndex}
        bind:selectedFormat={streamBindings.selectedFormat}
        bind:selectedResolution={streamBindings.selectedResolution}
        bind:selectedIntervalIdx={streamBindings.selectedIntervalIdx}
        bind:selectedInterval={streamBindings.selectedInterval}
        bind:libcameraTargetFps={streamBindings.libcameraTargetFps}
        bind:netcamTargetFps={streamBindings.netcamTargetFps}
        bind:fileBackendFps={streamBindings.fileBackendFps}
        bind:fileBackendLoop={streamBindings.fileBackendLoop}
        bind:fileBackendPathsText={streamBindings.fileBackendPathsText}
        bind:decoderImpl={streamBindings.decoderImpl}
        bind:encoderImpl={streamBindings.encoderImpl}
        bind:decoderEnabled={streamBindings.decoderEnabled}
        bind:encoderEnabled={streamBindings.encoderEnabled}
        bind:decoderSelectionTouched={streamBindings.decoderSelectionTouched}
        bind:encoderSelectionTouched={streamBindings.encoderSelectionTouched}
        bind:hostBuffer={streamBindings.hostBuffer}
        bind:decoderFpsLimit={streamBindings.decoderFpsLimit}
        bind:decoderRotationDegrees={streamBindings.decoderRotationDegrees}
        bind:decoderMirrorHorizontal={streamBindings.decoderMirrorHorizontal}
        bind:shadowRecorderEnabled={streamBindings.shadowRecorderEnabled}
        bind:encoderFpsLimit={streamBindings.encoderFpsLimit}
        bind:encoderSettingsOpen={streamBindings.encoderSettingsOpen}
        bind:encoderSettings={streamBindings.encoderSettings}
        bind:streamCrop={streamBindings.streamCrop}
        bind:streamCropGuidesEnabled={streamBindings.streamCropGuidesEnabled}
        bind:streamCropApplying={streamBindings.streamCropApplying}
        bind:streamCropError={streamBindings.streamCropError}
        bind:streamCropWarning={streamBindings.streamCropWarning}
        bind:streamCrosshair={streamBindings.streamCrosshair}
        bind:streamCrosshairEnabled={streamBindings.streamCrosshairEnabled}
        bind:streamCrosshairGuidesEnabled={streamBindings.streamCrosshairGuidesEnabled}
        bind:streamCrosshairApplying={streamBindings.streamCrosshairApplying}
        bind:streamCrosshairError={streamBindings.streamCrosshairError}
        bind:streamCrosshairWarning={streamBindings.streamCrosshairWarning}
        bind:streamOrderingMode={streamBindings.streamOrderingMode}
        bind:streamOrderingApplying={streamBindings.streamOrderingApplying}
        bind:streamOrderingError={streamBindings.streamOrderingError}
        bind:streamOrderingWarning={streamBindings.streamOrderingWarning}
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
    {:else if streamBindings.activeTab === 'media'}
      <CameraMediaTab streamId={ctx.stream?.id ?? ctx.streamId} />
    {:else if streamBindings.activeTab === 'controls'}
      <CameraControlsTab
        controls={ctx.controls}
        bind:controlsQuery={streamBindings.controlsQuery}
        bind:controlState={streamBindings.controlState}
        bind:controlAppliedState={streamBindings.controlAppliedState}
        bind:controlBusy={streamBindings.controlBusy}
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
    {:else if streamBindings.activeTab === 'pipelines'}
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
        streamLabel={streamBindings.cameraAlias}
        bind:pipelineGridRows={pipelineBindings.pipelineGridRows}
        bind:pipelineGridColumns={pipelineBindings.pipelineGridColumns}
        pipelineGridRowIndices={ctx.pipelineGridRowIndices}
        pipelineGridColumnIndices={ctx.pipelineGridColumnIndices}
        pipelineForCell={ctx.pipelineForCell}
        outputSelectionForPipeline={ctx.outputSelectionForPipeline}
        outputDurationForCell={(row, column, pipelineId, outputKey) => ctx.outputDurationForPipeline(pipelineId, outputKey)}
        outputKeyForCell={ctx.outputKeyForCell}
        pipelineWires={((ctx.stream?.manifest as any)?.pipeline_wires ?? (ctx.stream?.manifest as any)?.pipelineWires ?? [])}
        setFrameSourceForPipelineInstance={ctx.setFrameSourceForPipelineInstance}
        pipelineOutputOptionsCache={ctx.pipelineOutputOptionsCache}
        ensurePipelineOutputsLoaded={ctx.ensurePipelineOutputsLoaded}
        ensurePipelineGraphAndOutputs={ctx.ensurePipelineGraphAndOutputs}
        schedulePipelineLayoutApply={ctx.schedulePipelineLayoutApply}
        allowDrop={ctx.allowDrop}
        dropOnCell={ctx.dropOnCell}
        clearCell={ctx.clearCell}
        refreshPipelineGraphs={ctx.refreshPipelineGraphs}
        bind:selectedPipelineOutput={pipelineBindings.selectedPipelineOutput}
        setOutputSelectionForPipeline={ctx.setOutputSelectionForPipeline}
        setOutputKeyForCell={ctx.setOutputKeyForCell}
        setLivePipelineOutput={ctx.setLivePipelineOutput}
        bind:pipelineRemoveModalOpen={pipelineBindings.pipelineRemoveModalOpen}
        bind:pipelineRemoveCandidateId={pipelineBindings.pipelineRemoveCandidateId}
        closePipelineRemoveModal={ctx.closePipelineRemoveModal}
        confirmPipelineRemove={ctx.confirmPipelineRemove}
        bind:pipelineAssignModalOpen={pipelineBindings.pipelineAssignModalOpen}
        bind:pipelineAssignQuery={pipelineBindings.pipelineAssignQuery}
        bind:pipelineAssignDraft={pipelineBindings.pipelineAssignDraft}
        pipelineAssignFilteredGraphs={ctx.pipelineAssignFilteredGraphs}
        pipelineGraphs={ctx.pipelineGraphs}
        closePipelineAssignModal={ctx.closePipelineAssignModal}
        savePipelineAssignModal={ctx.savePipelineAssignModal}
        listPipelineTemplatesForAssign={ctx.listPipelineTemplatesForAssign}
        createPipelineFromTemplateAndAssign={ctx.createPipelineFromTemplateAndAssign}
      />
    {:else if streamBindings.activeTab === 'pose'}
      <CameraPoseTab cameraRef={ctx.resolvePoseCameraRef(ctx.stream, ctx.streamId)} />
    {:else if streamBindings.activeTab === 'calibration'}
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
    {/if}
  {/snippet}
</CameraStreamSidebar>
