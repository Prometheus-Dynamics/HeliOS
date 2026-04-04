<script lang="ts">
  import { onDestroy } from 'svelte';
  import { listMediaAssets } from '$lib/features/media/api';
  import type { MediaAsset } from '$lib/features/media/api';
  import { subscribeMediaMutations } from '$lib/features/media/mutations';
  import { MEDIA_KIND_OPTIONS, mediaKindLabel } from '$lib/features/media/mediaKind';
  import { apiUrl } from '$lib/api/client';
  import { reportError } from '$lib/ui/errorPolicy';
  import { apiFetchResponse } from '$lib/api/core/http';
  import {
    encoderSelectionId,
    encoderSettingsKindForCodec,
    encoderSettingsSummary,
    encoderSettingsSupportsQuality,
    encoderSettingsSupportsVideoControls
  } from '$lib/api/streamEncoderSettings';
  import { cancelDebounce, scheduleDebounce, type DebounceHandle } from '$lib/utils/debounce';
  import { toaster } from '$lib';
  import StreamConfigPanel from './stream/StreamConfigPanel.svelte';
  import StreamManifestDetails from './stream/StreamManifestDetails.svelte';
  import StreamControls from './stream/StreamControls.svelte';
  import StreamPreviewPanel from './stream/StreamPreviewPanel.svelte';
  import StreamOverlayControls from './stream/StreamOverlayControls.svelte';
  import StreamEncoderSettingsModal from './stream/StreamEncoderSettingsModal.svelte';
  import StreamMediaPickerModal from './stream/StreamMediaPickerModal.svelte';
  import {
    addMediaPickerSelectionState,
    applyMediaPickerSelectionText,
    buildMediaPickerSelection,
    normalizeMediaName,
    normalizeMediaPickerKind,
    normalizeMediaPickerSort,
    rangeSelectMediaPickerState,
    selectAllVisibleMedia,
    selectedMediaNamesFromText,
    singleMediaPickerSelection,
    toggleMediaPickerSelectionState
  } from './stream/streamMediaPickerHelpers';
  import { SvelteSet } from 'svelte/reactivity';
  import { modeKey } from './page/cameraPageHelpers';
  import {
    reduceCameraStreamEditorState,
    type CameraStreamEditorState,
    type StreamSelectionMode
  } from './page/cameraStreamEditorReducer';

  let {
    cameraAlias = $bindable(),
    selectedBackendIndex = $bindable(),
    selectedModeKey = $bindable(null as string | null),
    selectedFormat = $bindable(),
    selectedResolution = $bindable(),
    selectedIntervalIdx = $bindable(),
    libcameraTargetFps = $bindable(),
    netcamTargetFps = $bindable(),
    fileBackendFps = $bindable(),
    fileBackendLoop = $bindable(),
    fileBackendPathsText = $bindable(),
    decoderImpl = $bindable(),
    encoderImpl = $bindable(),
    decoderEnabled = $bindable(),
    encoderEnabled = $bindable(),
    decoderSelectionMode = $bindable('auto' as StreamSelectionMode),
    encoderSelectionMode = $bindable('auto' as StreamSelectionMode),
    hostBuffer = $bindable(),
    previewJpegQuality = $bindable(),
    decoderFpsLimit = $bindable(),
    decoderRotationDegrees = $bindable(),
    decoderMirrorHorizontal = $bindable(),
    shadowRecorderEnabled = $bindable(),
    encoderFpsLimit = $bindable(),
    encoderSettingsOpen = $bindable(),
    encoderSettings = $bindable(),
    streamCrop = $bindable([-1, 1, -1, 1] as [number, number, number, number]),
    streamCropGuidesEnabled = $bindable(true),
    streamCropApplying = $bindable(false),
    streamCropError = $bindable(null as string | null),
    streamCropWarning = $bindable(null as string | null),
    streamCrosshair = $bindable([0, 0] as [number, number]),
    streamCrosshairGuidesEnabled = $bindable(true),
    streamCrosshairApplying = $bindable(false),
    streamCrosshairError = $bindable(null as string | null),
    streamCrosshairWarning = $bindable(null as string | null),
    streamCrosshairEnabled = $bindable(true),
    streamOrderingMode = $bindable('none'),
    streamOrderingApplying = $bindable(false),
    streamOrderingError = $bindable(null as string | null),
    streamOrderingWarning = $bindable(null as string | null),
    applying,
    decoders,
    encoders,
    encoderSettingsAvailable,
    currentDevice,
    currentBackend,
    backendLabel,
    uniqueFormats,
    resolutionsForFormat,
    intervalsForSelection,
    firstFormat,
    firstResolution,
    syncModeSelection,
    fpsLabel,
    intervalToFps,
    applyStreamPreset,
    applyStreamCrop,
    applyStreamCrosshair,
    applyStreamOrdering,
    effectiveModes,
    resolutionKey
  } = $props();

  const MEDIA_ROOT = '/var/lib/helios/api-data/media';
  const apiPath = (path: string): string => apiUrl(path);

  let mediaPickerOpen = $state(false);
  let mediaPickerLoading = $state(false);
  let mediaPickerError = $state<string | null>(null);
  let mediaPickerQuery = $state('');
  let mediaPickerKind = $state<'all' | MediaAsset['kind']>('all');
  let mediaPickerSort = $state<'name' | 'recent'>('name');
  let mediaPickerAssets = $state<MediaAsset[]>([]);
  let mediaPickerSelected = $state<Record<string, boolean>>({});
  let mediaPickerAnchorName = $state<string | null>(null);
  let mediaPickerSearchTimer: DebounceHandle = null;
  let mediaPickerRefreshTimer: ReturnType<typeof setTimeout> | null = null;
  const mediaPickerSelectedCount = $derived(Object.keys(mediaPickerSelected).filter((key) => mediaPickerSelected[key]).length);
  let benchRunning = $state(false);
  let benchError = $state<string | null>(null);
  let benchWarnings = $state<string[]>([]);
  let benchResults = $state<BenchFormatGroup[]>([]);
  let benchSampleMs = $state(1500);
  let benchTargetFps = $state<number>(120);

  type BenchCodecStat = { implementation: string; avg_ms: number; avg_fps: number; errors?: number };
  type BenchFormatGroup = {
    format: string;
    capture_avg_fps: number;
    host_avg_fps: number;
    decoders?: BenchCodecStat[];
    encoders?: BenchCodecStat[];
  };
  type StreamModeDescriptor = { id: string } & Record<string, unknown>;
  type StreamCrop = [number, number, number, number];
  type StreamCrosshair = [number, number];
  type StreamOrderingMode =
    | 'none'
    | 'largest_to_smallest'
    | 'smallest_to_largest'
    | 'top_most'
    | 'bottom_most'
    | 'left_most'
    | 'right_most'
    | 'top_left'
    | 'top_right'
    | 'bottom_left'
    | 'bottom_right'
    | 'center_most'
    | 'crosshair';

  const STREAM_CROP_APPLY_DEBOUNCE_MS = 150;
  const STREAM_CROSSHAIR_APPLY_DEBOUNCE_MS = 150;
  const STREAM_ORDERING_APPLY_DEBOUNCE_MS = 120;
  const STREAM_CROP_MIN = -1;
  const STREAM_CROP_MAX = 1;
  const STREAM_CROP_STEP = 0.01;
  const STREAM_ORDERING_MODES: Array<{ value: StreamOrderingMode; label: string }> = [
    { value: 'none', label: 'None (input order)' },
    { value: 'largest_to_smallest', label: 'Largest to smallest' },
    { value: 'smallest_to_largest', label: 'Smallest to largest' },
    { value: 'top_most', label: 'Top most' },
    { value: 'bottom_most', label: 'Bottom most' },
    { value: 'left_most', label: 'Left most' },
    { value: 'right_most', label: 'Right most' },
    { value: 'top_left', label: 'Top left' },
    { value: 'top_right', label: 'Top right' },
    { value: 'bottom_left', label: 'Bottom left' },
    { value: 'bottom_right', label: 'Bottom right' },
    { value: 'center_most', label: 'Center most' },
    { value: 'crosshair', label: 'Crosshair nearest' }
  ];

  let streamCropApplyTimer: ReturnType<typeof setTimeout> | null = null;
  let streamCrosshairApplyTimer: ReturnType<typeof setTimeout> | null = null;
  let streamOrderingApplyTimer: ReturnType<typeof setTimeout> | null = null;

  const currentDeviceValue = $derived.by(() => currentDevice?.() ?? null);
  const currentBackendValue = $derived.by(() => currentBackend?.() ?? null);
  const backendKind = $derived.by(() => (currentBackendValue as { kind?: string } | null)?.kind ?? null);
  const backendOptions = $derived.by(() => (currentDeviceValue as { backends?: Array<{ kind?: string } | null> } | null)?.backends ?? []);
  const formatOptions = $derived.by(() => uniqueFormats());
  const resolutionOptions = $derived.by(() => resolutionsForFormat(selectedFormat));
  const intervalOptions = $derived.by(() => intervalsForSelection());
  const decoderOptions = $derived.by(() => decoders ?? []);
  const selectedEncoder = $derived.by(() => {
    const selected = String(encoderImpl ?? '').trim();
    if (!selected) return null;
    return encoders.find((c) => encoderSelectionId(c) === selected || c.implementation === selected || c.name === selected) ?? null;
  });
  const selectedEncoderSettingsKind = $derived.by(() => encoderSettingsKindForCodec(selectedEncoder));
  const selectedEncoderDefaultsSummary = $derived.by(() => encoderSettingsSummary(selectedEncoder?.tunables?.encoder_settings ?? null));
  const normalizedStreamCrop = $derived.by(() => normalizeStreamCrop(readStreamCrop()));
  const normalizedStreamCrosshair = $derived.by(() => normalizeStreamCrosshair(readStreamCrosshair()));
  const normalizedStreamOrderingMode = $derived.by(() => normalizeStreamOrderingMode(streamOrderingMode));

  const unsubscribeMediaMutations = subscribeMediaMutations(() => {
    if (!mediaPickerOpen) return;
    if (mediaPickerRefreshTimer !== null) return;
    mediaPickerRefreshTimer = setTimeout(() => {
      mediaPickerRefreshTimer = null;
      void loadMediaPickerAssets();
    }, 150);
  });

  onDestroy(() => {
    unsubscribeMediaMutations();
    if (streamCropApplyTimer !== null) {
      clearTimeout(streamCropApplyTimer);
      streamCropApplyTimer = null;
    }
    if (streamCrosshairApplyTimer !== null) {
      clearTimeout(streamCrosshairApplyTimer);
      streamCrosshairApplyTimer = null;
    }
    if (streamOrderingApplyTimer !== null) {
      clearTimeout(streamOrderingApplyTimer);
      streamOrderingApplyTimer = null;
    }
    if (mediaPickerRefreshTimer !== null) {
      clearTimeout(mediaPickerRefreshTimer);
      mediaPickerRefreshTimer = null;
    }
    mediaPickerSearchTimer = cancelDebounce(mediaPickerSearchTimer);
  });

  const selectedMediaNames = $derived(selectedMediaNamesFromText(fileBackendPathsText ?? '', new SvelteSet<string>()));

  function hydrateMediaPickerSelection(): void {
    mediaPickerSelected = buildMediaPickerSelection(selectedMediaNames);
  }

  async function loadMediaPickerAssets(): Promise<void> {
    if (mediaPickerLoading) return;
    mediaPickerLoading = true;
    mediaPickerError = null;
    try {
      const result = await listMediaAssets({
        search: mediaPickerQuery,
        sort: mediaPickerSort,
        kind: mediaPickerKind === 'all' ? undefined : mediaPickerKind,
        includeCounts: false
      });
      mediaPickerAssets = result.assets ?? [];
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to load media files.';
      mediaPickerError = message;
    } finally {
      mediaPickerLoading = false;
    }
  }

  async function openMediaPicker(): Promise<void> {
    mediaPickerOpen = true;
    hydrateMediaPickerSelection();
    await loadMediaPickerAssets();
  }

  function closeMediaPicker(): void {
    mediaPickerOpen = false;
    mediaPickerSearchTimer = cancelDebounce(mediaPickerSearchTimer);
  }

  function toggleMediaPickerSelection(name: string): void {
    const next = toggleMediaPickerSelectionState(mediaPickerSelected, name);
    if (!next) return;
    mediaPickerSelected = next.selected;
    mediaPickerAnchorName = next.anchorName;
  }

  function setMediaPickerSelectionOnly(name: string): void {
    const next = singleMediaPickerSelection(name);
    if (!next) return;
    mediaPickerSelected = next.selected;
    mediaPickerAnchorName = next.anchorName;
  }

  function addMediaPickerSelection(name: string): void {
    const next = addMediaPickerSelectionState(mediaPickerSelected, name);
    if (!next) return;
    mediaPickerSelected = next.selected;
    mediaPickerAnchorName = next.anchorName;
  }

  function rangeSelectMediaPicker(name: string, mode: 'replace' | 'add'): void {
    const next = rangeSelectMediaPickerState({
      name,
      mode,
      anchorName: mediaPickerAnchorName,
      assets: mediaPickerAssets ?? [],
      selected: mediaPickerSelected
    });
    if (!next) return;
    mediaPickerSelected = next.selected;
    mediaPickerAnchorName = next.anchorName;
  }

  function handleMediaPickerItemClick(asset: MediaAsset, event: MouseEvent): void {
    const heldShift = event.shiftKey;
    const heldCtrl = event.ctrlKey || event.metaKey;
    event.preventDefault();
    event.stopPropagation();
    if (heldShift) {
      rangeSelectMediaPicker(asset.name, heldCtrl ? 'add' : 'replace');
      return;
    }
    if (heldCtrl) {
      toggleMediaPickerSelection(asset.name);
      return;
    }
    setMediaPickerSelectionOnly(asset.name);
  }

  function applyMediaPickerSelection(): void {
    fileBackendPathsText = applyMediaPickerSelectionText(mediaPickerSelected, MEDIA_ROOT);
    closeMediaPicker();
  }

  function clearMediaPickerSelection(): void {
    mediaPickerSelected = {};
    mediaPickerAnchorName = null;
  }

  function selectAllMediaPickerVisible(): void {
    mediaPickerSelected = selectAllVisibleMedia(mediaPickerSelected, mediaPickerAssets);
  }

  function updateMediaPickerQuery(value: string): void {
    mediaPickerQuery = value;
    mediaPickerSearchTimer = scheduleDebounce(mediaPickerSearchTimer, () => {
      void loadMediaPickerAssets();
    }, 200);
  }

  function updateMediaPickerKind(value: 'all' | MediaAsset['kind']): void {
    mediaPickerKind = value;
    void loadMediaPickerAssets();
  }

  function handleMediaPickerKindClick(option: string): void {
    const normalized = normalizeMediaPickerKind(option);
    if (!normalized) return;
    updateMediaPickerKind(normalized);
  }

  function updateMediaPickerSort(value: 'name' | 'recent'): void {
    mediaPickerSort = value;
    void loadMediaPickerAssets();
  }

  function handleMediaPickerSortChange(event: Event): void {
    const target = event.currentTarget;
    if (!(target instanceof HTMLSelectElement)) return;
    const normalized = normalizeMediaPickerSort(target.value);
    if (!normalized) return;
    updateMediaPickerSort(normalized);
  }

  function handleCameraAliasInput(value: string): void {
    cameraAlias = value;
  }

  function currentEditorState(): CameraStreamEditorState {
    return {
      selectedBackendIndex,
      selectedModeKey,
      selectedFormat,
      selectedResolution,
      selectedIntervalIdx,
      shadowRecorderEnabled,
      encoderImpl,
      decoderImpl,
      encoderEnabled,
      decoderEnabled,
      encoderSelectionMode,
      decoderSelectionMode
    };
  }

  function applyEditorState(next: CameraStreamEditorState): void {
    selectedBackendIndex = next.selectedBackendIndex;
    selectedModeKey = next.selectedModeKey;
    selectedFormat = next.selectedFormat;
    selectedResolution = next.selectedResolution;
    selectedIntervalIdx = next.selectedIntervalIdx;
    shadowRecorderEnabled = next.shadowRecorderEnabled;
    encoderImpl = next.encoderImpl;
    decoderImpl = next.decoderImpl;
    encoderEnabled = next.encoderEnabled;
    decoderEnabled = next.decoderEnabled;
    encoderSelectionMode = next.encoderSelectionMode;
    decoderSelectionMode = next.decoderSelectionMode;
  }

  function applyEditorAction(
    action: Parameters<typeof reduceCameraStreamEditorState>[1]
  ): void {
    applyEditorState(
      reduceCameraStreamEditorState(currentEditorState(), action, { modeKey })
    );
  }

  function handleBackendChange(value: number): void {
    applyEditorAction({
      type: 'backend_selected',
      backendIndex: value,
      backendKind: (backendOptions[value] as { kind?: string } | null)?.kind ?? null,
      modes: effectiveModes()
    });
  }

  function handleFormatChange(value: string): void {
    applyEditorAction({
      type: 'format_selected',
      format: value,
      modes: effectiveModes()
    });
  }

  function handleResolutionChange(value: string): void {
    applyEditorAction({
      type: 'resolution_selected',
      resolution: value,
      modes: effectiveModes()
    });
  }

  function handleIntervalChange(index: number): void {
    applyEditorAction({
      type: 'interval_selected',
      intervalIndex: index
    });
  }

  function handleLibcameraFpsInput(raw: string): void {
    const parsed = raw.trim().length ? Number(raw) : null;
    libcameraTargetFps = parsed && Number.isFinite(parsed) && parsed > 0 ? parsed : null;
  }

  function handleFileBackendFpsInput(raw: string): void {
    const parsed = raw.trim().length ? Number(raw) : null;
    fileBackendFps = parsed && Number.isFinite(parsed) && parsed > 0 ? parsed : null;
  }

  function handleNetcamFpsInput(raw: string): void {
    const parsed = raw.trim().length ? Number(raw) : null;
    netcamTargetFps = parsed && Number.isFinite(parsed) && parsed > 0 ? parsed : null;
  }

  function handleFileBackendLoopChange(checked: boolean): void {
    fileBackendLoop = checked;
  }

  function handleDecoderSelect(value: string): void {
    applyEditorAction({
      type: 'decoder_selected',
      value: value || null
    });
  }

  function handleEncoderSelect(value: string): void {
    applyEditorAction({
      type: 'encoder_selected',
      value: value || null
    });
  }

  function handleDecoderFpsInput(raw: string): void {
    const parsed = raw.trim().length ? Number(raw) : null;
    decoderFpsLimit = parsed && Number.isFinite(parsed) && parsed > 0 ? parsed : null;
  }

  function handleEncoderFpsInput(raw: string): void {
    const parsed = raw.trim().length ? Number(raw) : null;
    encoderFpsLimit = parsed && Number.isFinite(parsed) && parsed > 0 ? parsed : null;
  }

  function handleRotationChange(value: number): void {
    decoderRotationDegrees = Number.isFinite(value) ? value : 0;
  }

  function handleMirrorChange(checked: boolean): void {
    decoderMirrorHorizontal = checked;
  }

  function handleOpenEncoderSettings(): void {
    encoderSettingsOpen = true;
  }

  function handleHostBufferInput(raw: string): void {
    const v = Number(raw);
    hostBuffer = Number.isFinite(v) && v > 0 ? v : 8;
  }

  function handlePreviewJpegQualityInput(raw: string): void {
    const parsed = raw.trim().length ? Number(raw) : null;
    previewJpegQuality = parsed && Number.isFinite(parsed) ? Math.min(100, Math.max(1, Math.trunc(parsed))) : 65;
  }

  function readStreamCrop(): StreamCrop {
    if (!Array.isArray(streamCrop) || streamCrop.length !== 4) return [-1, 1, -1, 1];
    return [
      Number(streamCrop[0] ?? -1),
      Number(streamCrop[1] ?? 1),
      Number(streamCrop[2] ?? -1),
      Number(streamCrop[3] ?? 1)
    ];
  }

  function clampStreamCropValue(value: number): number {
    if (!Number.isFinite(value)) return 0;
    return Math.max(STREAM_CROP_MIN, Math.min(STREAM_CROP_MAX, value));
  }

  function normalizeStreamCrop(crop: StreamCrop): StreamCrop {
    let [x0, x1, y0, y1] = crop;
    x0 = clampStreamCropValue(x0);
    x1 = clampStreamCropValue(x1);
    y0 = clampStreamCropValue(y0);
    y1 = clampStreamCropValue(y1);
    if (x1 < x0) [x0, x1] = [x1, x0];
    if (y1 < y0) [y0, y1] = [y1, y0];
    return [x0, x1, y0, y1];
  }

  function formatCropValue(value: number): string {
    return value.toFixed(2);
  }

  async function flushStreamCropApply(crop: StreamCrop): Promise<void> {
    if (typeof applyStreamCrop !== 'function') return;
    await applyStreamCrop(crop);
  }

  function scheduleStreamCropApply(immediate = false): void {
    if (streamCropApplyTimer !== null) {
      clearTimeout(streamCropApplyTimer);
      streamCropApplyTimer = null;
    }
    const crop = normalizeStreamCrop(readStreamCrop());
    if (immediate) {
      void flushStreamCropApply(crop);
      return;
    }
    streamCropApplyTimer = setTimeout(() => {
      streamCropApplyTimer = null;
      void flushStreamCropApply(crop);
    }, STREAM_CROP_APPLY_DEBOUNCE_MS);
  }

  function updateStreamCropRange(axis: 'x' | 'y', bound: 'min' | 'max', rawValue: number, immediate = false): void {
    const next = normalizeStreamCrop(readStreamCrop());
    const value = clampStreamCropValue(rawValue);
    if (axis === 'x') {
      if (bound === 'min') {
        next[0] = Math.min(value, next[1]);
        if (value > next[1]) next[1] = value;
      } else {
        next[1] = Math.max(value, next[0]);
        if (value < next[0]) next[0] = value;
      }
    } else if (bound === 'min') {
      next[2] = Math.min(value, next[3]);
      if (value > next[3]) next[3] = value;
    } else {
      next[3] = Math.max(value, next[2]);
      if (value < next[2]) next[2] = value;
    }
    streamCrop = normalizeStreamCrop(next);
    streamCropError = null;
    scheduleStreamCropApply(immediate);
  }

  function updateStreamCropBand(axis: 'x' | 'y', minRawValue: number, maxRawValue: number, immediate = false): void {
    const next = normalizeStreamCrop(readStreamCrop());
    const minValue = clampStreamCropValue(minRawValue);
    const maxValue = clampStreamCropValue(maxRawValue);
    if (axis === 'x') {
      next[0] = Math.min(minValue, maxValue);
      next[1] = Math.max(minValue, maxValue);
    } else {
      next[2] = Math.min(minValue, maxValue);
      next[3] = Math.max(minValue, maxValue);
    }
    streamCrop = normalizeStreamCrop(next);
    streamCropError = null;
    scheduleStreamCropApply(immediate);
  }

  function resetStreamCrop(): void {
    streamCrop = [-1, 1, -1, 1];
    streamCropError = null;
    scheduleStreamCropApply(true);
  }

  function readStreamCrosshair(): StreamCrosshair {
    if (!Array.isArray(streamCrosshair) || streamCrosshair.length !== 2) return [0, 0];
    return [Number(streamCrosshair[0] ?? 0), Number(streamCrosshair[1] ?? 0)];
  }

  function normalizeStreamCrosshair(crosshair: StreamCrosshair): StreamCrosshair {
    const x = clampStreamCropValue(crosshair[0]);
    const y = clampStreamCropValue(crosshair[1]);
    return [x, y];
  }

  async function flushStreamCrosshairApply(crosshair: StreamCrosshair, enabled: boolean): Promise<void> {
    if (typeof applyStreamCrosshair !== 'function') return;
    await applyStreamCrosshair(crosshair, enabled);
  }

  function scheduleStreamCrosshairApply(immediate = false): void {
    if (streamCrosshairApplyTimer !== null) {
      clearTimeout(streamCrosshairApplyTimer);
      streamCrosshairApplyTimer = null;
    }
    const crosshair = normalizeStreamCrosshair(readStreamCrosshair());
    const enabled = Boolean(streamCrosshairEnabled);
    if (immediate) {
      void flushStreamCrosshairApply(crosshair, enabled);
      return;
    }
    streamCrosshairApplyTimer = setTimeout(() => {
      streamCrosshairApplyTimer = null;
      void flushStreamCrosshairApply(crosshair, enabled);
    }, STREAM_CROSSHAIR_APPLY_DEBOUNCE_MS);
  }

  function updateStreamCrosshair(axis: 'x' | 'y', rawValue: number, immediate = false): void {
    const [currentX, currentY] = normalizeStreamCrosshair(readStreamCrosshair());
    const value = clampStreamCropValue(rawValue);
    const next: StreamCrosshair = axis === 'x' ? [value, currentY] : [currentX, value];
    streamCrosshair = normalizeStreamCrosshair(next);
    streamCrosshairError = null;
    scheduleStreamCrosshairApply(immediate);
  }

  function resetStreamCrosshair(): void {
    streamCrosshair = [0, 0];
    streamCrosshairError = null;
    scheduleStreamCrosshairApply(true);
  }

  function updateStreamCrosshairEnabled(rawValue: boolean): void {
    streamCrosshairEnabled = Boolean(rawValue);
    streamCrosshairError = null;
    scheduleStreamCrosshairApply(true);
  }

  function normalizeStreamOrderingMode(mode: unknown): StreamOrderingMode {
    const normalized = String(mode ?? 'none')
      .trim()
      .toLowerCase()
      .replaceAll('-', '_')
      .replaceAll(' ', '_');
    const allowed = STREAM_ORDERING_MODES.map((entry) => entry.value);
    return (allowed as string[]).includes(normalized) ? (normalized as StreamOrderingMode) : 'none';
  }

  async function flushStreamOrderingApply(mode: StreamOrderingMode): Promise<void> {
    if (typeof applyStreamOrdering !== 'function') return;
    await applyStreamOrdering(mode);
  }

  function scheduleStreamOrderingApply(mode: StreamOrderingMode, immediate = false): void {
    if (streamOrderingApplyTimer !== null) {
      clearTimeout(streamOrderingApplyTimer);
      streamOrderingApplyTimer = null;
    }
    const normalized = normalizeStreamOrderingMode(mode);
    if (immediate) {
      void flushStreamOrderingApply(normalized);
      return;
    }
    streamOrderingApplyTimer = setTimeout(() => {
      streamOrderingApplyTimer = null;
      void flushStreamOrderingApply(normalized);
    }, STREAM_ORDERING_APPLY_DEBOUNCE_MS);
  }

  function updateStreamOrderingMode(rawValue: string, immediate = false): void {
    const next = normalizeStreamOrderingMode(rawValue);
    streamOrderingMode = next;
    streamOrderingError = null;
    scheduleStreamOrderingApply(next, immediate);
  }

  function parseSelectedResolution(): { width: number; height: number } | null {
    const parts = String(selectedResolution ?? '').split('x');
    if (parts.length !== 2) return null;
    const width = Number(parts[0]);
    const height = Number(parts[1]);
    if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0) return null;
    return { width: Math.trunc(width), height: Math.trunc(height) };
  }

  function scaledSize(value: number, divisor: number): number {
    const scaled = Math.max(16, Math.round(value / Math.max(1, divisor)));
    return scaled % 2 === 0 ? scaled : scaled - 1;
  }

  function applyOutputScale(divisor: number): void {
    const src = parseSelectedResolution();
    if (!src) return;
    encoderSettings.outWidth = scaledSize(src.width, divisor);
    encoderSettings.outHeight = scaledSize(src.height, divisor);
  }

  async function runFormatBenchmark(): Promise<void> {
    const backend = currentBackend();
    const device = currentDevice();
    const res = parseSelectedResolution();
    if (!backend || !device || !res) {
      toaster.error({ title: 'Benchmark failed', description: 'Select a device/backend + resolution first.' });
      return;
    }
    if (benchRunning) return;
    benchRunning = true;
    benchError = null;
    benchWarnings = [];
    benchResults = [];
    try {
      const fps = Math.max(1, Math.trunc(Number(benchTargetFps) || 120));
      const sampleMs = Math.max(250, Math.trunc(Number(benchSampleMs) || 1500));
      const descriptorModes = effectiveModes() as StreamModeDescriptor[];
      const selectedModes = descriptorModes
        .filter((mode) => resolutionKey(mode) === selectedResolution)
        .map((mode) => ({ id: mode.id }));

      const payload = {
        backend: backend.kind,
        handle: backend.handle,
        device_keys: device.identity?.keys ?? [],
        modes: selectedModes,
        width: res.width,
        height: res.height,
        target_fps: fps,
        sample_ms: sampleMs,
        restore_existing: true,
        controls: []
      };

      const resp = await apiFetchResponse(apiPath('/streams/bench/formats'), {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify(payload)
      });
      if (!resp.ok) {
        const text = await resp.text().catch(() => '');
        throw new Error(text || `HTTP ${resp.status}`);
      }
      const json = await resp.json();
      benchWarnings = Array.isArray(json?.warnings) ? json.warnings : [];
      benchResults = Array.isArray(json?.formats) ? json.formats : [];
      benchResults = [...benchResults].sort((a, b) => Number(b.capture_avg_fps ?? 0) - Number(a.capture_avg_fps ?? 0));
    } catch (err) {
      console.error('Benchmark failed', err);
      reportError({
        title: 'Benchmark failed',
        error: err,
        fallback: 'Unable to run the benchmark right now.',
        inline: (message) => {
          benchError = message;
        }
      });
    } finally {
      benchRunning = false;
    }
  }
</script>

<div class="space-y-6">
  <div class="rounded border border-surface-800 bg-surface-900/70 p-4 space-y-4">
    <StreamConfigPanel
      cameraAlias={cameraAlias}
      selectedBackendIndex={selectedBackendIndex}
      selectedFormat={selectedFormat}
      selectedResolution={selectedResolution}
      selectedIntervalIdx={selectedIntervalIdx}
      libcameraTargetFps={libcameraTargetFps}
      fileBackendFps={fileBackendFps}
      netcamTargetFps={netcamTargetFps}
      backendKind={backendKind}
      backends={backendOptions}
      formats={formatOptions}
      resolutions={resolutionOptions}
      intervals={intervalOptions}
      backendLabel={backendLabel}
      fpsLabel={fpsLabel}
      intervalToFps={intervalToFps}
      onCameraAliasInput={handleCameraAliasInput}
      onBackendChange={handleBackendChange}
      onFormatChange={handleFormatChange}
      onResolutionChange={handleResolutionChange}
      onIntervalChange={handleIntervalChange}
      onLibcameraFpsInput={handleLibcameraFpsInput}
      onFileBackendFpsInput={handleFileBackendFpsInput}
      onNetcamFpsInput={handleNetcamFpsInput}
    />
    <StreamManifestDetails
      backendKind={backendKind}
      fileBackendLoop={fileBackendLoop}
      selectedMediaNames={selectedMediaNames}
      onFileBackendLoopChange={handleFileBackendLoopChange}
      onOpenMediaPicker={openMediaPicker}
      onClearMediaSelection={() => (fileBackendPathsText = '')}
    />
    <StreamControls
      backendKind={backendKind}
      decoders={decoderOptions}
      encoders={encoders}
      decoderEnabled={decoderEnabled}
      encoderEnabled={encoderEnabled}
      decoderImpl={decoderImpl}
      encoderImpl={encoderImpl}
      decoderFpsLimit={decoderFpsLimit}
      decoderRotationDegrees={decoderRotationDegrees}
      decoderMirrorHorizontal={decoderMirrorHorizontal}
      shadowRecorderEnabled={shadowRecorderEnabled}
      encoderFpsLimit={encoderFpsLimit}
      encoderSettingsAvailable={encoderSettingsAvailable}
      hostBuffer={hostBuffer}
      previewJpegQuality={previewJpegQuality}
      applying={applying}
      onDecoderSelect={handleDecoderSelect}
      onDecoderFpsInput={handleDecoderFpsInput}
      onRotationChange={handleRotationChange}
      onMirrorChange={handleMirrorChange}
      onShadowRecorderToggle={(next) => (shadowRecorderEnabled = next)}
      onEncoderSelect={handleEncoderSelect}
      onEncoderFpsInput={handleEncoderFpsInput}
      onOpenEncoderSettings={handleOpenEncoderSettings}
      onHostBufferInput={handleHostBufferInput}
      onPreviewJpegQualityInput={handlePreviewJpegQualityInput}
      onApplyPreset={() => applyStreamPreset()}
    />
    <StreamOverlayControls
      bind:streamCropGuidesEnabled={streamCropGuidesEnabled}
      streamCropApplying={streamCropApplying}
      streamCropError={streamCropError}
      streamCropWarning={streamCropWarning}
      normalizedStreamCrop={normalizedStreamCrop}
      formatCropValue={formatCropValue}
      updateStreamCropRange={updateStreamCropRange}
      updateStreamCropBand={updateStreamCropBand}
      resetStreamCrop={resetStreamCrop}
      streamCrosshairEnabled={streamCrosshairEnabled}
      streamCrosshairApplying={streamCrosshairApplying}
      streamCrosshairError={streamCrosshairError}
      streamCrosshairWarning={streamCrosshairWarning}
      normalizedStreamCrosshair={normalizedStreamCrosshair}
      updateStreamCrosshair={updateStreamCrosshair}
      updateStreamCrosshairEnabled={updateStreamCrosshairEnabled}
      resetStreamCrosshair={resetStreamCrosshair}
      streamOrderingApplying={streamOrderingApplying}
      streamOrderingError={streamOrderingError}
      streamOrderingWarning={streamOrderingWarning}
      normalizedStreamOrderingMode={normalizedStreamOrderingMode}
      streamOrderingModes={STREAM_ORDERING_MODES}
      updateStreamOrderingMode={updateStreamOrderingMode}
      streamCropMin={STREAM_CROP_MIN}
      streamCropMax={STREAM_CROP_MAX}
      streamCropStep={STREAM_CROP_STEP}
    />
  </div>

  <StreamPreviewPanel
    bind:benchTargetFps={benchTargetFps}
    bind:benchSampleMs={benchSampleMs}
    benchRunning={benchRunning}
    benchError={benchError}
    benchWarnings={benchWarnings}
    benchResults={benchResults}
    apiPath={apiPath}
    currentDevice={currentDeviceValue}
    currentBackend={currentBackendValue}
    onRunBenchmark={runFormatBenchmark}
  />
</div>

<StreamEncoderSettingsModal
  open={encoderSettingsOpen}
  selectedEncoder={selectedEncoder}
  encoderImpl={encoderImpl}
  encoderSettings={encoderSettings}
  selectedEncoderSettingsKind={selectedEncoderSettingsKind}
  selectedEncoderDefaultsSummary={selectedEncoderDefaultsSummary}
  parseSelectedResolution={parseSelectedResolution}
  applyOutputScale={applyOutputScale}
  onClose={() => (encoderSettingsOpen = false)}
/>

<StreamMediaPickerModal
  open={mediaPickerOpen}
  mediaRoot={MEDIA_ROOT}
  selectedCount={mediaPickerSelectedCount}
  {mediaPickerQuery}
  {mediaPickerSort}
  {mediaPickerKind}
  {mediaPickerLoading}
  {mediaPickerError}
  mediaPickerAssets={mediaPickerAssets}
  mediaPickerSelected={mediaPickerSelected}
  onClose={closeMediaPicker}
  onApplySelection={applyMediaPickerSelection}
  onRefresh={loadMediaPickerAssets}
  onClearSelection={clearMediaPickerSelection}
  onSelectAll={selectAllMediaPickerVisible}
  onQueryChange={updateMediaPickerQuery}
  onSortChange={handleMediaPickerSortChange}
  onKindClick={handleMediaPickerKindClick}
  onItemClick={handleMediaPickerItemClick}
  onToggleSelection={toggleMediaPickerSelection}
/>
