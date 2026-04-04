<script lang="ts">
  import LocalizationConfigPanel from '$lib/features/localization/page/LocalizationConfigEditor.svelte';
  import SolverPanel from '$lib/features/localization/page/SolverPanel.svelte';
  import CameraPoseOverlay from '$lib/features/localization/page/CameraPoseOverlay.svelte';
  import FieldMapManager from '$lib/features/localization/page/FieldMapManager.svelte';
  import type { LocalizationWorkspaceProps } from './localizationWorkspaceTypes';

  let {
    showImuRotationOverlay = $bindable(false),
    openImuRotationOverlay = () => {},
    imuWindowEl = $bindable<HTMLElement | null>(null),
    imuWindowStyle = () => '',
    ImuOrientationViewerComponent = null,
    imuOrientationForViewer = null,
    imuRotationData = null,
    imuRotationStatusMessage = null,
    beginImuWindowDrag = () => {},
    beginImuWindowResize = () => {},

    showOutputsOverlay = $bindable(false),
    activeProfileId = null,
    localizationConfigLoading = false,
    profileNameInput = $bindable(''),
    onCommitProfileName = undefined,
    activeSolverId = '',
    solvers = [],
    onSetActiveSolverId = undefined,
    onAddSolver = undefined,
    onRemoveActiveSolver = undefined,
    solverNameInput = $bindable(''),
    onCommitSolverName = undefined,
    activeSolverMode = null,
    supportedSolverModes = [],
    onSetSolverMode = undefined,
    activeSolverSourceIds = [],
    onSetActiveSolverUseAllSources = undefined,
    onToggleActiveSolverSource = undefined,
    solvePoseSpaces = [],
    derivedPoseSpaces = [],
    poseSpaceLabel = (space: string) => space,
    selectedFieldMapId = null,
    calibrationReady = false,
    uncalibratedSourcesCount = 0,
    tagSizeInput = $bindable(''),
    tagSizeError = null,
    onCommitTagSize = undefined,
    excludedTagIdsInput = $bindable(''),
    excludedTagIdsError = null,
    onCommitExcludedTagIds = undefined,
    fieldOriginMode = $bindable('blue'),
    fieldOriginCustom = { x: 0, z: 0, yawDeg: 0 },
    onSetFieldOriginMode = undefined,
    onSetFieldOriginCustomNumeric = undefined,
    snapZToGround = false,
    snapRollToGround = false,
    snapPitchToGround = false,
    onSetSnapZToGround = undefined,
    onSetSnapRollToGround = undefined,
    onSetSnapPitchToGround = undefined,
    profileTemporalStabilization = null,
    onSetProfileTemporalEnabled = undefined,
    onSetProfileTemporalNumeric = undefined,
    activeSolverTemporalOverride = null,
    activeSolverTemporalEffective = null,
    onSetSolverTemporalOverrideEnabled = undefined,
    onSetSolverTemporalEnabled = undefined,
    onSetSolverTemporalNumeric = undefined,
    activeSolverRuntimeTuning = null,
    onSetSolverRuntimeTuningNumeric = undefined,
    fieldMaps = [],
    fieldMapsLoading = false,
    fieldMapsError = null,
    mapUploadBusy = false,
    mapUploadError = null,
    fieldMapSelection = $bindable(''),
    onSetFieldMapSelection = undefined,
    onUploadMapFile = undefined,
    compatibleSourcesCount = 0,
    sourcesLoading = false,
    sourcesError = null,
    groupedSources = [],
    openSourceGroups = [],
    onToggleSourceGroup = undefined,
    calibratedCameraIds = new Set<string>(),
    isSourceCalibrated = undefined,
    selectedSourceIds = [],
    onToggleSource = undefined,
    sourceWeightsById = {},
    onSetSourceWeight = undefined,
    sourceUsedByProfilesById = {},
    sourceStatusRows = [],

    showMetricsOverlay = $bindable(false),
    feedStatus = 'idle',
    pollHz = 0,
    lastPollMs = null,
    activeSolveMs = null,
    profileTimingRows = [],

    showCameraPoseOverlay = $bindable(false),
    primaryCameraKey = null,
    cameraPoseXInput = $bindable(''),
    cameraPoseYInput = $bindable(''),
    cameraPoseZInput = $bindable(''),
    cameraPosePitchDeg = $bindable(''),
    cameraPoseYawDeg = $bindable(''),
    cameraPoseRollDeg = $bindable(''),
    cameraPoseEditorError = null,
    onResetPrimaryCameraPoseInputs = undefined,
    onApplyPrimaryCameraPose = undefined,

    showCustomFieldsOverlay = $bindable(false),
    newCustomFieldName = $bindable(''),
    newCustomFieldWidth = $bindable(''),
    newCustomFieldDepth = $bindable(''),
    newCustomFieldError = null,
    onCreateCustomField = undefined,
    newOriginName = $bindable(''),
    newOriginX = $bindable(''),
    newOriginZ = $bindable(''),
    newOriginYaw = $bindable(''),
    newOriginError = null,
    onAddOrigin = undefined,
    selectedCustomField = null,
    activeFieldMapBitsStatus = null,
    onRefreshMaps = undefined,
    mapUploadFile = null,
    mapAssignId = $bindable(''),
    onAssignMap = undefined,
    fieldMapDocErrors = {},
    hasActiveProfile = false,
    onSetMapUploadFile = undefined,
    onUploadSelectedMapFile = undefined
  }: LocalizationWorkspaceProps = $props();

  function formatFixed(value: number, digits = 2): string {
    return Number.isFinite(value) ? value.toFixed(digits) : '0';
  }

  function formatSigned(value: number, digits = 2): string {
    const out = formatFixed(value, digits);
    return value >= 0 ? `+${out}` : out;
  }
</script>

{#if !showImuRotationOverlay}
  <div class="pointer-events-auto absolute bottom-20 right-4 z-[69]">
    <button
      type="button"
      class="rounded border border-surface-700/70 bg-surface-950/88 px-3 py-2 text-micro-tight uppercase tracking-[0.24em] text-surface-200 shadow-lg transition hover:border-primary-400/80 hover:text-primary-100"
      onclick={openImuRotationOverlay}
    >
      IMU Viewer
    </button>
  </div>
{/if}

{#if showImuRotationOverlay}
  <div
    bind:this={imuWindowEl}
    class="pointer-events-auto absolute z-[70] max-w-[96vw] overflow-hidden rounded border border-surface-800 bg-surface-950/80 text-xs text-surface-200 shadow-xl backdrop-blur"
    style={imuWindowStyle()}
  >
    {#if imuRotationData}
      <div class="relative h-full w-full">
        {#if ImuOrientationViewerComponent}
          <ImuOrientationViewerComponent
            orientation={imuOrientationForViewer}
            showReferenceControls={false}
            showLegend={false}
            showWorldDecorations={false}
            showGroundPlane={true}
            cameraDistanceScale={0.22}
          />
        {:else}
          <div class="flex h-full w-full items-center justify-center bg-surface-950/60 text-xs text-surface-500">
            Loading IMU viewer…
          </div>
        {/if}
        <div class="pointer-events-none absolute inset-0 flex flex-col justify-between p-3">
          <div class="flex items-start justify-between gap-2">
            <div
              class="pointer-events-auto max-w-[75%] cursor-move select-none rounded border border-surface-700/70 bg-surface-950/72 px-2 py-1.5"
              style="touch-action:none;"
              role="presentation"
              onpointerdown={beginImuWindowDrag}
            >
              <p class="text-micro-tight uppercase tracking-[0.3em] text-surface-300">IMU Rotation</p>
              <p class="truncate text-[0.65rem] text-surface-400">{imuRotationData.sourceLabel} · {imuRotationData.outputKey}</p>
            </div>
            <button
              type="button"
              class="pointer-events-auto rounded border border-surface-700/70 bg-surface-950/72 px-2 py-0.5 text-micro-tight uppercase tracking-[0.2em] text-surface-300 transition hover:border-primary-400/80 hover:text-primary-100"
              onclick={() => (showImuRotationOverlay = false)}
            >
              Close
            </button>
          </div>

          <div class="space-y-2">
            <div class="rounded border border-surface-700/70 bg-surface-950/72 px-2 py-1.5 font-mono tabular-nums text-[0.67rem]">
              <p>
                <span style="color: var(--axis-roll)">R {formatSigned(imuRotationData.roll, 2)}°</span>
                <span class="mx-1 text-surface-600">|</span>
                <span style="color: var(--axis-pitch)">P {formatSigned(imuRotationData.pitch, 2)}°</span>
                <span class="mx-1 text-surface-600">|</span>
                <span style="color: var(--axis-yaw)">Y {formatSigned(imuRotationData.yaw, 2)}°</span>
              </p>
              {#if imuRotationData.quaternion}
                <p class="truncate text-[0.62rem] text-surface-400">
                  q ({formatFixed(imuRotationData.quaternion.x, 3)}, {formatFixed(imuRotationData.quaternion.y, 3)}, {formatFixed(imuRotationData.quaternion.z, 3)}, {formatFixed(imuRotationData.quaternion.w, 3)})
                </p>
              {/if}
              {#if imuRotationData.translation}
                <p class="truncate text-[0.62rem] text-surface-400">
                  t ({formatFixed(imuRotationData.translation.x, 3)}, {formatFixed(imuRotationData.translation.y, 3)}, {formatFixed(imuRotationData.translation.z, 3)})
                </p>
              {/if}
            </div>
            <div class="flex items-center justify-between rounded border border-surface-700/70 bg-surface-950/72 px-2 py-1 font-mono tabular-nums text-[0.62rem] text-surface-400">
              <span>age {Math.max(0, imuRotationData.ageMs).toFixed(0)} ms</span>
              <span>sample {imuRotationData.sampleTimestampMs != null ? `${imuRotationData.sampleTimestampMs.toFixed(0)} ms` : '—'}</span>
            </div>
          </div>
        </div>
      </div>
    {:else}
      <div class="relative h-full w-full bg-surface-950/90">
        <div class="absolute left-3 top-3">
          <div
            class="cursor-move select-none rounded border border-surface-700/70 bg-surface-950/72 px-2 py-0.5 text-micro-tight uppercase tracking-[0.2em] text-surface-300"
            style="touch-action:none;"
            role="presentation"
            onpointerdown={beginImuWindowDrag}
          >
            IMU Rotation
          </div>
        </div>
        <div class="absolute right-3 top-3">
          <button
            type="button"
            class="rounded border border-surface-700/70 bg-surface-950/72 px-2 py-0.5 text-micro-tight uppercase tracking-[0.2em] text-surface-300 transition hover:border-primary-400/80 hover:text-primary-100"
            onclick={() => (showImuRotationOverlay = false)}
          >
            Close
          </button>
        </div>
        <div class="flex h-full items-center justify-center p-4 text-micro-tight text-surface-400">
          {imuRotationStatusMessage ?? 'Waiting for IMU sample...'}
        </div>
      </div>
    {/if}
    <div
      class="absolute bottom-0 right-0 h-5 w-5 cursor-se-resize pointer-events-auto"
      style="touch-action:none;"
      role="presentation"
      onpointerdown={beginImuWindowResize}
    >
      <div class="absolute bottom-1 right-1 h-2.5 w-2.5 border-b border-r border-surface-400/80"></div>
    </div>
  </div>
{/if}

{#if showOutputsOverlay}
  <LocalizationConfigPanel
    open={showOutputsOverlay}
    onClose={() => (showOutputsOverlay = false)}
    {activeProfileId}
    {localizationConfigLoading}
    bind:profileNameInput={profileNameInput}
    {onCommitProfileName}
    {activeSolverId}
    {solvers}
    {onSetActiveSolverId}
    {onAddSolver}
    {onRemoveActiveSolver}
    bind:solverNameInput={solverNameInput}
    {onCommitSolverName}
    {activeSolverMode}
    {supportedSolverModes}
    {onSetSolverMode}
    {activeSolverSourceIds}
    {onSetActiveSolverUseAllSources}
    {onToggleActiveSolverSource}
    {solvePoseSpaces}
    {derivedPoseSpaces}
    {poseSpaceLabel}
    {selectedFieldMapId}
    {calibrationReady}
    {uncalibratedSourcesCount}
    bind:tagSizeInput={tagSizeInput}
    {tagSizeError}
    {onCommitTagSize}
    bind:excludedTagIdsInput={excludedTagIdsInput}
    {excludedTagIdsError}
    {onCommitExcludedTagIds}
    {fieldOriginMode}
    {fieldOriginCustom}
    {onSetFieldOriginMode}
    {onSetFieldOriginCustomNumeric}
    {snapZToGround}
    {snapRollToGround}
    {snapPitchToGround}
    {onSetSnapZToGround}
    {onSetSnapRollToGround}
    {onSetSnapPitchToGround}
    {profileTemporalStabilization}
    {onSetProfileTemporalEnabled}
    {onSetProfileTemporalNumeric}
    {activeSolverTemporalOverride}
    {activeSolverTemporalEffective}
    {onSetSolverTemporalOverrideEnabled}
    {onSetSolverTemporalEnabled}
    {onSetSolverTemporalNumeric}
    {activeSolverRuntimeTuning}
    {onSetSolverRuntimeTuningNumeric}
    {fieldMaps}
    {fieldMapsLoading}
    {fieldMapsError}
    {mapUploadBusy}
    {mapUploadError}
    bind:fieldMapSelection={fieldMapSelection}
    {onSetFieldMapSelection}
    {onUploadMapFile}
    {compatibleSourcesCount}
    {sourcesLoading}
    {sourcesError}
    {groupedSources}
    {openSourceGroups}
    {onToggleSourceGroup}
    {calibratedCameraIds}
    {isSourceCalibrated}
    {selectedSourceIds}
    {onToggleSource}
    {sourceWeightsById}
    {onSetSourceWeight}
    {sourceUsedByProfilesById}
    {sourceStatusRows}
  />
{/if}

<SolverPanel
  open={showMetricsOverlay}
  {feedStatus}
  {pollHz}
  {lastPollMs}
  {activeSolveMs}
  {sourceStatusRows}
  {profileTimingRows}
  onClose={() => (showMetricsOverlay = false)}
/>

{#if showCameraPoseOverlay || showCustomFieldsOverlay}
  <div class="absolute inset-4 z-50 pointer-events-none">
    <div class="grid max-h-full grid-cols-1 content-start gap-3 lg:grid-cols-2">
      {#if showCameraPoseOverlay}
        <CameraPoseOverlay
          onClose={() => (showCameraPoseOverlay = false)}
          {primaryCameraKey}
          bind:cameraPoseXInput={cameraPoseXInput}
          bind:cameraPoseYInput={cameraPoseYInput}
          bind:cameraPoseZInput={cameraPoseZInput}
          bind:cameraPosePitchDeg={cameraPosePitchDeg}
          bind:cameraPoseYawDeg={cameraPoseYawDeg}
          bind:cameraPoseRollDeg={cameraPoseRollDeg}
          {cameraPoseEditorError}
          onReset={onResetPrimaryCameraPoseInputs}
          onApply={onApplyPrimaryCameraPose}
        />
      {/if}

      {#if showCustomFieldsOverlay}
        <FieldMapManager
          onClose={() => (showCustomFieldsOverlay = false)}
          bind:newCustomFieldName={newCustomFieldName}
          bind:newCustomFieldWidth={newCustomFieldWidth}
          bind:newCustomFieldDepth={newCustomFieldDepth}
          {newCustomFieldError}
          {onCreateCustomField}
          bind:newOriginName={newOriginName}
          bind:newOriginX={newOriginX}
          bind:newOriginZ={newOriginZ}
          bind:newOriginYaw={newOriginYaw}
          {newOriginError}
          {onAddOrigin}
          hasSelectedCustomField={Boolean(selectedCustomField)}
          {activeFieldMapBitsStatus}
          {onRefreshMaps}
          {fieldMapsLoading}
          {mapUploadFile}
          {mapUploadBusy}
          {mapUploadError}
          {fieldMapsError}
          {fieldMaps}
          bind:mapAssignId={mapAssignId}
          {onAssignMap}
          {selectedFieldMapId}
          {fieldMapDocErrors}
          {hasActiveProfile}
          {onSetMapUploadFile}
          {onUploadSelectedMapFile}
        />
      {/if}
    </div>
  </div>
{/if}
