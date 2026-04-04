<script lang="ts">
  import { createEventDispatcher, onDestroy, onMount } from 'svelte';
  import { toaster } from '$lib';
  import { apiFetch } from '$lib/api/core/http';
  import { resolveStreamCreationDefaults } from '$lib/api/streamDefaults';
  import {
    createEncoderSettingsDraft,
    encoderSelectionId,
    type EncoderSettingsDraft
  } from '$lib/api/streamEncoderSettings';
  import { PeersService, PeripheralsService, apiUrl } from '$lib/api/client';
  import { connectDevicesUpdatesStream } from '$lib/api/devicesUpdates';
  import { PipelinesApi } from '$lib/api/pipelinesApi';
  import {
    invalidateOwnedStreamMutationResources,
    loadOwnedStreamCapabilities,
    loadOwnedStreams
  } from '$lib/api/streamResources';
  import { StreamsApi } from '$lib/api/streamsApi';
  import { buildErrorMessage, reportError } from '$lib/ui/errorPolicy';
  import { extractValidationReport } from '$lib/api/errors';
  import type {
    CodecInfo,
    Interval,
    Mode,
    ModeId,
    PipelineSummary,
    PipelineTemplateSummary,
    ProbedBackend,
    ProbedDevice,
    PeerInfo,
    StreamCapabilitiesResponse,
    ValidationIssue
  } from '$lib/api/client';
  import { registerCameraModal } from '$lib/stores/modals';
  import ModalShell from '$lib/components/ui/ModalShell.svelte';
  import RegisterCameraModalBody from '$lib/components/register-camera/RegisterCameraModalBody.svelte';
  import RegisterCameraModalActions from '$lib/components/register-camera/RegisterCameraModalActions.svelte';
  import RegisterCameraBenchmarkModal from '$lib/components/register-camera/RegisterCameraBenchmarkModal.svelte';
  import RegisterCameraBenchmarkResultsModal from '$lib/components/register-camera/RegisterCameraBenchmarkResultsModal.svelte';
  import RegisterCameraEncoderSettingsModal from '$lib/components/register-camera/RegisterCameraEncoderSettingsModal.svelte';
  import RegisterCameraModalHeader from '$lib/components/register-camera/RegisterCameraModalHeader.svelte';
  import {
    filteredSensorBenchmarks as filterSensorBenchmarks,
    fmtCpuDelta,
    sensorBenchModeForSelection
  } from '$lib/components/register-camera/registerCameraBenchUtils';
  import {
    bestIntervalIndex,
    bestResolutionModeForFormat,
    buildPeerStreamDevices,
    buildRegisterCameraManifest,
    dedupeCodecs,
    dedupeModesByResolution,
    encodeSimpleAttachSelection,
    findBackendIndexForSimpleKind,
    modeMatchesSimpleKind,
    pipelineDisplayName,
    resolveSimpleAttachPipelineId,
    resolveSimpleMode,
    sortPipelineEntries,
    sortTemplateEntries,
    type RegisterExperience,
    type SimplePipelineSource,
    type SimpleStreamKind
  } from '$lib/components/register-camera/registerCameraModalHelpers';
  import { createDevicesUpdatesHandler } from '$lib/components/register-camera/registerCameraUpdates';
  import {
    currentBackend as selectCurrentBackend,
    currentDevice as selectCurrentDevice,
    currentEncoder as selectCurrentEncoder,
    currentModes as selectCurrentModes,
    decodersForFormat as selectDecodersForFormat,
    deviceIsInUse,
    formatLabel as formatCodecLabel,
    formats as selectFormats,
    intervalToFps,
    intervalsForSelected as selectIntervalsForSelected,
    isRegistered as isDeviceRegistered,
    resolutionKey,
    resolutionLabel,
    resolutionsForSelectedFormat as selectResolutionsForSelectedFormat,
    streamAssignedKeys
  } from '$lib/components/register-camera/registerCameraSelectors';
  import type { SensorBenchListItem, SensorBenchResult } from '$lib/components/register-camera/sensorBenchTypes';
  import { SvelteSet } from 'svelte/reactivity';

  const dispatch = createEventDispatcher<{ create: { streamId?: string; descriptor?: unknown } }>();
  const apiPath = (path: string): string => apiUrl(path);

  let { registeredIds = [], registeredHardwareIds = [] } = $props();

  let loading = $state(true);
  let submitting = $state(false);
  let error = $state<string | null>(null);
  let submitError = $state<string | null>(null);
  let submitValidationIssues = $state<ValidationIssue[]>([]);
  let devices = $state<ProbedDevice[]>([]);
  let codecs = $state<CodecInfo[]>([]);
  let encoders = $state<CodecInfo[]>([]);
  let decoders = $state<CodecInfo[]>([]);
  let selectedDeviceIndex = $state(0);
  let selectedBackendIndex = $state(0);
  let selectedModeId = $state<ModeId | null>(null);
  let selectedFormat = $state<string | null>(null);
  let selectedResolutionKey = $state<string | null>(null);
  let selectedInterval = $state<Interval | null>(null);
  let alias = $state('');
  let encoderImpl = $state<string | null>(null);
  let decoderImpl = $state<string | null>(null);
  let decoderRotationDegrees = $state(0);
  let decoderMirrorHorizontal = $state(false);
  let hostBuffer = $state<number>(0);
  let encoderSettings = $state<EncoderSettingsDraft>(createEncoderSettingsDraft());
  let fpsLimit = $state<number | null>(null);
  let showSensorBenchModal = $state(false);
  let showSensorBenchResults = $state(false);
  let showAdvancedSettings = $state(false);
  let encoderSettingsModalOpen = $state(false);
  let registerExperience = $state<RegisterExperience>('simple');
  let simpleStreamKind = $state<SimpleStreamKind>('bw');
  let simplePipelineSource = $state<SimplePipelineSource>('none');
  let simplePipelineId = $state<string | null>(null);
  let simpleTemplateId = $state<string | null>(null);
  let availablePipelines = $state<PipelineSummary[]>([]);
  let availableTemplates = $state<PipelineTemplateSummary[]>([]);
  const WS_REFRESH_DEBOUNCE_MS = 250;
  const DEVICES_UPDATES_RECONNECT_MS = 750;

  const currentDevice = (): ProbedDevice | null => selectCurrentDevice(devices, selectedDeviceIndex);
  const currentBackend = (): ProbedBackend | null =>
    selectCurrentBackend(devices, selectedDeviceIndex, selectedBackendIndex);
  const currentModes = (): Mode[] => selectCurrentModes(devices, selectedDeviceIndex, selectedBackendIndex);
  const formatLabel = (fmt: string | null | undefined): string => formatCodecLabel(fmt);
  const formats = (): string[] => selectFormats(devices, selectedDeviceIndex, selectedBackendIndex);
  const resolutionsForSelectedFormat = (): Mode[] =>
    selectResolutionsForSelectedFormat(devices, selectedDeviceIndex, selectedBackendIndex, selectedFormat);
  const decodersForFormat = (): CodecInfo[] => selectDecodersForFormat(decoders, selectedFormat);
  const intervalsForSelected = (): Interval[] =>
    selectIntervalsForSelected(devices, selectedDeviceIndex, selectedBackendIndex, selectedResolutionKey, selectedFormat);
  const currentEncoder = (): CodecInfo | undefined => selectCurrentEncoder(encoders, encoderImpl);
  const encoderSettingsAvailable = $derived(Boolean(encoderImpl && currentEncoder()?.tunables?.encoder_settings));
  const selectedModeForEncoderSettings = (): Mode | null =>
    currentModes().find((m) => m.id === selectedModeId) ??
    currentModes().find((m) => resolutionKey(m) === selectedResolutionKey && formatLabel(m.format?.code) === selectedFormat) ??
    currentModes().find((m) => formatLabel(m.format?.code) === selectedFormat) ??
    currentModes()[0] ??
    null;
  const encoderSourceResolution = $derived.by(() => {
    const mode = selectedModeForEncoderSettings();
    const width = Number(mode?.format?.resolution?.width ?? 0);
    const height = Number(mode?.format?.resolution?.height ?? 0);
    if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0) return null;
    return { width: Math.trunc(width), height: Math.trunc(height) };
  });
  const isRegistered = (device: ProbedDevice | null): boolean => isDeviceRegistered(device, registeredHardwareIds);
  let streamCapabilities = $state<StreamCapabilitiesResponse | null>(null);
  const dedupeSimpleModesByResolution = (modes: Mode[]): Mode[] => dedupeModesByResolution(modes, resolutionKey);
  const simpleResolutionModes = $derived.by(() => {
    const matches = (currentBackend()?.descriptor?.modes ?? []).filter((mode) => modeMatchesSimpleKind(mode, simpleStreamKind));
    return dedupeSimpleModesByResolution(matches);
  });
  const simpleCanSubmit = $derived.by(() => {
    if (!currentDevice() || !currentBackend()) return false;
    return simpleResolutionModes.length > 0;
  });
  const simpleAttachSelection = $derived.by(() =>
    encodeSimpleAttachSelection(simplePipelineSource, simplePipelineId, simpleTemplateId)
  );

  let sensorBenchmarks = $state<SensorBenchListItem[]>([]);
  let sensorBenchSelectedId = $state<string | null>(null);
  let sensorBenchSelectedResult = $state<SensorBenchResult | null>(null);
  let sensorBenchLoading = $state(false);
  let sensorBenchError = $state<string | null>(null);
  const sensorBenchRuns = $derived(
    filterSensorBenchmarks(
      sensorBenchmarks,
      String(currentBackend()?.kind ?? '').toLowerCase(),
      (currentDevice()?.identity?.keys ?? [])[0] ?? null
    )
  );
  const sensorBenchSelection = $derived(
    sensorBenchModeForSelection(sensorBenchSelectedResult, selectedFormat, selectedResolutionKey)
  );
  const sensorBenchBestDecoder = $derived((sensorBenchSelection?.decoders ?? [])[0] ?? null);
  const sensorBenchBestEncoder = $derived((sensorBenchSelection?.encoders ?? [])[0] ?? null);

  const showNetcamWarning = $derived(String(currentBackend()?.kind ?? '').toLowerCase() === 'netcam');
  const isFileBackend = $derived(String(currentBackend()?.kind ?? '').toLowerCase() === 'file');

  const devicesUpdates = createDevicesUpdatesHandler({
    connectDevicesUpdatesStream,
    onRefresh: () => {
      void loadDevices({ preserveSelection: true, silent: true });
    },
    refreshDebounceMs: WS_REFRESH_DEBOUNCE_MS,
    reconnectMs: DEVICES_UPDATES_RECONNECT_MS
  });

  onMount(() => {
    void loadDevices();
    devicesUpdates.connectUpdates();
  });

  onDestroy(() => {
    devicesUpdates.disconnectUpdates();
  });

  $effect(() => {
    void registeredIds.length;
  });

  type SensorBenchmarksResponse = { benchmarks?: SensorBenchListItem[] };
  type SensorBenchStatusResponse = { status?: string; result?: SensorBenchResult | null };

  async function refreshSensorBenchmarks(): Promise<void> {
    sensorBenchError = null;
    sensorBenchLoading = true;
    try {
      const json = await apiFetch<SensorBenchmarksResponse>(apiPath('/streams/bench/sensor'));
      sensorBenchmarks = Array.isArray(json?.benchmarks) ? (json.benchmarks as SensorBenchListItem[]) : [];
      const filtered = filterSensorBenchmarks(
        sensorBenchmarks,
        String(currentBackend()?.kind ?? '').toLowerCase(),
        (currentDevice()?.identity?.keys ?? [])[0] ?? null
      );
      const ids = new SvelteSet(filtered.map((b) => b?.summary?.benchmark_id).filter(Boolean) as string[]);
      if (!sensorBenchSelectedId || !ids.has(sensorBenchSelectedId)) {
        sensorBenchSelectedId = filtered[0]?.summary?.benchmark_id ?? null;
      }
    } catch (err) {
      sensorBenchError = err instanceof Error ? err.message : String(err);
      sensorBenchmarks = [];
      sensorBenchSelectedId = null;
      sensorBenchSelectedResult = null;
    } finally {
      sensorBenchLoading = false;
    }
  }

  async function loadSensorBenchResult(id: string): Promise<void> {
    sensorBenchSelectedResult = null;
    sensorBenchError = null;
    try {
      const status = await apiFetch<SensorBenchStatusResponse>(apiPath(`/streams/bench/sensor/${encodeURIComponent(id)}`));
      if (status?.status === 'completed' && status?.result) {
        sensorBenchSelectedResult = status.result as SensorBenchResult;
      }
    } catch (err) {
      sensorBenchError = err instanceof Error ? err.message : String(err);
    }
  }

  $effect(() => {
    if (!showSensorBenchModal && !showSensorBenchResults) return;
    const identityKey = `${currentBackend()?.kind ?? 'none'}:${currentDevice()?.identity?.keys?.[0] ?? 'none'}`;
    void identityKey;
    sensorBenchSelectedId = null;
    sensorBenchSelectedResult = null;
    void refreshSensorBenchmarks();
  });

  $effect(() => {
    if (!showSensorBenchModal && !showSensorBenchResults) return;
    if (!sensorBenchSelectedId) {
      sensorBenchSelectedResult = null;
      return;
    }
    void loadSensorBenchResult(sensorBenchSelectedId);
  });

  const close = () => registerCameraModal.set(false);

  async function loadDevices(options: { preserveSelection?: boolean; silent?: boolean } | Event = {}): Promise<void> {
    const silent =
      typeof options === 'object' && options !== null && 'silent' in options
        ? Boolean((options as { silent?: boolean }).silent)
        : false;
    if (!silent) {
      loading = true;
      error = null;
      submitError = null;
      submitValidationIssues = [];
    }
    const preserveSelection =
      typeof options === 'object' && options !== null && 'preserveSelection' in options
        ? Boolean((options as { preserveSelection?: boolean }).preserveSelection)
        : false;
    const prevDeviceKeys = preserveSelection
      ? (currentDevice()?.identity?.keys ?? []).filter(
          (key): key is string => typeof key === 'string' && key.trim().length > 0
        )
      : [];
    const prevBackendKind = preserveSelection ? currentBackend()?.kind ?? null : null;
    const prevModeId = preserveSelection ? selectedModeId : null;
    const prevFormat = preserveSelection ? selectedFormat : null;
    const prevResolution = preserveSelection ? selectedResolutionKey : null;
    try {
      const [cameraResp, codecResp, streamsResp, peersResp, pipelineResp, templateResp, streamCapabilitiesResp] = await Promise.all([
        PeripheralsService.listCameras(),
        StreamsApi.listCodecs().catch(() => null) as Promise<CodecInfo[] | null>,
        loadOwnedStreams({ preferCached: false }).catch(() => []),
        PeersService.listPeers().catch(() => null) as Promise<{ peers?: PeerInfo[] } | null>,
        PipelinesApi.listGraphs().catch(() => []) as Promise<PipelineSummary[]>,
        PipelinesApi.listTemplates().catch(() => []) as Promise<PipelineTemplateSummary[]>,
        loadOwnedStreamCapabilities({ preferCached: false }).catch(() => null)
      ]);

      const allCodecs = Array.isArray(codecResp) ? codecResp.filter((c) => c?.fourcc) : [];

      encoders = dedupeCodecs(
        allCodecs.filter((c) => String(c.kind).toLowerCase() === 'encoder'),
        (codec) => {
          const selection = encoderSelectionId(codec);
          if (selection) return `selection|${selection}`;
          const impl = String(codec.implementation ?? '').trim().toLowerCase();
          if (impl) return `impl|${impl}`;
          const output = String(codec.output ?? codec.fourcc ?? '').trim().toLowerCase();
          const name = String(codec.name ?? '').trim().toLowerCase();
          return `fallback|${name}|${output}`;
        }
      );
      decoders = dedupeCodecs(
        allCodecs.filter((c) => String(c.kind).toLowerCase() === 'decoder'),
        (codec) => {
          const impl = String(codec.implementation ?? '').trim().toLowerCase();
          const input = String(codec.input ?? codec.fourcc ?? '').trim().toLowerCase();
          if (impl || input) return `impl|${impl}|input|${input}`;
          const name = String(codec.name ?? '').trim().toLowerCase();
          return `fallback|${name}`;
        }
      );
      codecs = encoders;
      encoderImpl = encoderSelectionId(encoders[0]) ?? null;
      availablePipelines = sortPipelineEntries(
        (Array.isArray(pipelineResp) ? pipelineResp : []).filter((entry) => entry && typeof entry.id === 'string')
      );
      availableTemplates = sortTemplateEntries(
        (Array.isArray(templateResp) ? templateResp : []).filter((entry) => entry && typeof entry.templateId === 'string')
      );
      if (!simplePipelineId && availablePipelines.length) {
        simplePipelineId = availablePipelines[0].id;
      }
      if (!simpleTemplateId && availableTemplates.length) {
        simpleTemplateId = availableTemplates[0].templateId;
      }
      if (simplePipelineSource === 'existing') {
        if (!availablePipelines.some((entry) => entry.id === simplePipelineId)) {
          if (availablePipelines.length) {
            simplePipelineId = availablePipelines[0].id;
          } else {
            simplePipelineSource = 'none';
            simplePipelineId = null;
          }
        }
      }
      if (simplePipelineSource === 'template') {
        if (!availableTemplates.some((entry) => entry.templateId === simpleTemplateId)) {
          if (availableTemplates.length) {
            simpleTemplateId = availableTemplates[0].templateId;
          } else {
            simplePipelineSource = 'none';
            simpleTemplateId = null;
          }
        }
      }
      streamCapabilities = streamCapabilitiesResp;
      const streamDefaults = resolveStreamCreationDefaults(streamCapabilitiesResp);
      if (!preserveSelection && streamDefaults) {
        hostBuffer = streamDefaults.defaultHostBuffer;
      }

      const list = Array.isArray(cameraResp?.cameras) ? cameraResp.cameras.filter(Boolean) : [];
      const usedKeys = streamAssignedKeys(Array.isArray(streamsResp) ? streamsResp : []);
      const peerDevices = buildPeerStreamDevices((peersResp?.peers ?? []).filter(Boolean) as PeerInfo[]);
      const combined = [...list, ...peerDevices];
      devices = combined.filter((device) => !deviceIsInUse(device, usedKeys));
      let nextDeviceIndex = devices.length ? 0 : -1;
      if (preserveSelection && prevDeviceKeys.length) {
        const matchIndex = devices.findIndex((device) => {
          const keys = device?.identity?.keys ?? [];
          return keys.some((key) => typeof key === 'string' && prevDeviceKeys.includes(key));
        });
        if (matchIndex >= 0) {
          nextDeviceIndex = matchIndex;
        }
      }

      selectedDeviceIndex = nextDeviceIndex;
      selectedBackendIndex = 0;
      if (preserveSelection && prevBackendKind) {
        const backends = devices[nextDeviceIndex]?.backends ?? [];
        const backendIndex = backends.findIndex((backend) => backend?.kind === prevBackendKind);
        selectedBackendIndex = backendIndex >= 0 ? backendIndex : 0;
      }

      let nextMode = currentModes()[0];
      if (preserveSelection && prevModeId) {
        const match = currentModes().find((mode) => mode?.id === prevModeId);
        if (match) nextMode = match;
      }
      if (preserveSelection && !nextMode && prevFormat) {
        nextMode = currentModes().find((mode) => formatLabel(mode?.format?.code) === prevFormat) ?? null;
      }
      if (preserveSelection && !nextMode && prevResolution) {
        nextMode = currentModes().find((mode) => resolutionKey(mode) === prevResolution) ?? null;
      }

      selectedModeId = nextMode?.id ?? null;
      selectedFormat = formatLabel(nextMode?.format?.code);
      decoderImpl = decodersForFormat()[0]?.implementation ?? null;
      selectedResolutionKey = resolutionKey(nextMode ?? undefined);
      selectedInterval = intervalsForSelected()[0] ?? null;
      if (!currentEncoder() && codecs.length) {
        encoderImpl = encoderSelectionId(codecs[0]) ?? null;
      }
      if (registerExperience === 'simple') {
        syncSimpleSelection();
      }
    } catch (err) {
      error = buildErrorMessage({ error: err, fallback: 'Unable to load cameras right now.' });
    } finally {
      if (!silent) {
        loading = false;
      }
    }
  }

  function handleDeviceChange(index: number): void {
    selectedDeviceIndex = index;
    selectedBackendIndex = 0;
    const firstMode = currentModes()[0];
    selectedModeId = firstMode?.id ?? null;
    selectedFormat = formatLabel(firstMode?.format?.code);
    decoderImpl = decodersForFormat()[0]?.implementation ?? null;
    selectedResolutionKey = resolutionKey(firstMode);
    selectedInterval = intervalsForSelected()[0] ?? null;
    if (registerExperience === 'simple') {
      syncSimpleSelection();
    }
  }

  function handleBackendChange(index: number): void {
    selectedBackendIndex = index;
    const firstMode = currentModes()[0];
    const prevResolution = selectedResolutionKey;
    const prevInterval = selectedInterval;
    selectedModeId = firstMode?.id ?? null;
    selectedFormat = formatLabel(firstMode?.format?.code);
    decoderImpl = decodersForFormat()[0]?.implementation ?? null;
    selectedResolutionKey = resolutionKey(firstMode);
    selectedInterval = intervalsForSelected()[0] ?? null;
    if (prevResolution) handleResolutionChange(prevResolution);
    if (prevInterval) {
      const idx = bestIntervalIndex(prevInterval, intervalsForSelected());
      if (idx != null) handleIntervalChange(idx);
    }
  }

  function handleFormatChange(fmt: string | null): void {
    const prevResolution = selectedResolutionKey;
    const prevInterval = selectedInterval;
    selectedFormat = fmt ?? null;
    const resMode =
      bestResolutionModeForFormat(currentModes(), formatLabel, resolutionKey, selectedFormat, prevResolution) ??
      currentModes()[0];
    selectedResolutionKey = resolutionKey(resMode);
    selectedModeId = resMode?.id ?? null;
    decoderImpl = decodersForFormat()[0]?.implementation ?? null;
    selectedInterval = intervalsForSelected()[0] ?? null;
    if (prevInterval) {
      const idx = bestIntervalIndex(prevInterval, intervalsForSelected());
      if (idx != null) handleIntervalChange(idx);
    }
  }

  function handleResolutionChange(key: string | null): void {
    const prevInterval = selectedInterval;
    selectedResolutionKey = key;
    const mode =
      currentModes().find((m) => resolutionKey(m) === key && formatLabel(m.format?.code) === selectedFormat) ??
      currentModes().find((m) => formatLabel(m.format?.code) === selectedFormat) ??
      currentModes()[0];
    selectedModeId = mode?.id ?? null;
    selectedInterval = intervalsForSelected()[0] ?? null;
    if (prevInterval) {
      const idx = bestIntervalIndex(prevInterval, intervalsForSelected());
      if (idx != null) handleIntervalChange(idx);
    }
  }

  function handleIntervalChange(idx: number): void {
    const intervals = intervalsForSelected();
    selectedInterval = intervals[idx] ?? intervals[0] ?? null;
  }

  function syncSimpleSelection(): void {
    const device = currentDevice();
    if (!device) return;
    const backendIndex = findBackendIndexForSimpleKind(device, simpleStreamKind);
    selectedBackendIndex = backendIndex;

    const matches = (currentBackend()?.descriptor?.modes ?? []).filter((mode) => modeMatchesSimpleKind(mode, simpleStreamKind));
    const byResolution = dedupeSimpleModesByResolution(matches);
    if (!byResolution.length) {
      const fallback = currentModes()[0] ?? null;
      selectedModeId = fallback?.id ?? null;
      selectedFormat = formatLabel(fallback?.format?.code);
      selectedResolutionKey = resolutionKey(fallback ?? undefined);
      selectedInterval = fallback?.intervals?.[0] ?? null;
      return;
    }
    const preferred =
      byResolution.find((mode) => resolutionKey(mode) === selectedResolutionKey) ??
      byResolution[0];
    selectedModeId = preferred?.id ?? null;
    selectedFormat = formatLabel(preferred?.format?.code);
    selectedResolutionKey = resolutionKey(preferred ?? undefined);
    selectedInterval = preferred?.intervals?.[0] ?? null;
    decoderImpl = decodersForFormat()[0]?.implementation ?? null;
  }

  function handleRegisterExperienceChange(next: RegisterExperience): void {
    registerExperience = next;
    if (next === 'simple') {
      syncSimpleSelection();
    }
  }

  function handleSimpleStreamKindChange(kind: SimpleStreamKind): void {
    simpleStreamKind = kind;
    syncSimpleSelection();
  }

  function handleSimpleAttachSelection(value: string): void {
    const trimmed = value.trim();
    if (!trimmed || trimmed === 'none') {
      simplePipelineSource = 'none';
      return;
    }
    if (trimmed.startsWith('pipeline:')) {
      const id = trimmed.slice('pipeline:'.length).trim();
      simplePipelineSource = id ? 'existing' : 'none';
      simplePipelineId = id || null;
      return;
    }
    if (trimmed.startsWith('template:')) {
      const id = trimmed.slice('template:'.length).trim();
      simplePipelineSource = id ? 'template' : 'none';
      simpleTemplateId = id || null;
      return;
    }
    simplePipelineSource = 'none';
  }

  function handleSimpleAttachSelectionEvent(event: Event): void {
    const target = event.currentTarget;
    if (!(target instanceof HTMLSelectElement)) {
      return;
    }
    handleSimpleAttachSelection(target.value);
  }

  async function submit(): Promise<void> {
    submitError = null;
    submitValidationIssues = [];
    const device = currentDevice();
    const backend = currentBackend();
    const isSimpleRegistration = registerExperience === 'simple';
    const mode = isSimpleRegistration
      ? resolveSimpleMode(backend, simpleStreamKind, selectedResolutionKey, resolutionKey)
      : currentModes().find(
          (m) =>
            m.id === selectedModeId ||
            (resolutionKey(m) === selectedResolutionKey &&
              formatLabel(m.format?.code) === selectedFormat &&
              m.intervals?.some((i: Interval) => i === selectedInterval))
        ) ??
        currentModes().find((m) => resolutionKey(m) === selectedResolutionKey && formatLabel(m.format?.code) === selectedFormat) ??
        currentModes().find((m) => formatLabel(m.format?.code) === selectedFormat) ??
        currentModes()[0];
    if (!device || !backend || !mode) {
      toaster.error({
        title: 'Missing selection',
        description: isSimpleRegistration
          ? 'Choose a camera, stream type, and resolution.'
          : 'Choose a camera, backend, and mode.'
      });
      return;
    }

    submitting = true;
    let attachedPipelineId: string | null = null;
    if (isSimpleRegistration) {
      try {
        attachedPipelineId = await resolveSimpleAttachPipelineId({
          source: simplePipelineSource,
          pipelineId: simplePipelineId,
          templateId: simpleTemplateId,
          alias,
          device
        });
      } catch (attachError) {
        submitting = false;
        toaster.error({
          title: 'Pipeline attach failed',
          description: attachError instanceof Error ? attachError.message : 'Unable to prepare selected pipeline/template.'
        });
        return;
      }
    }

    let manifest;
    try {
      manifest = buildRegisterCameraManifest({
        device,
        backend,
        mode,
        selectedInterval,
        registerExperience,
        attachedPipelineId,
        alias,
        encoderImpl,
        decoderImpl,
        decoderRotationDegrees,
        decoderMirrorHorizontal,
        hostBuffer,
        fpsLimit,
        encoderSettings,
        streamCapabilities
      });
    } catch (manifestError) {
      submitting = false;
      toaster.error({
        title: 'Stream manifest failed',
        description: manifestError instanceof Error ? manifestError.message : 'Unable to prepare the stream manifest.'
      });
      return;
    }

    try {
      const response = await StreamsApi.startStream({ requestBody: manifest });
      const streamId = response.stream_id;
      toaster.success({ title: 'Stream created', description: streamId ? `Stream ${streamId} started` : 'Capture stream started' });
      invalidateOwnedStreamMutationResources();
      dispatch('create', { streamId, descriptor: response.descriptor });
      registerCameraModal.set(false);
    } catch (err) {
      const validationReport = extractValidationReport(err);
      submitValidationIssues = validationReport?.issues ?? [];
      reportError({
        title: 'Stream start failed',
        error: err,
        fallback: 'Unable to start the stream right now.',
        inline: (message) => {
          submitError = message;
        }
      });
    } finally {
      submitting = false;
    }
  }
</script>

{#if loading}
  <ModalShell open title="Loading cameras…" closeOnBackdrop={false} closeOnEsc={false} className="z-50" panelClassName="max-w-sm">
    <div class="py-6 text-center text-sm text-surface-300">Fetching available cameras…</div>
  </ModalShell>
{:else if error}
  <ModalShell open title="Unable to load cameras" subtitle={error} className="z-50" onClose={close}>
    {#snippet footer()}
      <div class="flex justify-end gap-2">
        <button class="btn btn-ghost" type="button" onclick={close}>Close</button>
        <button class="btn preset-filled-primary-500" type="button" onclick={loadDevices}>Retry</button>
      </div>
    {/snippet}
  </ModalShell>
{:else}
  <ModalShell
    open
    size="xl"
    className="z-50"
    panelClassName="border-surface-800 bg-surface-900/95 text-surface-50 shadow-2xl backdrop-blur max-h-[min(94dvh,58rem)]"
    onClose={close}
  >
    {#snippet header()}
      <RegisterCameraModalHeader
        {registerExperience}
        {submitError}
        {submitValidationIssues}
        onRegisterExperienceChange={handleRegisterExperienceChange}
      />
    {/snippet}
    {#snippet actions()}
      <RegisterCameraModalActions
        {registerExperience}
        sensorBenchEnabled={Boolean(currentDevice() && currentBackend())}
        onOpenSensorBench={() => (showSensorBenchModal = true)}
        onClose={close}
      />
    {/snippet}
      <RegisterCameraModalBody
        {devices}
        {isRegistered}
        {currentDevice}
        {currentBackend}
        {registerExperience}
        {simpleStreamKind}
        {simpleResolutionModes}
        {selectedDeviceIndex}
        {selectedBackendIndex}
        {selectedFormat}
        {selectedResolutionKey}
        {selectedInterval}
        {simpleAttachSelection}
        {availablePipelines}
        {availableTemplates}
        {simpleCanSubmit}
        {submitting}
        {resolutionKey}
        {resolutionLabel}
        {pipelineDisplayName}
        {currentModes}
        {formats}
        {resolutionsForSelectedFormat}
        {intervalsForSelected}
        {formatLabel}
        {intervalToFps}
        {isFileBackend}
        {showNetcamWarning}
        {decodersForFormat}
        {codecs}
        {currentEncoder}
        {encoderSettingsAvailable}
        {sensorBenchError}
        {sensorBenchLoading}
        {sensorBenchRuns}
        {sensorBenchSelection}
        {sensorBenchBestDecoder}
        {sensorBenchBestEncoder}
        {showSensorBenchModal}
        {showSensorBenchResults}
        {fmtCpuDelta}
        onSelectDevice={handleDeviceChange}
        onSelectSimpleStreamKind={handleSimpleStreamKindChange}
        onSelectResolution={handleResolutionChange}
        onSimpleAttachSelectionEvent={handleSimpleAttachSelectionEvent}
        onBackendChange={handleBackendChange}
        onFormatChange={handleFormatChange}
        onIntervalChange={handleIntervalChange}
        onOpenEncoderSettings={() => (encoderSettingsModalOpen = true)}
        onToggleBenchModal={(open) => (showSensorBenchModal = open)}
        onToggleBenchResults={(open) => (showSensorBenchResults = open)}
        onCancel={close}
        onRetry={() => void loadDevices()}
        onSubmit={submit}
        bind:alias
        bind:decoderImpl
        bind:encoderImpl
        bind:decoderRotationDegrees
        bind:decoderMirrorHorizontal
        bind:hostBuffer
        bind:fpsLimit
        bind:showAdvancedSettings
      />
  </ModalShell>
{/if}

<RegisterCameraBenchmarkModal
  open={showSensorBenchModal}
  {apiPath}
  device={currentDevice()}
  backend={currentBackend()}
  onClose={() => (showSensorBenchModal = false)}
/>

<RegisterCameraBenchmarkResultsModal
  open={showSensorBenchResults}
  bind:sensorBenchSelectedId={sensorBenchSelectedId}
  {sensorBenchError}
  {sensorBenchLoading}
  sensorBenchRuns={sensorBenchRuns}
  sensorBenchSelectedResult={sensorBenchSelectedResult}
  {sensorBenchSelection}
  sensorBenchBestDecoder={sensorBenchBestDecoder}
  sensorBenchBestEncoder={sensorBenchBestEncoder}
  onClose={() => (showSensorBenchResults = false)}
  onRefresh={refreshSensorBenchmarks}
  onUseDecoder={(implementation) => (decoderImpl = implementation)}
  onUseEncoder={(implementation) => (encoderImpl = implementation)}
/>

<RegisterCameraEncoderSettingsModal
  open={encoderSettingsModalOpen}
  {encoderSettings}
  sourceResolution={encoderSourceResolution}
  currentEncoder={currentEncoder}
  onClose={() => (encoderSettingsModalOpen = false)}
/>
