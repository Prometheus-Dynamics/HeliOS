<script lang="ts">
  import { createEventDispatcher, onDestroy, onMount } from 'svelte';
  import { toaster } from '$lib';
  import { apiFetch } from '$lib/api/core/http';
  import { resolveStreamCreationDefaults } from '$lib/api/streamDefaults';
  import {
    buildEncoderSettingsForSelection,
    createEncoderSettingsDraft,
    encoderSelectionId,
    type EncoderSettingsDraft
  } from '$lib/api/streamEncoderSettings';
  import { normalizeRecordingMode } from '$lib/api/streamRecordingMode';
  import { withCurrentStreamManifestSchema } from '$lib/api/streamSchema';
  import { ApiError, PeersService, PeripheralsService, apiUrl } from '$lib/api/client';
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
    CaptureConfig,
    CodecInfo,
    Interval,
    Mode,
    ModeId,
    PipelineSummary,
    PipelineTemplateSummary,
    ProbedBackend,
    ProbedDevice,
    PeerInfo,
    StreamManifest,
    ValidationIssue
  } from '$lib/api/client';
  import type { StreamCapabilitiesResponse } from '$lib/api/client';
  import { registerCameraModal } from '$lib/stores/modals';
  import ModalShell from '$lib/components/ui/ModalShell.svelte';
  import ValidationIssueList from '$lib/components/ui/ValidationIssueList.svelte';
  import RegisterCameraDeviceList from '$lib/components/register-camera/RegisterCameraDeviceList.svelte';
  import RegisterCameraBackendMode from '$lib/components/register-camera/RegisterCameraBackendMode.svelte';
  import RegisterCameraStreamSettings from '$lib/components/register-camera/RegisterCameraStreamSettings.svelte';
  import RegisterCameraBenchmarkModal from '$lib/components/register-camera/RegisterCameraBenchmarkModal.svelte';
  import RegisterCameraBenchmarkResultsModal from '$lib/components/register-camera/RegisterCameraBenchmarkResultsModal.svelte';
  import RegisterCameraEncoderSettingsModal from '$lib/components/register-camera/RegisterCameraEncoderSettingsModal.svelte';
  import {
    filteredSensorBenchmarks as filterSensorBenchmarks,
    fmtCpuDelta,
    sensorBenchModeForSelection
  } from '$lib/components/register-camera/registerCameraBenchUtils';
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
  import type {
    SensorBenchListItem,
    SensorBenchResult
  } from '$lib/components/register-camera/sensorBenchTypes';
  import { SvelteSet, SvelteURL } from 'svelte/reactivity';

  const dispatch = createEventDispatcher<{ create: { streamId?: string; descriptor?: unknown } }>();
  const asRecord = (value: unknown): Record<string, unknown> | null =>
    value && typeof value === 'object' ? (value as Record<string, unknown>) : null;
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
  type RegisterExperience = 'simple' | 'advanced';
  type SimpleStreamKind = 'bw' | 'color';
  type SimplePipelineSource = 'none' | 'existing' | 'template';
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
  const SIMPLE_KIND_FORMATS: Record<SimpleStreamKind, string[]> = {
    bw: ['N12', 'NV12'],
    color: ['YUYV']
  };
  const normalizeFormatCode = (value: string | null | undefined): string =>
    String(value ?? '')
      .trim()
      .toUpperCase()
      .replace(/[^A-Z0-9]/g, '');
  const modeMatchesSimpleKind = (mode: Mode | null | undefined, kind: SimpleStreamKind): boolean => {
    const code = normalizeFormatCode(mode?.format?.code ?? null);
    if (!code) return false;
    return SIMPLE_KIND_FORMATS[kind].some((candidate) => normalizeFormatCode(candidate) === code);
  };
  const backendSupportsSimpleKind = (backend: ProbedBackend | null, kind: SimpleStreamKind): boolean =>
    (backend?.descriptor?.modes ?? []).some((mode) => modeMatchesSimpleKind(mode, kind));
  const findBackendIndexForSimpleKind = (device: ProbedDevice | null, kind: SimpleStreamKind): number => {
    const backends = device?.backends ?? [];
    const index = backends.findIndex((backend) => backendSupportsSimpleKind(backend, kind));
    return index >= 0 ? index : 0;
  };
  const dedupeModesByResolution = (modes: Mode[]): Mode[] => {
    const seen = new SvelteSet<string>();
    const out: Mode[] = [];
    for (const mode of modes) {
      const key = resolutionKey(mode);
      if (!key || seen.has(key)) continue;
      seen.add(key);
      out.push(mode);
    }
    return out;
  };
  const simpleResolutionModes = $derived.by(() => {
    const matches = (currentBackend()?.descriptor?.modes ?? []).filter((mode) => modeMatchesSimpleKind(mode, simpleStreamKind));
    return dedupeModesByResolution(matches);
  });
  const simpleCanSubmit = $derived.by(() => {
    if (!currentDevice() || !currentBackend()) return false;
    return simpleResolutionModes.length > 0;
  });
  const pipelineDisplayName = (entry: PipelineSummary | null | undefined): string => {
    const name = String(entry?.name ?? '').trim();
    return name.length ? name : entry?.id ?? 'Pipeline';
  };
  const encodeSimpleAttachSelection = (
    source: SimplePipelineSource,
    pipelineId: string | null,
    templateId: string | null
  ): string => {
    if (source === 'existing' && pipelineId) return `pipeline:${pipelineId}`;
    if (source === 'template' && templateId) return `template:${templateId}`;
    return 'none';
  };
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
      const dedupeCodecs = (items: CodecInfo[], keyFor: (codec: CodecInfo) => string): CodecInfo[] => {
        const seen = new SvelteSet<string>();
        const result: CodecInfo[] = [];
        for (const codec of items) {
          const key = keyFor(codec);
          if (seen.has(key)) continue;
          seen.add(key);
          result.push(codec);
        }
        return result;
      };

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
      availablePipelines = (Array.isArray(pipelineResp) ? pipelineResp : [])
        .filter((entry) => entry && typeof entry.id === 'string')
        .sort((a, b) => pipelineDisplayName(a).localeCompare(pipelineDisplayName(b)));
      availableTemplates = (Array.isArray(templateResp) ? templateResp : [])
        .filter((entry) => entry && typeof entry.templateId === 'string')
        .sort((a, b) => String(a.name ?? '').localeCompare(String(b.name ?? '')));
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

  function buildPeerStreamDevices(peers: PeerInfo[]): ProbedDevice[] {
    const out: ProbedDevice[] = [];
    for (const peer of peers) {
      if (!peer || typeof peer !== 'object') continue;
      const labelBase = String(peer.alias ?? peer.id ?? 'Peer').trim() || 'Peer';
      const integration = asRecord(peer.integration);
      const integrationKind = String(peer.integration?.kind ?? '').trim().toLowerCase();
      if (integrationKind === 'helios') {
        continue;
      }
      const urls = [
        ...(Array.isArray(integration?.streamUrls) ? integration.streamUrls : []),
        ...(Array.isArray(peer.integration?.stream_urls) ? peer.integration.stream_urls : []),
        integration?.streamUrl,
        peer.integration?.stream_url
      ]
        .map((value) => (typeof value === 'string' ? value.trim() : ''))
        .filter((value: string) => value.length > 0);

      const deduped = Array.from(new SvelteSet(urls));
      for (const url of deduped) {
        const parsed = safeParseUrl(url);
        const portLabel = parsed?.port ? parsed.port : '';
        const endpoint = parsed?.endpoint ?? 'stream';
        const display = `${labelBase} - ${portLabel}${portLabel ? ' ' : ''}${endpoint}`;
        const peerKey = (() => {
          const port = portLabel.trim();
          if (!port) return null;
          const alias = labelBase
            .trim()
            .toLowerCase()
            .replace(/\s+/g, '-')
            .replace(/[^a-z0-9_-]/g, '');
          const stable = alias || 'peer';
          return `peer:${stable}:${port}`;
        })();
        out.push({
          identity: { display, keys: peerKey ? [peerKey, url] : [url] },
          backends: [
            {
              kind: 'Netcam',
              // The API expects the same serde-tagged handle shape that `/v1/streams` returns
              // (e.g. `{ type: "libcamera", ... }`), so don't use the older `{ Netcam: {...} }`
              // OpenAPI union.
              handle: { type: 'netcam', url, width: 0, height: 0, fps: 30 } as unknown as ProbedBackend['handle'],
              properties: [],
              descriptor: {
                controls: [],
                modes: [
                  {
                    format: { code: 'MJPG', color: 'Srgb', resolution: { width: 1, height: 1 } },
                    id: {
                      format: { code: 'MJPG', color: 'Srgb', resolution: { width: 1, height: 1 } },
                      interval: { numerator: 1, denominator: 30 }
                    },
                    intervals: [{ numerator: 1, denominator: 30 }]
                  }
                ]
              }
            }
          ]
        });
      }
    }
    return out;
  }

  function safeParseUrl(raw: string): { port?: string; endpoint?: string } | null {
    const trimmed = raw.trim();
    if (!trimmed) return null;
    try {
      const url = new SvelteURL(trimmed);
      const port = url.port || (url.protocol === 'http:' ? '80' : url.protocol === 'https:' ? '443' : url.protocol === 'rtsp:' ? '554' : '');
      const endpoint = (() => {
        const path = url.pathname?.trim() ?? '';
        if (path && path !== '/') {
          const last = path.split('/').filter(Boolean).pop();
          if (last) return last;
        }
        if (url.search) {
          const q = url.search.replace(/^\?/, '').trim();
          if (q) return q;
        }
        return null;
      })();
      return { port: port || undefined, endpoint: endpoint ?? undefined };
    } catch {
      return null;
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

  function fpsOf(interval: Interval | null | undefined): number | null {
    if (!interval) return null;
    const num = Number(interval.numerator);
    const den = Number(interval.denominator);
    if (!Number.isFinite(num) || !Number.isFinite(den) || num <= 0 || den <= 0) return null;
    return den / num;
  }

	  function bestIntervalIndex(target: Interval | null | undefined, candidates: Interval[]): number | null {
	    const want = fpsOf(target);
	    if (want == null || !candidates.length) return null;
	    let bestIdx = 0;
	    let bestDiff = Infinity;
	    for (let i = 0; i < candidates.length; i += 1) {
	      const fps = fpsOf(candidates[i]);
	      if (fps == null) continue;
	      const diff = Math.abs(fps - want);
	      if (diff < bestDiff) {
	        bestDiff = diff;
	        bestIdx = i;
	      }
	    }
	    return Number.isFinite(bestDiff) ? bestIdx : null;
	  }

	  function parseResolutionKey(key: string | null | undefined): { w: number; h: number } | null {
	    if (!key) return null;
	    const m = String(key).match(/^(\d+)x(\d+)$/);
	    if (!m) return null;
	    const w = Number(m[1]);
	    const h = Number(m[2]);
	    if (!Number.isFinite(w) || !Number.isFinite(h) || w <= 0 || h <= 0) return null;
	    return { w, h };
	  }

	  function bestResolutionModeForFormat(fmt: string | null, targetKey: string | null): Mode | null {
	    const modes = currentModes().filter((m) => formatLabel(m.format?.code) === fmt);
	    if (!modes.length) return currentModes()[0] ?? null;
	    const want = parseResolutionKey(targetKey);
	    if (!want) return modes[0] ?? null;

	    let best: Mode | null = null;
	    let bestScore = Infinity;
	    for (const m of modes) {
	      const key = resolutionKey(m);
	      const r = parseResolutionKey(key);
	      if (!r) continue;
	      const dx = r.w - want.w;
	      const dy = r.h - want.h;
	      const score = dx * dx + dy * dy;
	      if (score < bestScore) {
	        bestScore = score;
	        best = m;
	      }
	    }
	    return best ?? (modes[0] ?? null);
	  }

	  function handleFormatChange(fmt: string | null): void {
	    const prevResolution = selectedResolutionKey;
	    const prevInterval = selectedInterval;
	    selectedFormat = fmt ?? null;
	    const resMode = bestResolutionModeForFormat(selectedFormat, prevResolution) ?? currentModes()[0];
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
    const byResolution = dedupeModesByResolution(matches);
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

  function resolveSimpleMode(backend: ProbedBackend | null): Mode | null {
    const matches = (backend?.descriptor?.modes ?? []).filter((mode) => modeMatchesSimpleKind(mode, simpleStreamKind));
    if (!matches.length) return null;
    return (
      matches.find((mode) => resolutionKey(mode) === selectedResolutionKey) ??
      matches[0] ??
      null
    );
  }

  async function resolveSimpleAttachPipelineId(device: ProbedDevice): Promise<string | null> {
    if (simplePipelineSource === 'none') {
      return null;
    }
    if (simplePipelineSource === 'existing') {
      const id = String(simplePipelineId ?? '').trim();
      return id || null;
    }
    const templateId = String(simpleTemplateId ?? '').trim();
    if (!templateId) return null;
    const templateDoc = await PipelinesApi.fetchTemplate({ id: templateId });
    const templateGraph = templateDoc?.graph;
    if (!templateGraph || typeof templateGraph !== 'object') {
      throw new Error('Selected template has no graph payload.');
    }
    const templateName = String(templateDoc?.name ?? '').trim() || 'Template Pipeline';
    const aliasSeed =
      String(alias).trim() ||
      String(device.identity?.display ?? '').trim() ||
      'Camera';
    const baseUploadName = `${templateName} - ${aliasSeed}`;

    const isIdentityConflict = (error: unknown): boolean => {
      if (!(error instanceof ApiError)) return false;
      if (error.status !== 409) return false;
      const message =
        typeof error.body === 'string'
          ? error.body
          : typeof asRecord(error.body)?.error === 'string'
            ? String(asRecord(error.body)?.error)
            : '';
      const normalized = message.toLowerCase();
      return normalized.includes('identity token already in use') || normalized.includes('collide within the requested pipeline');
    };

    let createdId: string | null = null;
    const MAX_ATTEMPTS = 8;
    for (let attempt = 0; attempt < MAX_ATTEMPTS; attempt += 1) {
      const uploadName = attempt === 0 ? baseUploadName : `${baseUploadName} (${attempt + 1})`;
      try {
        const created = await PipelinesApi.uploadGraph({ requestBody: { graph: templateGraph, name: uploadName } });
        createdId = String(created?.id ?? '').trim() || null;
        if (createdId) break;
      } catch (error) {
        if (isIdentityConflict(error) && attempt < MAX_ATTEMPTS - 1) {
          continue;
        }
        throw error;
      }
    }

    if (!createdId) {
      throw new Error('Failed to create pipeline from template.');
    }
    return createdId;
  }

  function selectStableHardwareId(keys: unknown): string | null {
    if (!Array.isArray(keys)) return null;
    const normalized = keys.map((value) => (typeof value === 'string' ? value.trim() : '')).filter(Boolean) as string[];
    const withSlash = normalized.find((value) => value.includes('/'));
    if (withSlash) return withSlash;
    const withColon = normalized.find((value) => value.includes(':'));
    if (withColon) return withColon;
    const sorted = [...normalized].sort();
    return sorted[0] ?? null;
  }

  async function submit(): Promise<void> {
    submitError = null;
    submitValidationIssues = [];
    const device = currentDevice();
    const backend = currentBackend();
    const isSimpleRegistration = registerExperience === 'simple';
    const mode = isSimpleRegistration
      ? resolveSimpleMode(backend)
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
        attachedPipelineId = await resolveSimpleAttachPipelineId(device);
      } catch (attachError) {
        submitting = false;
        toaster.error({
          title: 'Pipeline attach failed',
          description: attachError instanceof Error ? attachError.message : 'Unable to prepare selected pipeline/template.'
        });
        return;
      }
    }

    const capture: CaptureConfig = {
      device_keys: device.identity?.keys ?? [],
      backend: backend.kind,
      handle: backend.handle,
      mode: mode.id,
      interval:
        !isSimpleRegistration && fpsLimit && fpsLimit > 0
          ? { numerator: 1, denominator: Math.max(1, Math.round(fpsLimit)) }
          : selectedInterval ?? mode.intervals?.[0] ?? null,
      controls: []
    };

    const normalizedEncoderImpl = encoderImpl && encoderImpl.trim().length ? encoderImpl : null;
    const normalizedDecoderImpl = decoderImpl && decoderImpl.trim().length ? decoderImpl.trim() : null;
    const modeWidth = Number(mode.format?.resolution?.width ?? 0);
    const modeHeight = Number(mode.format?.resolution?.height ?? 0);
    const defaultEncoderOutputResolution =
      Number.isFinite(modeWidth) && Number.isFinite(modeHeight) && modeWidth > 0 && modeHeight > 0
        ? { width: Math.max(1, Math.trunc(modeWidth / 2)), height: Math.max(1, Math.trunc(modeHeight / 2)) }
        : null;
    const backendKind = String(backend.kind ?? '').trim().toLowerCase();
    const isMediaBackend = backendKind === 'file' || backendKind === 'netcam';
    const streamDefaults = resolveStreamCreationDefaults(streamCapabilities);
    if (!streamDefaults) {
      toaster.error({
        title: 'Stream defaults unavailable',
        description: 'Unable to determine the backend stream defaults required to build the manifest.'
      });
      return;
    }

    const rawPipelineId = String(streamCapabilities?.rawPipelineId ?? '').trim();
    const rawPipelineOutput = streamDefaults.rawOutput;
    if (isMediaBackend && (!rawPipelineId || !rawPipelineOutput)) {
      submitting = false;
      toaster.error({
        title: 'Stream capabilities unavailable',
        description: 'Unable to determine raw pipeline defaults from backend capabilities.'
      });
      return;
    }
    const shouldAttachSelectedPipeline = Boolean(attachedPipelineId);
    const useRawMediaPipelineInSimpleMode = isSimpleRegistration && !shouldAttachSelectedPipeline && isMediaBackend;

      const encoderSettingsWire = buildEncoderSettingsForSelection(normalizedEncoderImpl, encoderSettings, {
        frameRate:
          encoderSettings.framerateNum && encoderSettings.framerateDen
            ? { numerator: encoderSettings.framerateNum, denominator: encoderSettings.framerateDen }
            : null,
        defaultOutputResolution: defaultEncoderOutputResolution
      });

      const manifest: StreamManifest = withCurrentStreamManifestSchema({
        // Backend expects `DeviceIdentity { id, alias, hardware_id }` for stream manifests.
        // The TS bindings can lag (some shapes still show `{ display, keys }`), so cast through
        // `unknown` to keep the runtime JSON correct without widening the rest of the payload.
        // to keep the runtime JSON correct.
        identity: {
          id: null,
          alias: alias.trim().length ? alias.trim() : null,
          hardware_id: selectStableHardwareId(device.identity?.keys) ?? device.identity?.display?.trim?.() ?? null
        } as unknown as StreamManifest['identity'],
        capture,
        internal: false,
        pipeline_enabled: isSimpleRegistration
          ? shouldAttachSelectedPipeline || useRawMediaPipelineInSimpleMode
          : isMediaBackend,
        active_pipeline_id: isSimpleRegistration
          ? attachedPipelineId ?? (useRawMediaPipelineInSimpleMode ? rawPipelineId : null)
          : isMediaBackend
            ? rawPipelineId
            : null,
        active_pipeline_output: isSimpleRegistration
          ? useRawMediaPipelineInSimpleMode
            ? rawPipelineOutput
            : null
          : isMediaBackend
            ? rawPipelineOutput
            : null,
        pipelines: isSimpleRegistration
          ? attachedPipelineId
            ? [{ pipeline_id: attachedPipelineId, pipeline_graph: null, pipeline_output: null }]
            : useRawMediaPipelineInSimpleMode
              ? [{ pipeline_id: rawPipelineId, pipeline_graph: null, pipeline_output: rawPipelineOutput }]
              : []
          : isMediaBackend
            ? [{ pipeline_id: rawPipelineId, pipeline_graph: null, pipeline_output: rawPipelineOutput }]
            : [],
        pipeline_layout: isSimpleRegistration
          ? attachedPipelineId
            ? {
                rows: 1,
                columns: 1,
                slots: [{ row: 0, column: 0, pipeline_id: attachedPipelineId, output_key: null }]
              }
            : useRawMediaPipelineInSimpleMode
              ? {
                  rows: 1,
                  columns: 1,
                  slots: [{ row: 0, column: 0, pipeline_id: rawPipelineId, output_key: rawPipelineOutput }]
                }
              : null
          : isMediaBackend
            ? {
                rows: 1,
                columns: 1,
                slots: [{ row: 0, column: 0, pipeline_id: rawPipelineId, output_key: rawPipelineOutput }]
              }
            : null,
        pipeline_wires: [],
        pipeline_host_inputs: {},
        encoder: normalizedEncoderImpl
          ? {
              state: 'enabled',
              id: normalizedEncoderImpl,
              settings: encoderSettingsWire ?? undefined
            }
          : {
              state: 'disabled'
            },
        decoder: normalizedDecoderImpl
          ? {
              state: 'enabled',
              id: normalizedDecoderImpl,
              settings: {
                fps_limit: fpsLimit ?? null,
                rotation_degrees: decoderRotationDegrees,
                mirror_horizontal: decoderMirrorHorizontal
              }
            }
          : {
              state: 'disabled'
            },
        host_buffer: Number.isFinite(hostBuffer) && hostBuffer > 0 ? hostBuffer : streamDefaults.defaultHostBuffer,
        preview_jpeg_quality: streamDefaults.defaultPreviewJpegQuality,
        recording_mode: normalizeRecordingMode(streamDefaults.defaultRecordingMode),
        start_on_boot: streamDefaults.defaultStartOnBoot
      });

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
      <div>
        <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Register stream</p>
        <p class="text-xl font-semibold text-surface-50">
          {registerExperience === 'simple' ? 'Quick camera setup' : 'Pick a camera, backend, and mode'}
        </p>
        <p class="text-sm text-surface-400">
          {registerExperience === 'simple'
            ? 'Choose camera, stream type, resolution, and optional pipeline/template.'
            : 'Detected devices from /peripherals with a fallback to /streams/backends.'}
        </p>
        {#if submitError}
          <div class="mt-4 space-y-3 rounded-lg border border-error-500/30 bg-error-500/10 px-4 py-3">
            <div>
              <p class="text-[0.7rem] font-semibold uppercase tracking-[0.24em] text-error-100">Last start attempt failed</p>
              <p class="mt-2 text-sm text-surface-100">{submitError}</p>
            </div>
            {#if submitValidationIssues.length}
              <ValidationIssueList issues={submitValidationIssues} title="Stream incompatibilities" compact />
            {/if}
          </div>
        {/if}
        <div class="mt-3 inline-flex rounded-md border border-surface-700 bg-surface-950/70 p-1">
          <button
            type="button"
            class={`rounded px-3 py-1.5 text-xs font-semibold uppercase tracking-[0.2em] transition ${
              registerExperience === 'simple'
                ? 'bg-primary-500/20 text-primary-100'
                : 'text-surface-400 hover:text-surface-100'
            }`}
            onclick={() => handleRegisterExperienceChange('simple')}
          >
            Simple
          </button>
          <button
            type="button"
            class={`rounded px-3 py-1.5 text-xs font-semibold uppercase tracking-[0.2em] transition ${
              registerExperience === 'advanced'
                ? 'bg-primary-500/20 text-primary-100'
                : 'text-surface-400 hover:text-surface-100'
            }`}
            onclick={() => handleRegisterExperienceChange('advanced')}
          >
            Advanced
          </button>
        </div>
      </div>
    {/snippet}
    {#snippet actions()}
      {#if registerExperience === 'advanced'}
        <button
          class="btn preset-filled-primary-500"
          type="button"
          onclick={() => (showSensorBenchModal = true)}
          disabled={!currentDevice() || !currentBackend()}
          title="Benchmark all sensor modes before registering"
        >
          <span class="inline-flex items-center gap-2">
            <svg class="h-4 w-4" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
              <path
                d="M3 5a2 2 0 0 1 2-2h3a1 1 0 1 1 0 2H5v14h14v-3a1 1 0 1 1 2 0v3a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5Zm12.293 2.293a1 1 0 0 1 1.414 0L20.414 11a1 1 0 0 1 0 1.414l-6.793 6.793a1 1 0 0 1-.707.293H10a1 1 0 0 1-1-1v-2.914a1 1 0 0 1 .293-.707l6-6ZM11 16.086V17.5h1.414l5.5-5.5L16.5 10.586l-5.5 5.5Z"
              />
            </svg>
            Sensor bench
          </span>
        </button>
      {/if}
      <button class="btn btn-ghost" type="button" onclick={close}>Close</button>
    {/snippet}
      {#if devices.length === 0}
        <div class="rounded-lg border border-surface-800 bg-surface-950/70 px-4 py-6 text-sm text-surface-300">
          No cameras detected. Ensure the engine is running and the device is connected, then retry.
          <div class="mt-4 flex gap-2">
            <button class="btn btn-ghost" type="button" onclick={close}>Close</button>
            <button class="btn preset-filled-primary-500" type="button" onclick={loadDevices}>Retry</button>
          </div>
        </div>
      {:else}
        {#if registerExperience === 'simple'}
          <div class="grid gap-6 lg:grid-cols-[1.15fr_1fr]">
            <div class="space-y-4">
              <RegisterCameraDeviceList
                devices={devices}
                selectedIndex={selectedDeviceIndex}
                isRegistered={isRegistered}
                onSelect={handleDeviceChange}
              />
            </div>
            <div class="space-y-4 rounded-lg border border-surface-800 bg-surface-950/60 p-4">
              <div class="flex items-center justify-between">
                <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Simple setup</span>
                {#if isRegistered(currentDevice())}
                  <span class="rounded-full bg-warning-500/20 px-2 py-0.5 text-micro font-semibold uppercase tracking-[0.15em] text-warning-100">
                    Already registered
                  </span>
                {/if}
              </div>

              <div class="space-y-2">
                <p class="text-micro uppercase tracking-[0.2em] text-surface-500">Stream type</p>
                <div class="flex flex-wrap gap-2">
                  <button
                    type="button"
                    class={`rounded-md border px-3 py-2 text-sm transition ${
                      simpleStreamKind === 'bw'
                        ? 'border-primary-400 bg-primary-500/10 text-primary-50'
                        : 'border-surface-800 bg-surface-900 text-surface-200 hover:border-surface-700'
                    }`}
                    onclick={() => handleSimpleStreamKindChange('bw')}
                  >
                    B/W (N12)
                  </button>
                  <button
                    type="button"
                    class={`rounded-md border px-3 py-2 text-sm transition ${
                      simpleStreamKind === 'color'
                        ? 'border-primary-400 bg-primary-500/10 text-primary-50'
                        : 'border-surface-800 bg-surface-900 text-surface-200 hover:border-surface-700'
                    }`}
                    onclick={() => handleSimpleStreamKindChange('color')}
                  >
                    Colour (YUYV)
                  </button>
                </div>
              </div>

              <div class="space-y-2">
                <p class="text-micro uppercase tracking-[0.2em] text-surface-500">Resolution</p>
                {#if simpleResolutionModes.length}
                  <div class="flex flex-wrap gap-2">
                    {#each simpleResolutionModes as mode, index (`${resolutionKey(mode) ?? mode.id ?? `${mode.format?.resolution?.width ?? 0}x${mode.format?.resolution?.height ?? 0}`}:${index}`)}
                      <button
                        type="button"
                        class={`rounded-md border px-3 py-2 text-sm transition ${
                          resolutionKey(mode) === selectedResolutionKey
                            ? 'border-primary-400 bg-primary-500/10 text-primary-50'
                            : 'border-surface-800 bg-surface-900 text-surface-200 hover:border-surface-700'
                        }`}
                        onclick={() => handleResolutionChange(resolutionKey(mode))}
                      >
                        {resolutionLabel(mode)}
                      </button>
                    {/each}
                  </div>
                {:else}
                  <p class="text-sm text-warning-100">
                    No {simpleStreamKind === 'bw' ? 'N12/NV12' : 'YUYV'} modes were reported for this camera.
                  </p>
                {/if}
              </div>

              <div class="space-y-2">
                <label class="text-micro uppercase tracking-[0.2em] text-surface-500" for="simple-stream-attach">
                  Template / pipeline
                </label>
                <select
                  id="simple-stream-attach"
                  class="select w-full bg-surface-950"
                  value={simpleAttachSelection}
                  onchange={handleSimpleAttachSelectionEvent}
                >
                  <option value="none">None (raw stream)</option>
                  {#if availablePipelines.length}
                    <optgroup label="Pipelines">
                      {#each availablePipelines as entry, index (`${entry.id}:${index}`)}
                        <option value={`pipeline:${entry.id}`}>{pipelineDisplayName(entry)}</option>
                      {/each}
                    </optgroup>
                  {/if}
                  {#if availableTemplates.length}
                    <optgroup label="Templates">
                      {#each availableTemplates as entry, index (`${entry.templateId}:${index}`)}
                        <option value={`template:${entry.templateId}`}>{entry.name}</option>
                      {/each}
                    </optgroup>
                  {/if}
                </select>
                <p class="text-xs text-surface-500">
                  Selecting a template creates a persisted pipeline from that template before stream registration.
                </p>
              </div>

              <div class="flex justify-end gap-2 pt-2">
                <button class="btn btn-ghost" type="button" onclick={close} disabled={submitting}>Cancel</button>
                <button
                  class="btn preset-filled-primary-500"
                  type="button"
                  onclick={submit}
                  disabled={submitting || !simpleCanSubmit}
                >
                  {submitting ? 'Registering…' : 'Register stream'}
                </button>
              </div>
            </div>
          </div>
        {:else}
          <div class="grid gap-6 lg:grid-cols-[1.15fr_1fr]">
            <div class="space-y-4">
              <RegisterCameraDeviceList
                devices={devices}
                selectedIndex={selectedDeviceIndex}
                isRegistered={isRegistered}
                onSelect={handleDeviceChange}
              />
              <RegisterCameraBackendMode
                device={currentDevice()}
                selectedBackendIndex={selectedBackendIndex}
                selectedFormat={selectedFormat}
                selectedResolutionKey={selectedResolutionKey}
                selectedInterval={selectedInterval}
                currentModes={currentModes}
                formats={formats}
                resolutionsForSelectedFormat={resolutionsForSelectedFormat}
                intervalsForSelected={intervalsForSelected}
                formatLabel={formatLabel}
                resolutionLabel={resolutionLabel}
                resolutionKey={resolutionKey}
                intervalToFps={intervalToFps}
                onBackendChange={handleBackendChange}
                onFormatChange={handleFormatChange}
                onResolutionChange={handleResolutionChange}
                onIntervalChange={handleIntervalChange}
              />
            </div>
            <RegisterCameraStreamSettings
              bind:alias={alias}
              bind:decoderImpl={decoderImpl}
              bind:encoderImpl={encoderImpl}
              bind:decoderRotationDegrees={decoderRotationDegrees}
              bind:decoderMirrorHorizontal={decoderMirrorHorizontal}
              bind:hostBuffer={hostBuffer}
              bind:fpsLimit={fpsLimit}
              bind:showAdvancedSettings={showAdvancedSettings}
              isRegistered={isRegistered}
              currentDevice={currentDevice}
              currentBackend={currentBackend}
              isFileBackend={isFileBackend}
              showNetcamWarning={showNetcamWarning}
              decodersForFormat={decodersForFormat}
              codecs={codecs}
              currentEncoder={currentEncoder}
              encoderSettingsAvailable={encoderSettingsAvailable}
              formatLabel={formatLabel}
              showSensorBenchModal={showSensorBenchModal}
              showSensorBenchResults={showSensorBenchResults}
              sensorBenchError={sensorBenchError}
              sensorBenchLoading={sensorBenchLoading}
              sensorBenchRuns={sensorBenchRuns}
              sensorBenchSelection={sensorBenchSelection}
              sensorBenchBestDecoder={sensorBenchBestDecoder}
              sensorBenchBestEncoder={sensorBenchBestEncoder}
              fmtCpuDelta={fmtCpuDelta}
              submitting={submitting}
              onOpenEncoderSettings={() => (encoderSettingsModalOpen = true)}
              onToggleBenchModal={(open) => (showSensorBenchModal = open)}
              onToggleBenchResults={(open) => (showSensorBenchResults = open)}
              onCancel={close}
              onSubmit={submit}
            />
          </div>
        {/if}
      {/if}
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
