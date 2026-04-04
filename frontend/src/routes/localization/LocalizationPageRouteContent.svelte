<script lang="ts">
  import LocalizationWorkspace from '$lib/features/localization/page/LocalizationWorkspace.svelte';
  import LocalizationProfileSidebar from './LocalizationProfileSidebar.svelte';
  import LocalizationDeleteProfileModal from './LocalizationDeleteProfileModal.svelte';

  let { state }: { state: Record<string, any> } = $props();
</script>

<div class="flex h-full min-h-0 flex-1 flex-col gap-4 overflow-hidden">
  <section class="flex min-h-0 flex-1 gap-4 overflow-hidden lg:gap-6">
    <LocalizationProfileSidebar
      profileTransferBusy={state.profileTransferBusy}
      canExport={Boolean(state.localizationConfig?.profiles?.length || state.localizationConfig)}
      bind:profileImportInputEl={state.profileImportInputEl}
      bind:profileSearch={state.profileSearch}
      filteredProfiles={state.filteredProfiles}
      activeProfileId={state.activeProfileId}
      coordinateSpace={state.coordinateSpace}
      profileSupportedSpacesById={state.profileSupportedSpacesById}
      profiles={state.profiles}
      profileIndexById={state.profileIndexById}
      onCreateProfile={state.addProfile}
      onExportProfiles={state.exportLocalizationProfiles}
      onOpenImportProfilesDialog={state.openImportProfilesDialog}
      onHandleProfileImportInput={state.handleProfileImportInput}
      onSetActiveProfile={state.setActiveProfile}
      onSetProfileEnabled={state.setProfileEnabled}
      onSetProfileViewEnabled={state.setProfileViewEnabled}
      onHandleProfileColorInput={state.handleProfileColorInput}
      onOpenDeleteProfileModal={state.openDeleteProfileModal}
      bind:showOutputsOverlay={state.showOutputsOverlay}
      bind:showMetricsOverlay={state.showMetricsOverlay}
    />

    <div class="min-w-0 flex flex-1 flex-col">
      {#if state.showLocalizationBootLoading}
        <section class="flex min-h-0 flex-1 items-center justify-center rounded border border-surface-800/60 bg-surface-950/60 p-6 text-center">
          <div class="max-w-xl space-y-3">
            <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Loading localization</p>
            <h2 class="text-lg font-semibold text-white">Preparing profiles, sources, field maps, and solver state…</h2>
            <p class="text-sm text-surface-400">The page has painted; the initial workspace data is loading in the background.</p>
          </div>
        </section>
      {:else if state.localizationBootError && !state.hasLocalizationBootstrapData}
        <section class="flex min-h-0 flex-1 items-center justify-center rounded border border-error-500/40 bg-error-500/10 p-6 text-center">
          <div class="max-w-xl space-y-3">
            <p class="text-xs uppercase tracking-[0.3em] text-error-200">Localization failed to load</p>
            <h2 class="text-lg font-semibold text-white">{state.localizationBootError}</h2>
            <button class="btn btn-sm preset-filled-primary-500 uppercase tracking-[0.3em]" type="button" onclick={state.retryLocalizationBootstrap}>
              Retry
            </button>
          </div>
        </section>
      {:else}
        <LocalizationWorkspace
          viewersComponent={state.LocalizationViewersComponent}
          markers={state.viewerMarkers}
          tagLineMarkers={state.viewerTagLineMarkers}
          referenceMarkers={state.referenceMarkersForViewer}
          mode={state.viewMode}
          bumperNumber={state.bumperId}
          bumperColor={state.viewerAccentColor}
          robotOverlays={state.viewProfileOverlays}
          robot={state.rigLayoutState.layout.robot}
          cameras={state.viewerRenderableCamerasForRender}
          cameraTransforms={state.viewerCameraTransformsForRender}
          robotTransform={state.viewerRobotTransformForGhost}
          sceneTransform={state.viewerSceneTransform}
          customField={state.activeCustomField}
          showRobot={state.viewerShowRobot && state.hasAnyViewsEnabled}
          bind:showOriginAxes={state.showOriginAxes}
          bind:showTagLines={state.showTagLines}
          bind:showFieldImage={state.showFieldImage}
          bind:showMinimapTrail={state.showMinimapTrail}
          showCameras={state.viewerShowCameras}
          cameraGhostActive={state.viewerCameraGhostActive}
          cameraHighlightColor={state.viewerAccentColor}
          minimapPoseDot={state.minimapPoseDot}
          cameraPovEnabled={state.viewerCameraPovEnabled}
          cameraPovTransform={state.viewerCameraPovTransform}
          cameraPovIntrinsics={state.viewerCameraPovIntrinsics}
          cameraPovApplyFov={state.viewerCameraPovApplyFov}
          cameraPovForwardSign={state.viewerCameraPovForwardSign}
          feedStatus={state.feedStatus}
          selectedSourceCount={state.selectedSourceIds.length}
          liveMarkerCount={state.liveMarkers.length}
          lastPollMs={state.lastPollMs}
          activeSolveMs={state.activeSolveMs}
          bind:pollHz={state.pollHz}
          pollHzMin={state.pollHzMin}
          pollHzMax={state.pollHzMax}
          pollHzStep={state.pollHzStep}
          feedMessage={state.feedMessage}
          targetSpaceOverlay={state.targetSpaceOverlay}
          baseFrame={state.baseFrame}
          activeProfileId={state.activeProfileId}
          fieldSpaceAllowed={state.fieldSpaceAllowed}
          bind:coordinateSpace={state.coordinateSpace}
          cameraPovOptions={state.cameraPovOptions}
          bind:cameraPovSelectionId={state.cameraPovSelectionId}
          bind:cameraPovFovMode={state.cameraPovFovMode}
          availableCoordinateSpaces={state.availableCoordinateSpaces}
          poseSpaceLabel={state.poseSpaceLabel}
          fieldOriginMode={state.fieldOriginMode}
          fieldOriginCustom={state.profileFieldOriginCustom}
          onSetFieldOriginMode={state.setProfileFieldOriginMode}
          onSetFieldOriginCustomNumeric={state.setProfileFieldOriginCustomNumeric}
          bind:showOutputsOverlay={state.showOutputsOverlay}
          bind:showMetricsOverlay={state.showMetricsOverlay}
          localizationConfigLoading={state.localizationConfigLoading}
          bind:profileNameInput={state.profileNameInput}
          onCommitProfileName={state.commitProfileName}
          activeSolverId={state.activeSolverId}
          solvers={state.activeProfile?.solvers ?? []}
          onSetActiveSolverId={state.setActiveSolverIdForUi}
          onAddSolver={state.addSolver}
          onRemoveActiveSolver={state.removeActiveSolver}
          bind:solverNameInput={state.solverNameInput}
          onCommitSolverName={state.commitSolverName}
          activeSolverMode={state.activeSolverConfig?.mode ?? null}
          supportedSolverModes={state.supportedSolverModes}
          onSetSolverMode={state.setActiveSolverMode}
          activeSolverSourceIds={state.activeSolverConfig?.sourceIds ?? []}
          onSetActiveSolverUseAllSources={state.setActiveSolverUseAllSources}
          onToggleActiveSolverSource={state.toggleActiveSolverSource}
          solvePoseSpaces={state.solvePoseSpaces}
          derivedPoseSpaces={state.derivedPoseSpaces}
          selectedFieldMapId={state.selectedFieldMapId}
          calibrationReady={state.calibrationReady}
          uncalibratedSourcesCount={state.uncalibratedSources.length}
          bind:tagSizeInput={state.tagSizeInput}
          tagSizeError={state.tagSizeError}
          onCommitTagSize={state.commitTagSize}
          bind:excludedTagIdsInput={state.excludedTagIdsInput}
          excludedTagIdsError={state.excludedTagIdsError}
          onCommitExcludedTagIds={state.commitExcludedTagIds}
          snapZToGround={state.activeProfile?.snapZToGround ?? false}
          snapRollToGround={state.activeProfile?.snapRollToGround ?? false}
          snapPitchToGround={state.activeProfile?.snapPitchToGround ?? false}
          onSetSnapZToGround={state.setSnapZToGround}
          onSetSnapRollToGround={state.setSnapRollToGround}
          onSetSnapPitchToGround={state.setSnapPitchToGround}
          profileTemporalStabilization={state.profileTemporalStabilization}
          onSetProfileTemporalEnabled={state.setProfileTemporalEnabled}
          onSetProfileTemporalNumeric={state.setProfileTemporalNumeric}
          activeSolverTemporalOverride={state.activeSolverTemporalOverride}
          activeSolverTemporalEffective={state.activeSolverTemporalEffective}
          onSetSolverTemporalOverrideEnabled={state.setSolverTemporalOverrideEnabled}
          onSetSolverTemporalEnabled={state.setSolverTemporalEnabled}
          onSetSolverTemporalNumeric={state.setSolverTemporalNumeric}
          activeSolverRuntimeTuning={state.activeSolverRuntimeTuning}
          onSetSolverRuntimeTuningNumeric={state.setSolverRuntimeTuningNumeric}
          fieldMaps={state.fieldMaps}
          fieldMapsLoading={state.fieldMapsLoading}
          fieldMapsError={state.fieldMapsError}
          mapUploadBusy={state.mapUploadBusy}
          mapUploadError={state.mapUploadError}
          bind:fieldMapSelection={state.fieldMapSelection}
          onSetFieldMapSelection={state.setFieldMapSelection}
          onUploadMapFile={state.handleMapUploadFile}
          compatibleSourcesCount={state.compatibleSources.length}
          sourcesLoading={state.sourcesLoading}
          sourcesError={state.sourcesError}
          groupedSources={state.groupedSources}
          openSourceGroups={state.openSourceGroups}
          onToggleSourceGroup={state.toggleSourceGroup}
          calibratedCameraIds={state.calibratedCameraIds}
          isSourceCalibrated={state.isSourceCalibrated}
          selectedSourceIds={state.selectedSourceIds}
          onToggleSource={state.toggleSource}
          sourceWeightsById={state.sourceWeightsById}
          onSetSourceWeight={state.setSourceWeight}
          sourceUsedByProfilesById={state.sourceUsedByProfilesById}
          sourceStatusRows={state.sourceStatusRows}
          profileTimingRows={state.profileTimingRows}
          bind:showCameraPoseOverlay={state.showCameraPoseOverlay}
          bind:showCustomFieldsOverlay={state.showCustomFieldsOverlay}
          bind:showImuRotationOverlay={state.showImuRotationOverlay}
          imuRotationData={state.imuRotationOverlayData}
          imuRotationStatusMessage={state.imuRotationStatusMessage}
          primaryCameraKey={state.primaryCameraKey}
          bind:cameraPoseXInput={state.cameraPoseXInput}
          bind:cameraPoseYInput={state.cameraPoseYInput}
          bind:cameraPoseZInput={state.cameraPoseZInput}
          bind:cameraPosePitchDeg={state.cameraPosePitchDeg}
          bind:cameraPoseYawDeg={state.cameraPoseYawDeg}
          bind:cameraPoseRollDeg={state.cameraPoseRollDeg}
          cameraPoseEditorError={state.cameraPoseEditorError}
          onResetPrimaryCameraPoseInputs={state.resetPrimaryCameraPoseInputs}
          onApplyPrimaryCameraPose={state.applyPrimaryCameraPose}
          bind:newCustomFieldName={state.newCustomFieldName}
          bind:newCustomFieldWidth={state.newCustomFieldWidth}
          bind:newCustomFieldDepth={state.newCustomFieldDepth}
          newCustomFieldError={state.newCustomFieldError}
          onCreateCustomField={state.createCustomField}
          bind:newOriginName={state.newOriginName}
          bind:newOriginX={state.newOriginX}
          bind:newOriginZ={state.newOriginZ}
          bind:newOriginYaw={state.newOriginYaw}
          newOriginError={state.newOriginError}
          onAddOrigin={state.addOriginToSelectedField}
          selectedCustomField={state.selectedCustomField}
          activeFieldMapBitsStatus={state.activeFieldMapBitsStatus}
          onRefreshMaps={state.loadFieldMapList}
          mapUploadFile={state.mapUploadFile}
          bind:mapAssignId={state.mapAssignId}
          onAssignMap={state.assignMapToSelectedField}
          fieldMapDocErrors={state.fieldMapDocErrors}
          hasActiveProfile={Boolean(state.activeProfile)}
          onSetMapUploadFile={(file) => (state.mapUploadFile = file)}
          onUploadSelectedMapFile={state.uploadSelectedMapFile}
        />
      {/if}
    </div>
  </section>
</div>

<LocalizationDeleteProfileModal
  pendingProfileDelete={state.pendingProfileDelete}
  profileDeleteBusy={state.profileDeleteBusy}
  profileDeleteError={state.profileDeleteError}
  onClose={state.closeDeleteProfileModal}
  onConfirm={state.confirmDeleteProfile}
/>
