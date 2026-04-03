<script lang="ts">
  import { onDestroy } from 'svelte';
  import { listMediaAssets } from '$lib/features/media/api';
  import type { MediaAsset } from '$lib/features/media/api';
  import { subscribeMediaMutations } from '$lib/features/media/mutations';
  import { MEDIA_KIND_OPTIONS, mediaKindLabel } from '$lib/features/media/mediaKind';
  import { OpenAPI } from '$lib/ts-bindings/http/client';
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
  import RangeBandSlider from '$lib/components/controls/RangeBandSlider.svelte';
  import StreamConfigPanel from './stream/StreamConfigPanel.svelte';
  import StreamManifestDetails from './stream/StreamManifestDetails.svelte';
  import StreamControls from './stream/StreamControls.svelte';
  import StreamPreviewPanel from './stream/StreamPreviewPanel.svelte';
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

  function apiPath(path: string): string {
    const base = String(OpenAPI.BASE ?? '').replace(/\/+$/, '');
    const normalized = path.startsWith('/') ? path : `/${path}`;
    return `${base}${normalized}`;
  }

  const selectedMediaNames = $derived((() => {
    const names = new SvelteSet<string>();
    const lines = String(fileBackendPathsText ?? '').split('\n');
    for (const line of lines) {
      const name = normalizeMediaName(line);
      if (name) names.add(name);
    }
    return Array.from(names);
  })());

  function normalizeMediaName(value: string): string {
    const trimmed = String(value ?? '').trim();
    if (!trimmed) return '';
    const parts = trimmed.split('/');
    return parts[parts.length - 1] ?? '';
  }

  function toMediaPath(name: string): string {
    return `${MEDIA_ROOT}/${name}`;
  }

  function hydrateMediaPickerSelection(): void {
    const next: Record<string, boolean> = {};
    selectedMediaNames.forEach((name) => {
      next[name] = true;
    });
    mediaPickerSelected = next;
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
    const key = normalizeMediaName(name);
    if (!key) return;
    const next = { ...mediaPickerSelected };
    if (next[key]) delete next[key];
    else next[key] = true;
    mediaPickerSelected = next;
    mediaPickerAnchorName = key;
  }

  function setMediaPickerSelectionOnly(name: string): void {
    const key = normalizeMediaName(name);
    if (!key) return;
    mediaPickerSelected = { [key]: true };
    mediaPickerAnchorName = key;
  }

  function addMediaPickerSelection(name: string): void {
    const key = normalizeMediaName(name);
    if (!key) return;
    if (mediaPickerSelected[key]) {
      mediaPickerAnchorName = key;
      return;
    }
    mediaPickerSelected = { ...mediaPickerSelected, [key]: true };
    mediaPickerAnchorName = key;
  }

  function rangeSelectMediaPicker(name: string, mode: 'replace' | 'add'): void {
    const key = normalizeMediaName(name);
    if (!key) return;
    const anchor = mediaPickerAnchorName;
    const list = mediaPickerAssets ?? [];
    const toIndex = list.findIndex((asset) => normalizeMediaName(asset.name) === key);
    const anchorIndex = anchor ? list.findIndex((asset) => normalizeMediaName(asset.name) === anchor) : -1;
    if (toIndex < 0 || anchorIndex < 0) {
      if (mode === 'replace') setMediaPickerSelectionOnly(key);
      else addMediaPickerSelection(key);
      return;
    }
    const start = Math.min(toIndex, anchorIndex);
    const end = Math.max(toIndex, anchorIndex);
    const next = mode === 'replace' ? {} : { ...mediaPickerSelected };
    for (let i = start; i <= end; i += 1) {
      const id = normalizeMediaName(list[i]?.name ?? '');
      if (id) next[id] = true;
    }
    mediaPickerSelected = next;
    mediaPickerAnchorName = key;
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
    const names = Object.keys(mediaPickerSelected).filter((key) => mediaPickerSelected[key]);
    fileBackendPathsText = names.map((name) => toMediaPath(name)).join('\n');
    closeMediaPicker();
  }

  function clearMediaPickerSelection(): void {
    mediaPickerSelected = {};
    mediaPickerAnchorName = null;
  }

  function selectAllMediaPickerVisible(): void {
    const next = { ...mediaPickerSelected };
    for (const asset of mediaPickerAssets) {
      const key = normalizeMediaName(asset.name);
      if (key) next[key] = true;
    }
    mediaPickerSelected = next;
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
    if (option === 'all') {
      updateMediaPickerKind(option);
      return;
    }
    if (!MEDIA_KIND_OPTIONS.includes(option as MediaAsset['kind'])) return;
    updateMediaPickerKind(option as MediaAsset['kind']);
  }

  function mediaPickerKindLabel(option: string): string {
    if (option === 'all') return 'All';
    return mediaKindLabel(option as MediaAsset['kind']);
  }

  function updateMediaPickerSort(value: 'name' | 'recent'): void {
    mediaPickerSort = value;
    void loadMediaPickerAssets();
  }

  function handleMediaPickerSortChange(event: Event): void {
    const target = event.currentTarget;
    if (!(target instanceof HTMLSelectElement)) return;
    if (target.value !== 'name' && target.value !== 'recent') return;
    updateMediaPickerSort(target.value);
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
    <details class="rounded border border-surface-800 bg-surface-950/40 p-4" open>
      <summary class="cursor-pointer select-none text-sm text-surface-300">Crop</summary>
      <div class="mt-3 space-y-3">
        <p class="text-micro-tight text-surface-500">Normalized stream crop bounds (-1 to 1)</p>
        <div class="flex flex-wrap items-center justify-between gap-3">
          <label class="flex items-center gap-2 text-micro-tight text-surface-400">
            <input
              type="checkbox"
              checked={Boolean(streamCropGuidesEnabled)}
              onchange={(event) => (streamCropGuidesEnabled = event.currentTarget.checked)}
            />
            Show crop guides in preview
          </label>
          <div class="flex items-center gap-2">
            {#if streamCropApplying}
              <span class="text-micro-tight text-surface-500">Applying…</span>
            {/if}
            <button class="btn btn-3xs preset-tonal uppercase tracking-[0.3em]" type="button" onclick={resetStreamCrop}>
              Full frame
            </button>
          </div>
        </div>

        <div class="space-y-4">
          <div class="space-y-2">
            <div class="flex items-center justify-between text-micro-tight uppercase tracking-[0.22em] text-surface-500">
              <span>Horizontal (x)</span>
              <span>{formatCropValue(normalizedStreamCrop[0])} to {formatCropValue(normalizedStreamCrop[1])}</span>
            </div>
            <div class="flex items-center gap-2">
              <input
                class="input h-9 w-24 text-xs"
                type="number"
                min={STREAM_CROP_MIN}
                max={STREAM_CROP_MAX}
                step={STREAM_CROP_STEP}
                value={normalizedStreamCrop[0]}
                oninput={(event) => updateStreamCropRange('x', 'min', Number(event.currentTarget.value))}
                onchange={(event) => updateStreamCropRange('x', 'min', Number(event.currentTarget.value), true)}
              />
              <div class="flex-1">
                <RangeBandSlider
                  min={STREAM_CROP_MIN}
                  max={STREAM_CROP_MAX}
                  step={STREAM_CROP_STEP}
                  valueMin={normalizedStreamCrop[0]}
                  valueMax={normalizedStreamCrop[1]}
                  gradient="var(--color-primary-500)"
                  on:change={(event) => updateStreamCropBand('x', Number(event.detail.min), Number(event.detail.max))}
                />
              </div>
              <input
                class="input h-9 w-24 text-xs"
                type="number"
                min={STREAM_CROP_MIN}
                max={STREAM_CROP_MAX}
                step={STREAM_CROP_STEP}
                value={normalizedStreamCrop[1]}
                oninput={(event) => updateStreamCropRange('x', 'max', Number(event.currentTarget.value))}
                onchange={(event) => updateStreamCropRange('x', 'max', Number(event.currentTarget.value), true)}
              />
            </div>
          </div>

          <div class="space-y-2">
            <div class="flex items-center justify-between text-micro-tight uppercase tracking-[0.22em] text-surface-500">
              <span>Vertical (y)</span>
              <span>{formatCropValue(normalizedStreamCrop[2])} to {formatCropValue(normalizedStreamCrop[3])}</span>
            </div>
            <div class="flex items-center gap-2">
              <input
                class="input h-9 w-24 text-xs"
                type="number"
                min={STREAM_CROP_MIN}
                max={STREAM_CROP_MAX}
                step={STREAM_CROP_STEP}
                value={normalizedStreamCrop[2]}
                oninput={(event) => updateStreamCropRange('y', 'min', Number(event.currentTarget.value))}
                onchange={(event) => updateStreamCropRange('y', 'min', Number(event.currentTarget.value), true)}
              />
              <div class="flex-1">
                <RangeBandSlider
                  min={STREAM_CROP_MIN}
                  max={STREAM_CROP_MAX}
                  step={STREAM_CROP_STEP}
                  valueMin={normalizedStreamCrop[2]}
                  valueMax={normalizedStreamCrop[3]}
                  gradient="var(--color-primary-500)"
                  on:change={(event) => updateStreamCropBand('y', Number(event.detail.min), Number(event.detail.max))}
                />
              </div>
              <input
                class="input h-9 w-24 text-xs"
                type="number"
                min={STREAM_CROP_MIN}
                max={STREAM_CROP_MAX}
                step={STREAM_CROP_STEP}
                value={normalizedStreamCrop[3]}
                oninput={(event) => updateStreamCropRange('y', 'max', Number(event.currentTarget.value))}
                onchange={(event) => updateStreamCropRange('y', 'max', Number(event.currentTarget.value), true)}
              />
            </div>
          </div>
        </div>
        {#if streamCropError}
          <p class="text-xs text-error-300">{streamCropError}</p>
        {/if}
        {#if streamCropWarning}
          <p class="text-xs text-amber-300">{streamCropWarning}</p>
        {/if}
      </div>
    </details>

    <details class="rounded border border-surface-800 bg-surface-950/40 p-4">
      <summary class="cursor-pointer select-none text-sm text-surface-300">Crosshair</summary>
      <div class="mt-3 space-y-3">
        <p class="text-micro-tight text-surface-500">Normalized crosshair position (-1 to 1)</p>
        <p class="text-micro-tight text-surface-500">Crosshair is rendered by the active pipeline graph on the stream output frame.</p>
        <div class="flex flex-wrap items-center justify-between gap-3">
          <label class="flex items-center gap-2 text-micro-tight text-surface-400">
            <input
              type="checkbox"
              checked={Boolean(streamCrosshairEnabled)}
              onchange={(event) => updateStreamCrosshairEnabled(event.currentTarget.checked)}
            />
            Draw crosshair in output
          </label>
          <div class="flex items-center gap-2">
            {#if streamCrosshairApplying}
              <span class="text-micro-tight text-surface-500">Applying…</span>
            {/if}
            <button class="btn btn-3xs preset-tonal uppercase tracking-[0.3em]" type="button" onclick={resetStreamCrosshair}>
              Center
            </button>
          </div>
        </div>
        <div class="space-y-4">
          <div class="space-y-2">
            <div class="flex items-center justify-between text-micro-tight uppercase tracking-[0.22em] text-surface-500">
              <span>Horizontal (x)</span>
              <span>{formatCropValue(normalizedStreamCrosshair[0])}</span>
            </div>
            <div class="flex items-center gap-2">
              <div class="flex-1">
                <input
                  class="h-2 w-full cursor-pointer accent-primary-500"
                  type="range"
                  min={STREAM_CROP_MIN}
                  max={STREAM_CROP_MAX}
                  step={STREAM_CROP_STEP}
                  value={normalizedStreamCrosshair[0]}
                  oninput={(event) => updateStreamCrosshair('x', Number(event.currentTarget.value))}
                  onchange={(event) => updateStreamCrosshair('x', Number(event.currentTarget.value), true)}
                />
              </div>
              <input
                class="input h-9 w-24 text-xs"
                type="number"
                min={STREAM_CROP_MIN}
                max={STREAM_CROP_MAX}
                step={STREAM_CROP_STEP}
                value={normalizedStreamCrosshair[0]}
                oninput={(event) => updateStreamCrosshair('x', Number(event.currentTarget.value))}
                onchange={(event) => updateStreamCrosshair('x', Number(event.currentTarget.value), true)}
              />
            </div>
          </div>

          <div class="space-y-2">
            <div class="flex items-center justify-between text-micro-tight uppercase tracking-[0.22em] text-surface-500">
              <span>Vertical (y)</span>
              <span>{formatCropValue(normalizedStreamCrosshair[1])}</span>
            </div>
            <div class="flex items-center gap-2">
              <div class="flex-1">
                <input
                  class="h-2 w-full cursor-pointer accent-primary-500"
                  type="range"
                  min={STREAM_CROP_MIN}
                  max={STREAM_CROP_MAX}
                  step={STREAM_CROP_STEP}
                  value={normalizedStreamCrosshair[1]}
                  oninput={(event) => updateStreamCrosshair('y', Number(event.currentTarget.value))}
                  onchange={(event) => updateStreamCrosshair('y', Number(event.currentTarget.value), true)}
                />
              </div>
              <input
                class="input h-9 w-24 text-xs"
                type="number"
                min={STREAM_CROP_MIN}
                max={STREAM_CROP_MAX}
                step={STREAM_CROP_STEP}
                value={normalizedStreamCrosshair[1]}
                oninput={(event) => updateStreamCrosshair('y', Number(event.currentTarget.value))}
                onchange={(event) => updateStreamCrosshair('y', Number(event.currentTarget.value), true)}
              />
            </div>
          </div>
        </div>

        {#if streamCrosshairError}
          <p class="text-xs text-error-300">{streamCrosshairError}</p>
        {/if}
        {#if streamCrosshairWarning}
          <p class="text-xs text-amber-300">{streamCrosshairWarning}</p>
        {/if}
      </div>
    </details>

    <details class="rounded border border-surface-800 bg-surface-950/40 p-4" open>
      <summary class="cursor-pointer select-none text-sm text-surface-300">Ordering</summary>
      <div class="mt-3 space-y-3">
        <p class="text-micro-tight text-surface-500">Order detections before downstream targeting/filtering.</p>
        <div class="flex items-center justify-between gap-2">
          <span class="text-micro-tight uppercase tracking-[0.22em] text-surface-500">Mode</span>
          {#if streamOrderingApplying}
            <span class="text-micro-tight text-surface-500">Applying…</span>
          {/if}
        </div>
        <select
          class="input h-10 w-full"
          value={normalizedStreamOrderingMode}
          onchange={(event) => updateStreamOrderingMode(event.currentTarget.value, true)}
        >
          {#each STREAM_ORDERING_MODES as option (option.value)}
            <option value={option.value}>{option.label}</option>
          {/each}
        </select>
        {#if streamOrderingError}
          <p class="text-xs text-error-300">{streamOrderingError}</p>
        {/if}
        {#if streamOrderingWarning}
          <p class="text-xs text-amber-300">{streamOrderingWarning}</p>
        {/if}
      </div>
    </details>
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

{#if encoderSettingsOpen}
  <div class="fixed inset-0 z-40 flex items-center justify-center bg-black/70 px-4 py-6" role="dialog" aria-modal="true" aria-label="Encoder settings">
    <div class="w-full max-w-2xl rounded-lg border border-surface-800 bg-surface-950 p-5 shadow-2xl">
      <div class="flex flex-wrap items-start justify-between gap-3">
        <div>
          <p class="text-2xs uppercase tracking-[0.3em] text-surface-500">Encoder settings</p>
          <p class="mt-1 text-sm text-surface-300">{selectedEncoder?.name ?? 'Encoder'}</p>
          <p class="text-xs text-surface-500">{encoderImpl ?? ''}</p>
        </div>
        <button class="btn btn-3xs preset-tonal uppercase tracking-[0.3em]" type="button" onclick={() => (encoderSettingsOpen = false)}>
          Close
        </button>
      </div>

      {#if selectedEncoderDefaultsSummary}
        <p class="mt-2 text-micro text-surface-500">{selectedEncoderDefaultsSummary}</p>
      {/if}

      <div class="mt-4 space-y-3">
        {#if encoderSettingsSupportsQuality(selectedEncoderSettingsKind)}
          <label class="text-sm">
            <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Quality</span>
            <input
              class="mt-1 w-full border border-surface-700 bg-surface-900/70 px-3 py-2"
              type="number"
              min="1"
              max="100"
              step="1"
              value={encoderSettings.quality ?? ''}
              oninput={(e) => (encoderSettings.quality = Number(e.currentTarget.value) || null)}
              placeholder="Auto"
            />
          </label>
        {:else if encoderSettingsSupportsVideoControls(selectedEncoderSettingsKind)}
          <div class="grid gap-3 md:grid-cols-3">
            <label class="text-sm">
              <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Bitrate (bps)</span>
              <input
                class="mt-1 w-full border border-surface-700 bg-surface-900/70 px-3 py-2"
                type="number"
                min="0"
                step="100000"
                value={encoderSettings.bitrate ?? ''}
                oninput={(e) => (encoderSettings.bitrate = Number(e.currentTarget.value) || null)}
                placeholder="4000000"
              />
            </label>
            <label class="text-sm">
              <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">GOP</span>
              <input
                class="mt-1 w-full border border-surface-700 bg-surface-900/70 px-3 py-2"
                type="number"
                min="0"
                step="1"
                value={encoderSettings.gop ?? ''}
                oninput={(e) => (encoderSettings.gop = Number(e.currentTarget.value) || null)}
                placeholder="Auto"
              />
            </label>
            <label class="text-sm">
              <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Threads</span>
              <input
                class="mt-1 w-full border border-surface-700 bg-surface-900/70 px-3 py-2"
                type="number"
                min="0"
                step="1"
                value={encoderSettings.threadCount ?? ''}
                oninput={(e) => (encoderSettings.threadCount = Number(e.currentTarget.value) || null)}
                placeholder="Auto"
              />
            </label>
          </div>
          <div class="space-y-2">
            <div class="flex flex-wrap items-center gap-2">
              <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Output scale</span>
              <button class="btn btn-3xs preset-tonal uppercase tracking-[0.3em]" type="button" onclick={() => applyOutputScale(1)}>1x</button>
              <button class="btn btn-3xs preset-tonal uppercase tracking-[0.3em]" type="button" onclick={() => applyOutputScale(2)}>2x</button>
              <button class="btn btn-3xs preset-tonal uppercase tracking-[0.3em]" type="button" onclick={() => applyOutputScale(3)}>3x</button>
              <button class="btn btn-3xs preset-tonal uppercase tracking-[0.3em]" type="button" onclick={() => applyOutputScale(4)}>4x</button>
            </div>
            <p class="text-micro text-surface-500">
              {#if parseSelectedResolution()}
                Source {parseSelectedResolution()?.width}x{parseSelectedResolution()?.height} · 2x-4x is recommended for higher encode FPS.
              {:else}
                Select a stream resolution to enable scale presets.
              {/if}
            </p>
          </div>

          <div class="grid gap-3 md:grid-cols-2">
            <label class="text-sm">
              <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Output width</span>
              <input
                class="mt-1 w-full border border-surface-700 bg-surface-900/70 px-3 py-2"
                type="number"
                min="0"
                step="1"
                value={encoderSettings.outWidth ?? ''}
                oninput={(e) => (encoderSettings.outWidth = Number(e.currentTarget.value) || null)}
                placeholder="Match source"
              />
            </label>
            <label class="text-sm">
              <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Output height</span>
              <input
                class="mt-1 w-full border border-surface-700 bg-surface-900/70 px-3 py-2"
                type="number"
                min="0"
                step="1"
                value={encoderSettings.outHeight ?? ''}
                oninput={(e) => (encoderSettings.outHeight = Number(e.currentTarget.value) || null)}
                placeholder="Match source"
              />
            </label>
          </div>
        {/if}
      </div>
    </div>
  </div>
{/if}

{#if mediaPickerOpen}
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-surface-950/70 px-4"
    role="dialog"
    aria-modal="true"
    onclick={(event) => {
      if (event.target === event.currentTarget) closeMediaPicker();
    }}
    onkeydown={(event) => {
      if (event.key === 'Escape') closeMediaPicker();
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'a') {
        event.preventDefault();
        selectAllMediaPickerVisible();
      }
    }}
    tabindex="-1"
  >
    <div class="w-full max-w-5xl rounded border border-surface-800/70 bg-surface-950/95 p-6 text-sm text-surface-400 shadow-2xl max-h-[90vh] max-h-[90svh] max-h-[90dvh] overflow-y-auto">
      <div class="flex flex-wrap items-center justify-between gap-4">
        <div>
          <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Media library</p>
          <p class="text-sm text-surface-300">{mediaPickerSelectedCount} selected</p>
        </div>
        <div class="flex gap-2">
          <button class="btn btn-2xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={closeMediaPicker}>
            Cancel
          </button>
          <button class="btn btn-2xs preset-filled-primary-500 uppercase tracking-[0.3em]" type="button" onclick={applyMediaPickerSelection}>
            Apply selection
          </button>
        </div>
      </div>

      <div class="mt-4 flex flex-wrap gap-3">
        <input
          class="input flex-1 min-w-[220px]"
          type="search"
          placeholder="Search media"
          value={mediaPickerQuery}
          oninput={(e) => updateMediaPickerQuery(e.currentTarget.value)}
        />
        <button class="btn btn-2xs preset-tonal uppercase tracking-[0.3em]" type="button" onclick={loadMediaPickerAssets}>
          Refresh
        </button>
        <select
          class="input w-40"
          value={mediaPickerSort}
          onchange={handleMediaPickerSortChange}
        >
          <option value="name">Sort: Name</option>
          <option value="recent">Sort: Recent</option>
        </select>
        <button class="btn btn-2xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={clearMediaPickerSelection}>
          Clear
        </button>
        <button class="btn btn-2xs preset-tonal uppercase tracking-[0.3em]" type="button" onclick={selectAllMediaPickerVisible}>
          Select all
        </button>
      </div>

      <div class="mt-3 flex flex-wrap gap-2">
        {#each ['all', ...MEDIA_KIND_OPTIONS] as option (option)}
          <button
            class={`btn btn-2xs uppercase tracking-[0.3em] ${
              mediaPickerKind === option ? 'preset-filled-primary-500' : 'preset-tonal'
            }`}
            type="button"
            onclick={() => handleMediaPickerKindClick(option)}
          >
            {mediaPickerKindLabel(option)}
          </button>
        {/each}
      </div>

      {#if mediaPickerLoading}
        <div class="mt-6 text-sm text-surface-500">Loading media…</div>
      {:else if mediaPickerError}
        <div class="mt-6 text-sm text-error-200">{mediaPickerError}</div>
      {:else if !mediaPickerAssets.length}
        <div class="mt-6 text-sm text-surface-500">No media files found.</div>
      {:else}
        <div class="mt-4 grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
          {#each mediaPickerAssets as asset (asset.id)}
            {@const isSelected = Boolean(mediaPickerSelected[asset.name])}
            <button
              class={`flex items-center gap-3 rounded border px-3 py-2 text-left transition ${
                isSelected ? 'border-primary-400 bg-primary-500/10' : 'border-surface-800/60 bg-surface-950/40 hover:border-primary-400/60'
              }`}
              type="button"
              onclick={(event) => handleMediaPickerItemClick(asset, event)}
            >
              {#if asset.previewUrl}
                <img class="h-12 w-16 rounded object-cover" src={asset.previewUrl} alt={asset.name} loading="lazy" />
              {:else}
                <div class="flex h-12 w-16 items-center justify-center rounded bg-surface-900 text-micro-tight uppercase tracking-[0.2em] text-surface-400">
                  {mediaKindLabel(asset.kind)}
                </div>
              {/if}
              <div class="min-w-0 flex-1">
                <p class="truncate text-sm text-surface-200">{asset.name}</p>
                <p class="text-xs text-surface-500">{mediaKindLabel(asset.kind)}</p>
              </div>
              <input
                type="checkbox"
                checked={isSelected}
                aria-label={isSelected ? 'Deselect media' : 'Select media'}
                onclick={(e) => e.stopPropagation()}
                onchange={() => toggleMediaPickerSelection(asset.name)}
              />
            </button>
          {/each}
        </div>
      {/if}
      <p class="mt-4 text-xs text-surface-500">Selected files map to `{MEDIA_ROOT}/&lt;name&gt;` when the stream starts.</p>
    </div>
  </div>
{/if}
