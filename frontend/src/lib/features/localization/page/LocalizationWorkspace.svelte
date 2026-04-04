<script lang="ts">
  import { browser } from '$app/environment';
  import type { LocalizationFieldOriginMode, LocalizationPoseSpace } from '$lib/features/localization/localizationConfig';
  import type { SensorOrientation } from '$lib/types/devices';
  import LocalizationWorkspacePanels from '$lib/features/localization/page/LocalizationWorkspacePanels.svelte';
  import LocalizationWorkspaceViewer from '$lib/features/localization/page/LocalizationWorkspaceViewer.svelte';
  import { createLazySvelteComponentLoader } from '$lib/utils/lazySvelteComponent';
  import { ROBOT_FOLLOW_POV_OPTION_ID } from './localizationWorkspaceTypes';
  import type {
    CameraPovFovMode,
    CameraPovOption,
    CameraPovOptionGroup,
    LocalizationWorkspaceProps
  } from './localizationWorkspaceTypes';

  type ImuOrientationViewerComponent = (typeof import('$lib/components/ImuOrientationViewer.svelte'))['default'];

  const imuOrientationViewerLoader = createLazySvelteComponentLoader<ImuOrientationViewerComponent>(
    () => import('$lib/components/ImuOrientationViewer.svelte')
  );

  let {
    viewersComponent = null,
    markers = [],
    tagLineMarkers = [],
    referenceMarkers = [],
    mode = 'isolated',
    bumperNumber = '',
    bumperColor = null,
    robotOverlays = [],
    robot = { width: 0, length: 0, bumperHeight: 0, bumperThickness: 0, groundClearance: 0 },
    cameras = [],
    cameraTransforms = null,
    robotTransform = null,
    sceneTransform = null,
    customField = null,
    showRobot = true,
    showCameras = true,
    cameraGhostActive = false,
    cameraHighlightColor = null,
    minimapPoseDot = null,
    cameraPovEnabled = false,
    cameraPovTransform = null,
    cameraPovIntrinsics = null,
    cameraPovApplyFov = true,
    cameraPovForwardSign = 1,
    feedStatus = 'idle',
    selectedSourceCount = 0,
    liveMarkerCount = 0,
    lastPollMs = null,
    activeSolveMs = null,
    pollHz = $bindable(30),
    pollHzMin = 1,
    pollHzMax = 240,
    pollHzStep = 1,
    feedMessage = null,
    targetSpaceOverlay = null,
    showOriginAxes = $bindable(true),
    showTagLines = $bindable(false),
    showFieldImage = $bindable(true),
    showMinimapTrail = $bindable(true),
    baseFrame = 'camera',
    activeProfileId = $bindable<string | null>(null),
    fieldSpaceAllowed = false,
    coordinateSpace = $bindable('tag_in_camera' as LocalizationPoseSpace),
    cameraPovOptions = [],
    cameraPovSelectionId = $bindable(''),
    cameraPovFovMode = $bindable('undistorted' as CameraPovFovMode),
    availableCoordinateSpaces = [],
    poseSpaceLabel = (space) => space,
    fieldOriginMode = $bindable('blue' as LocalizationFieldOriginMode),
    showOutputsOverlay = $bindable(false),
    showMetricsOverlay = $bindable(false),
    localizationConfigLoading = false,
    profileNameInput = $bindable(''),
    onCommitProfileName,
    activeSolverId = '',
    solvers = [],
    onSetActiveSolverId,
    onAddSolver,
    onRemoveActiveSolver,
    solverNameInput = $bindable(''),
    onCommitSolverName,
    activeSolverMode = null,
    supportedSolverModes = [],
    onSetSolverMode,
    activeSolverSourceIds = [],
    onSetActiveSolverUseAllSources,
    onToggleActiveSolverSource,
    solvePoseSpaces = [],
    derivedPoseSpaces = [],
    selectedFieldMapId = null,
    calibrationReady = false,
    uncalibratedSourcesCount = 0,
    tagSizeInput = $bindable(''),
    tagSizeError = null,
    onCommitTagSize,
    excludedTagIdsInput = $bindable(''),
    excludedTagIdsError = null,
    onCommitExcludedTagIds,
    fieldOriginCustom = { x: 0, z: 0, yawDeg: 0 },
    onSetFieldOriginMode,
    onSetFieldOriginCustomNumeric,
    snapZToGround = false,
    snapRollToGround = false,
    snapPitchToGround = false,
    onSetSnapZToGround,
    onSetSnapRollToGround,
    onSetSnapPitchToGround,
    profileTemporalStabilization = {
      enabled: true,
      singleTagTranslationAlpha: 0.18,
      singleTagRotationAlpha: 0.16,
      multiTagTranslationAlpha: 0.45,
      multiTagRotationAlpha: 0.38,
      maxTranslationJumpM: 1.2,
      maxRotationJumpDeg: 70,
      reanchorRejectWindowMs: 450
    },
    onSetProfileTemporalEnabled,
    onSetProfileTemporalNumeric,
    activeSolverTemporalOverride = null,
    activeSolverTemporalEffective = {
      enabled: true,
      singleTagTranslationAlpha: 0.18,
      singleTagRotationAlpha: 0.16,
      multiTagTranslationAlpha: 0.45,
      multiTagRotationAlpha: 0.38,
      maxTranslationJumpM: 1.2,
      maxRotationJumpDeg: 70,
      reanchorRejectWindowMs: 450
    },
    onSetSolverTemporalOverrideEnabled,
    onSetSolverTemporalEnabled,
    onSetSolverTemporalNumeric,
    activeSolverRuntimeTuning = {
      minObservationWeight: 0.03,
      minSingleTagSolveWeight: 0.34,
      minMultiTagTotalWeight: 0.58,
      minMultiTagEffectiveCount: 1.2,
      weakSingleTagMargin: 0.08,
      coplanarHeightDeltaM: 0.08,
      severeObservedHeightDeltaM: 0.45,
      moderateObservedHeightDeltaM: 0.25,
      mildObservedHeightDeltaM: 0.15,
      severePenalty: 0.1,
      moderatePenalty: 0.3,
      mildPenalty: 0.6,
      dtScaleMin: 0.4,
      dtScaleMax: 2.5,
      switchedSingleTagMaxTranslationJumpM: 0.38,
      switchedSingleTagMaxRotationJumpDeg: 24,
      droppedMultiToSingleMaxTranslationJumpM: 0.58,
      droppedMultiToSingleMaxRotationJumpDeg: 36,
      switchedSingleTagRejectWindowScale: 1.8,
      switchedSingleTagRejectWindowMinMs: 700,
      droppedMultiToSingleRejectWindowScale: 1.3,
      droppedMultiToSingleRejectWindowMinMs: 520,
      switchedSingleTagGainDamp: 0.35,
      switchedSingleTagMinTranslationGain: 0.04,
      switchedSingleTagMinRotationGain: 0.04,
      droppedMultiToSingleGainDamp: 0.5,
      droppedMultiToSingleMinTranslationGain: 0.06,
      droppedMultiToSingleMinRotationGain: 0.06
    },
    onSetSolverRuntimeTuningNumeric,
    fieldMaps = [],
    fieldMapsLoading = false,
    fieldMapsError = null,
    mapUploadBusy = false,
    mapUploadError = null,
    fieldMapSelection = $bindable(''),
    onSetFieldMapSelection,
    onUploadMapFile,
    compatibleSourcesCount = 0,
    sourcesLoading = false,
    sourcesError = null,
    groupedSources = [],
    openSourceGroups = [],
    onToggleSourceGroup,
    calibratedCameraIds = new Set<string>(),
    isSourceCalibrated = () => true,
    selectedSourceIds = [],
    onToggleSource,
    sourceWeightsById = {},
    onSetSourceWeight,
    sourceUsedByProfilesById = {},
    sourceStatusRows = [],
    profileTimingRows = [],
    showCameraPoseOverlay = $bindable(false),
    showCustomFieldsOverlay = $bindable(false),
    showImuRotationOverlay = $bindable(false),
    imuRotationData = null,
    imuRotationStatusMessage = null,
    primaryCameraKey = null,
    cameraPoseXInput = $bindable(''),
    cameraPoseYInput = $bindable(''),
    cameraPoseZInput = $bindable(''),
    cameraPosePitchDeg = $bindable('0'),
    cameraPoseYawDeg = $bindable('0'),
    cameraPoseRollDeg = $bindable('0'),
    cameraPoseEditorError = null,
    onResetPrimaryCameraPoseInputs,
    onApplyPrimaryCameraPose,
    newCustomFieldName = $bindable('Custom field'),
    newCustomFieldWidth = $bindable(''),
    newCustomFieldDepth = $bindable(''),
    newCustomFieldError = null,
    onCreateCustomField,
    newOriginName = $bindable('Origin'),
    newOriginX = $bindable('0m'),
    newOriginZ = $bindable('0m'),
    newOriginYaw = $bindable('0'),
    newOriginError = null,
    onAddOrigin,
    selectedCustomField = null,
    activeFieldMapBitsStatus = null,
    onRefreshMaps,
    mapUploadFile = null,
    mapAssignId = $bindable(''),
    onAssignMap,
    fieldMapDocErrors = {},
    hasActiveProfile = false,
    onSetMapUploadFile,
    onUploadSelectedMapFile
  }: LocalizationWorkspaceProps = $props();

  let ImuOrientationViewerComponent = $state<ImuOrientationViewerComponent | null>(
    imuOrientationViewerLoader.current()
  );

  const ViewersComponent = $derived(viewersComponent);
  const groupedCameraPovOptions = $derived.by<CameraPovOptionGroup[]>(() => {
    const groups = new Map<string, Map<string, CameraPovOption[]>>();
    for (const option of cameraPovOptions) {
      const groupKey = (option.groupLabel ?? '').trim() || 'Profile';
      const subgroupKey = (option.subgroupLabel ?? '').trim() || 'Camera';
      const subgroups = groups.get(groupKey) ?? new Map<string, CameraPovOption[]>();
      const list = subgroups.get(subgroupKey) ?? [];
      list.push(option);
      subgroups.set(subgroupKey, list);
      groups.set(groupKey, subgroups);
    }
    return Array.from(groups.entries()).map(([label, subgroups]) => ({
      label,
      subgroups: Array.from(subgroups.entries()).map(([subLabel, options]) => ({ label: subLabel, options }))
    }));
  });
  const selectedCameraPovOption = $derived.by<CameraPovOption | null>(() => {
    const selected = cameraPovSelectionId.trim();
    if (!selected || selected === ROBOT_FOLLOW_POV_OPTION_ID) return null;
    return cameraPovOptions.find((option) => option.id === selected) ?? null;
  });

  function handleFieldOriginModeChange(event: Event): void {
    const target = event.currentTarget;
    if (!(target instanceof HTMLSelectElement)) {
      return;
    }
    onSetFieldOriginMode?.(target.value as LocalizationFieldOriginMode);
  }

  async function ensureImuOrientationViewer(): Promise<void> {
    ImuOrientationViewerComponent ??= await imuOrientationViewerLoader.load();
  }

  $effect(() => {
    if (!browser || !showImuRotationOverlay || !imuRotationData) return;
    void ensureImuOrientationViewer();
  });
  const selectedCameraPovIsGhost = $derived.by<boolean>(() => Boolean(selectedCameraPovOption?.ghost));
  const robotFollowPovEnabled = $derived.by(
    () => cameraPovSelectionId.trim() === ROBOT_FOLLOW_POV_OPTION_ID
  );
  const imuOrientationForViewer = $derived.by<SensorOrientation>(() => {
    const sample = imuRotationData;
    return {
      roll: sample?.roll ?? 0,
      pitch: sample?.pitch ?? 0,
      yaw: sample?.yaw ?? 0,
      quaternion: sample?.quaternion
        ? {
            w: sample.quaternion.w,
            x: sample.quaternion.x,
            y: sample.quaternion.y,
            z: sample.quaternion.z
          }
        : null
    };
  });
  let posesCollapsed = $state(true);
  const IMU_WINDOW_MIN_WIDTH_PX = 280;
  const IMU_WINDOW_MIN_HEIGHT_PX = 240;
  const IMU_WINDOW_DEFAULT_WIDTH_PX = 368;
  const IMU_WINDOW_DEFAULT_HEIGHT_PX = 416;
  const IMU_WINDOW_MARGIN_PX = 16;
  let workspaceRootEl = $state<HTMLDivElement | null>(null);
  let imuWindowEl = $state<HTMLDivElement | null>(null);
  let imuWindowWidth = $state(IMU_WINDOW_DEFAULT_WIDTH_PX);
  let imuWindowHeight = $state(IMU_WINDOW_DEFAULT_HEIGHT_PX);
  let imuWindowX = $state<number | null>(null);
  let imuWindowY = $state<number | null>(null);
  let imuDragActive = false;
  let imuResizeActive = false;
  let imuDragOffsetX = 0;
  let imuDragOffsetY = 0;
  let imuResizePointerStartX = 0;
  let imuResizePointerStartY = 0;
  let imuResizeStartWidth = IMU_WINDOW_DEFAULT_WIDTH_PX;
  let imuResizeStartHeight = IMU_WINDOW_DEFAULT_HEIGHT_PX;
  let lastImuOverlayVisible = false;

  function clampImuWindowRect(
    nextX: number,
    nextY: number,
    nextWidth: number,
    nextHeight: number
  ): { x: number; y: number; width: number; height: number } {
    const fallbackWidth = typeof window !== 'undefined' ? window.innerWidth : 1280;
    const fallbackHeight = typeof window !== 'undefined' ? window.innerHeight : 720;
    const rootWidth = workspaceRootEl?.clientWidth ?? fallbackWidth;
    const rootHeight = workspaceRootEl?.clientHeight ?? fallbackHeight;

    const maxWidth = Math.max(IMU_WINDOW_MIN_WIDTH_PX, rootWidth - IMU_WINDOW_MARGIN_PX * 2);
    const maxHeight = Math.max(IMU_WINDOW_MIN_HEIGHT_PX, rootHeight - IMU_WINDOW_MARGIN_PX * 2);
    const width = Math.min(Math.max(nextWidth, IMU_WINDOW_MIN_WIDTH_PX), maxWidth);
    const height = Math.min(Math.max(nextHeight, IMU_WINDOW_MIN_HEIGHT_PX), maxHeight);
    const maxX = Math.max(IMU_WINDOW_MARGIN_PX, rootWidth - width - IMU_WINDOW_MARGIN_PX);
    const maxY = Math.max(IMU_WINDOW_MARGIN_PX, rootHeight - height - IMU_WINDOW_MARGIN_PX);
    const x = Math.min(Math.max(nextX, IMU_WINDOW_MARGIN_PX), maxX);
    const y = Math.min(Math.max(nextY, IMU_WINDOW_MARGIN_PX), maxY);
    return { x, y, width, height };
  }

  function applyImuWindowRect(next: { x: number; y: number; width: number; height: number }): void {
    imuWindowX = next.x;
    imuWindowY = next.y;
    imuWindowWidth = next.width;
    imuWindowHeight = next.height;
  }

  function placeImuWindowDefault(): void {
    const fallbackWidth = typeof window !== 'undefined' ? window.innerWidth : 1280;
    const fallbackHeight = typeof window !== 'undefined' ? window.innerHeight : 720;
    const rootWidth = workspaceRootEl?.clientWidth ?? fallbackWidth;
    const rootHeight = workspaceRootEl?.clientHeight ?? fallbackHeight;
    applyImuWindowRect(
      clampImuWindowRect(
        rootWidth - imuWindowWidth - IMU_WINDOW_MARGIN_PX,
        rootHeight - imuWindowHeight - IMU_WINDOW_MARGIN_PX,
        imuWindowWidth,
        imuWindowHeight
      )
    );
  }

  function ensureImuWindowVisible(): void {
    if (imuWindowX == null || imuWindowY == null) {
      placeImuWindowDefault();
      return;
    }
    applyImuWindowRect(clampImuWindowRect(imuWindowX, imuWindowY, imuWindowWidth, imuWindowHeight));
  }

  function stopImuWindowPointerTracking(): void {
    imuDragActive = false;
    imuResizeActive = false;
    if (typeof window === 'undefined') return;
    window.removeEventListener('pointermove', onImuWindowPointerMove);
    window.removeEventListener('pointerup', onImuWindowPointerEnd);
    window.removeEventListener('pointercancel', onImuWindowPointerEnd);
  }

  function startImuWindowPointerTracking(): void {
    if (typeof window === 'undefined') return;
    window.removeEventListener('pointermove', onImuWindowPointerMove);
    window.removeEventListener('pointerup', onImuWindowPointerEnd);
    window.removeEventListener('pointercancel', onImuWindowPointerEnd);
    window.addEventListener('pointermove', onImuWindowPointerMove);
    window.addEventListener('pointerup', onImuWindowPointerEnd);
    window.addEventListener('pointercancel', onImuWindowPointerEnd);
  }

  function onImuWindowPointerMove(event: PointerEvent): void {
    if (!showImuRotationOverlay) return;
    const rootRect = workspaceRootEl?.getBoundingClientRect();
    const rootLeft = rootRect?.left ?? 0;
    const rootTop = rootRect?.top ?? 0;
    if (imuDragActive) {
      applyImuWindowRect(
        clampImuWindowRect(
          event.clientX - rootLeft - imuDragOffsetX,
          event.clientY - rootTop - imuDragOffsetY,
          imuWindowWidth,
          imuWindowHeight
        )
      );
      return;
    }
    if (imuResizeActive) {
      const deltaX = event.clientX - imuResizePointerStartX;
      const deltaY = event.clientY - imuResizePointerStartY;
      applyImuWindowRect(
        clampImuWindowRect(
          imuWindowX ?? IMU_WINDOW_MARGIN_PX,
          imuWindowY ?? IMU_WINDOW_MARGIN_PX,
          imuResizeStartWidth + deltaX,
          imuResizeStartHeight + deltaY
        )
      );
    }
  }

  function onImuWindowPointerEnd(): void {
    stopImuWindowPointerTracking();
  }

  function beginImuWindowDrag(event: PointerEvent): void {
    if (event.button !== 0) return;
    ensureImuWindowVisible();
    const panelRect = imuWindowEl?.getBoundingClientRect();
    if (!panelRect) return;
    imuDragActive = true;
    imuResizeActive = false;
    imuDragOffsetX = event.clientX - panelRect.left;
    imuDragOffsetY = event.clientY - panelRect.top;
    startImuWindowPointerTracking();
    event.preventDefault();
  }

  function beginImuWindowResize(event: PointerEvent): void {
    if (event.button !== 0) return;
    ensureImuWindowVisible();
    imuResizeActive = true;
    imuDragActive = false;
    imuResizePointerStartX = event.clientX;
    imuResizePointerStartY = event.clientY;
    imuResizeStartWidth = imuWindowWidth;
    imuResizeStartHeight = imuWindowHeight;
    startImuWindowPointerTracking();
    event.preventDefault();
  }

  function onImuWindowViewportResize(): void {
    if (!showImuRotationOverlay) return;
    ensureImuWindowVisible();
  }

  function openImuRotationOverlay(): void {
    showImuRotationOverlay = true;
    queueMicrotask(() => {
      ensureImuWindowVisible();
    });
  }

  function imuWindowStyle(): string {
    if (imuWindowX == null || imuWindowY == null) {
      return `right:${IMU_WINDOW_MARGIN_PX}px;bottom:${IMU_WINDOW_MARGIN_PX}px;width:${imuWindowWidth}px;height:${imuWindowHeight}px;`;
    }
    return `left:${imuWindowX}px;top:${imuWindowY}px;width:${imuWindowWidth}px;height:${imuWindowHeight}px;`;
  }

  $effect(() => {
    const visible = showImuRotationOverlay;
    if (visible && !lastImuOverlayVisible) {
      queueMicrotask(() => {
        ensureImuWindowVisible();
      });
    }
    if (!visible && lastImuOverlayVisible) {
      stopImuWindowPointerTracking();
    }
    lastImuOverlayVisible = visible;
  });

  $effect(() => {
    return () => {
      stopImuWindowPointerTracking();
    };
  });

  function compactPoseValues(values: string): string {
    if (values.includes('id:') || values.includes('d:')) {
      return values.trim();
    }
    const tokens = values.match(/—|[-+]?\d+(?:\.\d+)?/g) ?? [];
    if (tokens.length < 6) return values.trim();
    const pos = tokens.slice(0, 3).join(', ');
    const rot = tokens.slice(3, 6).join(', ');
    return `(${pos}) (${rot})`;
  }

</script>

<svelte:window onresize={onImuWindowViewportResize} />

<div class="relative flex-1 min-h-0 overflow-hidden" bind:this={workspaceRootEl}>
  <LocalizationWorkspaceViewer
    {ViewersComponent}
    {markers}
    {tagLineMarkers}
    {referenceMarkers}
    {mode}
    {bumperNumber}
    {bumperColor}
    {robotOverlays}
    {robot}
    {cameras}
    {cameraTransforms}
    {robotTransform}
    {sceneTransform}
    {customField}
    {showRobot}
    {showCameras}
    {cameraGhostActive}
    bind:showOriginAxes
    bind:showTagLines
    bind:showFieldImage
    bind:showMinimapTrail
    {cameraHighlightColor}
    {minimapPoseDot}
    {cameraPovEnabled}
    {cameraPovTransform}
    {cameraPovIntrinsics}
    {cameraPovApplyFov}
    {cameraPovForwardSign}
    {robotFollowPovEnabled}
    bind:showMetricsOverlay
    {feedStatus}
    {selectedSourceCount}
    {liveMarkerCount}
    {lastPollMs}
    {activeSolveMs}
    bind:pollHz
    {pollHzMin}
    {pollHzMax}
    {pollHzStep}
    {feedMessage}
  />

  <div class="absolute left-4 top-4 z-40 flex max-w-[92vw] flex-col gap-2">
    <div class="grid gap-3 rounded border border-surface-800 bg-surface-950/70 p-3 text-xs text-surface-200 shadow-xl backdrop-blur">
      <div class="grid gap-2">
        <div class="flex items-center justify-between">
          <p class="text-micro-tight uppercase tracking-[0.35em] text-surface-500">Target space</p>
          {#if !fieldSpaceAllowed && selectedFieldMapId}
            <span class="text-micro-tight text-amber-300">uncalibrated</span>
          {/if}
        </div>
        <select
          class="min-w-[12rem] rounded border border-surface-800 bg-surface-950/70 px-3 py-2 text-xs uppercase tracking-[0.3em] text-surface-200 focus:border-primary-400 focus:outline-none"
          bind:value={coordinateSpace}
        >
          {#each availableCoordinateSpaces as space (space)}
            <option value={space}>
              {poseSpaceLabel(space)}
            </option>
          {/each}
        </select>

      </div>

      <div class="flex items-center gap-3 border-t border-surface-800/70 pt-3">
        <p class="shrink-0 text-micro-tight uppercase tracking-[0.35em] text-surface-500">POV</p>
        <select
          class={`min-w-0 flex-1 rounded border border-surface-800 bg-surface-950/70 px-3 py-2 text-xs uppercase tracking-[0.3em] focus:border-primary-400 focus:outline-none ${
            selectedCameraPovIsGhost ? 'text-surface-400' : 'text-surface-200'
          }`}
          bind:value={cameraPovSelectionId}
        >
          <option value="">off</option>
          <option value={ROBOT_FOLLOW_POV_OPTION_ID}>robot</option>
          {#each groupedCameraPovOptions as group (group.label)}
            <optgroup label={group.label}>
              {#each group.subgroups as subgroup (subgroup.label)}
                <option value={`__header:${group.label}:${subgroup.label}`} disabled>{`- ${subgroup.label}`}</option>
                {#each subgroup.options as option (option.id)}
                  <option
                    value={option.id}
                    disabled={!option.available}
                    style={option.ghost ? 'opacity:0.62; text-decoration: line-through;' : undefined}
                  >
                    {option.ghost
                      ? `${option.label} (ghost)`
                      : option.available
                        ? option.label
                        : `${option.label} (no pose)`}
                  </option>
                {/each}
              {/each}
            </optgroup>
          {/each}
        </select>
      </div>

      <div class="flex items-center gap-3 border-t border-surface-800/70 pt-3">
        <p class="shrink-0 text-micro-tight uppercase tracking-[0.35em] text-surface-500">POV FOV</p>
        <select
          class="min-w-0 flex-1 rounded border border-surface-800 bg-surface-950/70 px-3 py-2 text-xs uppercase tracking-[0.3em] text-surface-200 focus:border-primary-400 focus:outline-none"
          bind:value={cameraPovFovMode}
        >
          <option value="undistorted">calibration undistorted</option>
          <option value="raw">raw distorted</option>
          <option value="none">none</option>
        </select>
      </div>

      {#if mode === 'frc-field' && baseFrame === 'field'}
        <div class="grid gap-2">
          <p class="text-micro-tight uppercase tracking-[0.35em] text-surface-500">Origin</p>
          <select
            class="min-w-[12rem] rounded border border-surface-800 bg-surface-950/70 px-3 py-2 text-xs uppercase tracking-[0.3em] text-surface-200 focus:border-primary-400 focus:outline-none"
            value={fieldOriginMode}
            onchange={handleFieldOriginModeChange}
          >
            <option value="blue">wpiblue</option>
            <option value="red">wpired</option>
            <option value="center">center</option>
            <option value="custom">custom</option>
          </select>
        </div>
      {/if}

    </div>

    {#if targetSpaceOverlay && !showOutputsOverlay}
      <div class="grid gap-2 rounded border border-surface-800 bg-surface-950/70 p-3 text-xs text-surface-200 shadow-xl backdrop-blur">
        <button
          type="button"
          class="flex items-center justify-between gap-3 rounded border border-surface-800/70 bg-surface-950/40 px-2 py-1.5 text-left"
          onclick={() => {
            posesCollapsed = !posesCollapsed;
          }}
        >
          <span class="text-micro-tight uppercase tracking-[0.3em] text-surface-400">
            poses ({targetSpaceOverlay.rows.length})
          </span>
          <span class="text-[0.7rem] text-surface-500">{posesCollapsed ? '+' : '−'}</span>
        </button>
        {#if !posesCollapsed}
          <p class="text-micro-tight text-surface-500">{targetSpaceOverlay.header}</p>
          <div class="max-h-28 space-y-1 overflow-y-auto pr-1">
            {#each targetSpaceOverlay.rows as row (row.key)}
              <div class={`grid grid-cols-[0.6rem_minmax(0,1fr)] items-center gap-2 text-[0.65rem] leading-4 ${row.stale ? 'opacity-45 text-surface-400' : 'opacity-100 text-surface-200'}`}>
                <div class="h-2 w-2 rounded-full" style={`background:${row.color}; opacity:${row.stale ? 0.5 : 1};`}></div>
                <div class={`truncate font-mono tabular-nums ${row.stale ? 'text-surface-400' : 'text-surface-100'}`}>
                  {compactPoseValues(row.values)}
                </div>
              </div>
            {/each}
          </div>
        {/if}
      </div>
    {/if}

    <!-- Anchor/tag-frame viewer controls removed for now (camera_in_tag/robot_in_tag are not exposed). -->
  </div>

  <LocalizationWorkspacePanels
    bind:showImuRotationOverlay
    {openImuRotationOverlay}
    bind:imuWindowEl
    {imuWindowStyle}
    {ImuOrientationViewerComponent}
    {imuOrientationForViewer}
    {imuRotationData}
    {imuRotationStatusMessage}
    {beginImuWindowDrag}
    {beginImuWindowResize}
    bind:showOutputsOverlay
    {activeProfileId}
    {localizationConfigLoading}
    bind:profileNameInput
    {onCommitProfileName}
    {activeSolverId}
    {solvers}
    {onSetActiveSolverId}
    {onAddSolver}
    {onRemoveActiveSolver}
    bind:solverNameInput
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
    bind:tagSizeInput
    {tagSizeError}
    {onCommitTagSize}
    bind:excludedTagIdsInput
    {excludedTagIdsError}
    {onCommitExcludedTagIds}
    bind:fieldOriginMode
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
    bind:fieldMapSelection
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
    bind:showMetricsOverlay
    {feedStatus}
    {pollHz}
    {lastPollMs}
    {activeSolveMs}
    {profileTimingRows}
    bind:showCameraPoseOverlay
    {primaryCameraKey}
    bind:cameraPoseXInput
    bind:cameraPoseYInput
    bind:cameraPoseZInput
    bind:cameraPosePitchDeg
    bind:cameraPoseYawDeg
    bind:cameraPoseRollDeg
    {cameraPoseEditorError}
    {onResetPrimaryCameraPoseInputs}
    {onApplyPrimaryCameraPose}
    bind:showCustomFieldsOverlay
    bind:newCustomFieldName
    bind:newCustomFieldWidth
    bind:newCustomFieldDepth
    {newCustomFieldError}
    {onCreateCustomField}
    bind:newOriginName
    bind:newOriginX
    bind:newOriginZ
    bind:newOriginYaw
    {newOriginError}
    {onAddOrigin}
    {selectedCustomField}
    {activeFieldMapBitsStatus}
    {onRefreshMaps}
    {mapUploadFile}
    bind:mapAssignId
    {onAssignMap}
    {fieldMapDocErrors}
    {hasActiveProfile}
    {onSetMapUploadFile}
    {onUploadSelectedMapFile}
  />
</div>
