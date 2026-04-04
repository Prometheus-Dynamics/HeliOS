<script lang="ts">
  import type LocalizationViewers from '$lib/components/LocalizationViewers.svelte';
  import type { LocalizationMarker, LocalizationViewMode } from '$lib/features/localization/viewers/localizationViewerTypes';
  import type { PoseQuaternion, Vec3 } from '$lib/features/localization/poseMath';
  import type { RigCameraInfo, RobotDimensions } from '$lib/types/rig';
  import type { CameraPovFovMode, ViewProfileOverlay } from './localizationWorkspaceTypes';
  import type { CustomField } from '$lib/features/localization/types';
  import type { LocalizationFieldDefinition } from '$lib/features/localization/viewers/localizationViewerTypes';

  type Props = {
    ViewersComponent?: typeof LocalizationViewers | null;
    markers?: LocalizationMarker[];
    tagLineMarkers?: LocalizationMarker[];
    referenceMarkers?: LocalizationMarker[];
    mode?: LocalizationViewMode;
    bumperNumber?: string;
    bumperColor?: string | null;
    robotOverlays?: ViewProfileOverlay[];
    robot?: RobotDimensions;
    cameras?: RigCameraInfo[];
    cameraTransforms?: Record<string, { position: Vec3; quaternion?: PoseQuaternion }> | null;
    robotTransform?: { position: Vec3; quaternion?: PoseQuaternion } | null;
    sceneTransform?: { position: Vec3; quaternion: PoseQuaternion } | null;
    customField?: CustomField | LocalizationFieldDefinition | null;
    showRobot?: boolean;
    showCameras?: boolean;
    cameraGhostActive?: boolean;
    showOriginAxes?: boolean;
    showTagLines?: boolean;
    showFieldImage?: boolean;
    showMinimapTrail?: boolean;
    cameraHighlightColor?: string | null;
    minimapPoseDot?: { position: Vec3; color?: string | null } | null;
    cameraPovEnabled?: boolean;
    cameraPovTransform?: { position: Vec3; quaternion?: PoseQuaternion } | null;
    cameraPovIntrinsics?: { fx: number; fy: number; cx: number; cy: number; width: number; height: number } | null;
    cameraPovApplyFov?: boolean;
    cameraPovForwardSign?: 1 | -1;
    robotFollowPovEnabled?: boolean;
    showMetricsOverlay?: boolean;
    feedStatus?: string;
    selectedSourceCount?: number;
    liveMarkerCount?: number;
    lastPollMs?: number | null;
    activeSolveMs?: number | null;
    pollHz?: number;
    pollHzMin?: number;
    pollHzMax?: number;
    pollHzStep?: number;
    feedMessage?: string | null;
  };

  let {
    ViewersComponent = null,
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
    showOriginAxes = $bindable(true),
    showTagLines = $bindable(false),
    showFieldImage = $bindable(true),
    showMinimapTrail = $bindable(true),
    cameraHighlightColor = null,
    minimapPoseDot = null,
    cameraPovEnabled = false,
    cameraPovTransform = null,
    cameraPovIntrinsics = null,
    cameraPovApplyFov = true,
    cameraPovForwardSign = 1,
    robotFollowPovEnabled = false,
    showMetricsOverlay = $bindable(false),
    feedStatus = 'idle',
    selectedSourceCount = 0,
    liveMarkerCount = 0,
    lastPollMs = null,
    activeSolveMs = null,
    pollHz = $bindable(30),
    pollHzMin = 1,
    pollHzMax = 240,
    pollHzStep = 1,
    feedMessage = null
  }: Props = $props();

  function toggleMetrics() {
    showMetricsOverlay = !showMetricsOverlay;
  }
</script>

{#if ViewersComponent}
  <ViewersComponent
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
    {showOriginAxes}
    {showTagLines}
    {showFieldImage}
    {showMinimapTrail}
    {cameraHighlightColor}
    {minimapPoseDot}
    {cameraPovEnabled}
    {cameraPovTransform}
    {cameraPovIntrinsics}
    {cameraPovApplyFov}
    {cameraPovForwardSign}
    {robotFollowPovEnabled}
    metricsActive={showMetricsOverlay}
    onMetricsToggle={toggleMetrics}
  >
    {#snippet footerStatus()}
      <div class="grid grid-cols-[12ch_8ch_8ch_10ch_10ch_7ch_minmax(0,1fr)] items-center gap-x-2 text-micro-tight tracking-[0.3em] text-surface-400">
        <span class="whitespace-nowrap font-semibold uppercase text-surface-50">{feedStatus}</span>
        <span class="whitespace-nowrap text-right font-mono tabular-nums">{selectedSourceCount} src</span>
        <span class="whitespace-nowrap text-right font-mono tabular-nums">{liveMarkerCount} tags</span>
        <span class="whitespace-nowrap text-right font-mono tabular-nums">{lastPollMs != null ? `${lastPollMs.toFixed(1)}ms` : '—'}</span>
        <span class="whitespace-nowrap text-right font-mono tabular-nums">{activeSolveMs != null ? `${activeSolveMs.toFixed(1)}ms` : '—'}</span>
        <span class="whitespace-nowrap text-right font-mono tabular-nums">{pollHz}Hz</span>
        <span class={`min-w-0 truncate ${feedMessage ? 'text-error-300' : 'text-surface-500'}`}>{feedMessage ?? ''}</span>
      </div>
    {/snippet}

    {#snippet minimapControls()}
      <div class="pointer-events-auto w-full space-y-2">
        <div class="w-full rounded border border-surface-800 bg-surface-950/70 p-3 text-xs text-surface-300 shadow-xl backdrop-blur">
          <div class="flex items-center justify-between gap-3">
            <div>
              <p class="text-micro uppercase tracking-[0.35em] text-surface-500">Update rate</p>
              <span class="text-micro font-semibold text-surface-50">{pollHz} Hz</span>
            </div>
          </div>
          <input type="range" min={pollHzMin} max={pollHzMax} step={pollHzStep} class="range range-xs mt-2 w-full" bind:value={pollHz} />

          <div class="mt-3 border-t border-surface-800/70 pt-3">
            <p class="text-micro uppercase tracking-[0.35em] text-surface-500">Viewer</p>
            <div class="mt-2 flex flex-col gap-2">
              <label class="flex items-center justify-between gap-3">
                <span class="text-surface-500">Show origin axes</span>
                <input type="checkbox" bind:checked={showOriginAxes} />
              </label>
              <label class="flex items-center justify-between gap-3">
                <span class="text-surface-500">Draw tag lines</span>
                <input type="checkbox" bind:checked={showTagLines} />
              </label>
              <label class="flex items-center justify-between gap-3">
                <span class="text-surface-500">Show field image</span>
                <input type="checkbox" bind:checked={showFieldImage} />
              </label>
              <label class="flex items-center justify-between gap-3">
                <span class="text-surface-500">Show minimap trail</span>
                <input type="checkbox" bind:checked={showMinimapTrail} />
              </label>
            </div>
          </div>
        </div>
      </div>
    {/snippet}
  </ViewersComponent>
{:else}
  <div class="flex h-full w-full items-center justify-center text-sm text-surface-500">Loading viewer...</div>
{/if}
