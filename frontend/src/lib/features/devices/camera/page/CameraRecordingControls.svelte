<script lang="ts">
  import { onDestroy } from 'svelte';
  import { toaster } from '$lib';
  import { apiFetchResponse } from '$lib/api/core/http';
  import type { StreamInfo, StreamManifest, StreamPipelineBinding } from '$lib/api/httpClient';
  import { streamRecordingActive } from '$lib/api/streamRuntime';
  import { recordingModeEnabled } from '$lib/api/streamRecordingMode';
  import FaIcon from '$lib/components/icons/FaIcon.svelte';
  import { emitMediaMutation } from '$lib/features/media/mutations';
  import { reportError } from '$lib/ui/errorPolicy';
  import { backendFeatures } from '$lib/api/backendFeatures';
  import { extractGraphOutputPortTypes, filterEncoderCompatibleOutputs } from '$lib/features/pipelines/outputFilters';
  import { faCamera, faCircle, faClock, faGear, faStop, faTriangleExclamation } from '@fortawesome/free-solid-svg-icons';

  type PipelineBindingLike = StreamPipelineBinding & {
    pipelineId?: string | null;
    id?: string | null;
    pipelineGraph?: unknown;
    graph?: unknown;
  };

  type RecordingManifest = StreamManifest & {
    pipeline_id?: string | null;
    pipeline_output?: string | null;
    pipelines?: PipelineBindingLike[] | null;
  };

  type PipelineState = {
    selectedPipelineId?: string | null;
    assignedPipelineIds?: string[] | null;
    pipelineGridSlots?: Record<string, string | null> | null;
  };

  type PipelineOutputOptionsCache = Map<string, string[]> | Record<string, string[]>;

  type RecordingControlsContext = {
    stream: StreamInfo | null;
    streamId: string | null;
    activePipelineIds?: string[] | null;
    pipelineState?: PipelineState | null;
    RAW_PIPELINE_ID: string;
    RAW_PIPELINE_UUID: string;
    extractGraphOutputPorts?: ((graph: unknown) => string[]) | null;
    pipelineOutputOptionsCache?: PipelineOutputOptionsCache | (() => PipelineOutputOptionsCache | null | undefined) | null;
    pipelineLabel?: ((pipelineId: string) => string) | null;
    apiPath: (path: string) => string;
    refresh?: (() => Promise<void> | void) | null;
    ensurePipelineOutputsLoaded?: ((pipelineId: string) => void) | null;
  };

  const { ctx } = $props<{ ctx: RecordingControlsContext }>();

  const asRecord = (value: unknown): Record<string, unknown> | null =>
    value && typeof value === 'object' ? (value as Record<string, unknown>) : null;

  const dedupePipelineIds = (values: Iterable<string>): string[] => {
    const out: string[] = [];
    const seen = new Set<string>();
    for (const value of values) {
      const normalized = typeof value === 'string' ? value.trim() : '';
      if (!normalized || seen.has(normalized)) continue;
      seen.add(normalized);
      out.push(normalized);
    }
    return out;
  };

  const manifestFor = (stream: StreamInfo | null): RecordingManifest | null => {
    const manifest = stream?.manifest ?? null;
    return manifest ? (manifest as RecordingManifest) : null;
  };

  const pipelineBindingsFor = (manifest: RecordingManifest | null): PipelineBindingLike[] =>
    Array.isArray(manifest?.pipelines) ? manifest.pipelines.filter((entry): entry is PipelineBindingLike => Boolean(entry)) : [];

  const DEFAULT_RECORDING_FPS: number | null = null;
  const DEFAULT_RECORDING_CODEC: 'h264' | 'h265' = 'h264';
  const DEFAULT_RECORDING_FORMAT: 'mp4' | 'raw' = 'raw';
  const DEFAULT_RECORDING_BITRATE_BPS: number | null = null;
  const DEFAULT_RECORDING_GOP: number | null = null;
  const DEFAULT_RECORDING_QUALITY: number | null = null;
  const DEFAULT_RECORDING_SOURCE: 'multiplex' | 'pipeline' | 'raw' | 'undistorted' = 'multiplex';
  const DEFAULT_RECORDING_INCLUDE_IMU = true;

  let recordingActive = $state(false);
  let recordingBusy = $state(false);
  let snapshotBusy = $state(false);
  let recordingConfigOpen = $state(false);
  let recordingTimer: ReturnType<typeof setTimeout> | null = null;
  let recordingSource = $state<'multiplex' | 'pipeline' | 'raw' | 'undistorted'>(DEFAULT_RECORDING_SOURCE);
  let recordingPipelineId = $state<string | null>(null);
  let recordingOutputKey = $state<string | null>(null);
  let recordingFps = $state<number | null>(DEFAULT_RECORDING_FPS);
  let recordingCodec = $state<'h264' | 'h265'>(DEFAULT_RECORDING_CODEC);
  let recordingFormat = $state<'mp4' | 'raw'>(DEFAULT_RECORDING_FORMAT);
  let recordingBitrate = $state<number | null>(DEFAULT_RECORDING_BITRATE_BPS);
  let recordingGop = $state<number | null>(DEFAULT_RECORDING_GOP);
  let recordingQuality = $state<number | null>(DEFAULT_RECORDING_QUALITY);
  let recordingIncludeImu = $state<boolean>(DEFAULT_RECORDING_INCLUDE_IMU);
  const recordingLive = $derived(streamRecordingActive(ctx.stream) || recordingActive);
  const activePipelineIds = $derived.by(() => {
    const existing: string[] = Array.isArray(ctx.activePipelineIds) ? ctx.activePipelineIds : [];
    const filteredExisting = dedupePipelineIds(existing).filter((id) => id && id !== ctx.RAW_PIPELINE_ID);
    if (filteredExisting.length) return filteredExisting;
    const pipelineState = ctx.pipelineState ?? null;
    if (pipelineState) {
      const collected = new Set<string>();
      const add = (value: unknown) => {
        const raw = typeof value === 'string' ? value.trim() : '';
        if (!raw) return;
        const normalized = raw === ctx.RAW_PIPELINE_UUID ? ctx.RAW_PIPELINE_ID : raw;
        if (normalized === ctx.RAW_PIPELINE_ID) return;
        collected.add(normalized);
      };
      add(pipelineState.selectedPipelineId);
      (pipelineState.assignedPipelineIds ?? []).forEach((id: string) => add(id));
      Object.values(pipelineState.pipelineGridSlots ?? {}).forEach((id: string | null) => add(id));
      if (collected.size) return dedupePipelineIds(collected);
    }
    const manifest = ctx.stream?.manifest ?? null;
    const manifestState = manifestFor(ctx.stream);
    const collected = new Set<string>();
    const add = (value: unknown) => {
      const raw = typeof value === 'string' ? value.trim() : '';
      if (!raw) return;
      const normalized = raw === ctx.RAW_PIPELINE_UUID ? ctx.RAW_PIPELINE_ID : raw;
      if (normalized === ctx.RAW_PIPELINE_ID) return;
      collected.add(normalized);
    };
    void manifest;
    add(manifestState?.pipeline_id);
    add(manifestState?.active_pipeline_id);
    pipelineBindingsFor(manifestState).forEach((entry) => add(entry.pipeline_id ?? entry.pipelineId ?? entry.id));
    return dedupePipelineIds(collected);
  });
  const manifestOutputOptionsById = $derived.by(() => {
    const outputsById: Record<string, string[]> = {};
    const manifest = manifestFor(ctx.stream);
    const extractor = ctx.extractGraphOutputPorts;
    if (!manifest || typeof extractor !== 'function') return outputsById;
    const normalizeId = (value: unknown) => {
      const raw = typeof value === 'string' ? value.trim() : '';
      if (!raw) return null;
      const normalized = raw === ctx.RAW_PIPELINE_UUID ? ctx.RAW_PIPELINE_ID : raw;
      if (normalized === ctx.RAW_PIPELINE_ID) return null;
      return normalized;
    };
    const readGraph = (value: unknown) => {
      const record = asRecord(value);
      return record?.pipeline_graph ?? record?.pipelineGraph ?? record?.graph ?? null;
    };
    const addOutputs = (id: unknown, graph: unknown, fallback?: unknown) => {
      const normalized = normalizeId(id);
      if (!normalized || outputsById[normalized]?.length) return;
      if (graph) {
        const outputs = extractor(graph);
        if (outputs.length) {
          const types = extractGraphOutputPortTypes(graph);
          outputsById[normalized] = filterEncoderCompatibleOutputs(outputs, types);
          return;
        }
      }
      if (typeof fallback === 'string') {
        const trimmed = fallback.trim();
        if (trimmed.length) outputsById[normalized] = [trimmed];
      }
    };
    addOutputs(
      manifest.active_pipeline_id ?? manifest.pipeline_id,
      readGraph(manifest),
      manifest.active_pipeline_output ?? manifest.pipeline_output
    );
    pipelineBindingsFor(manifest).forEach((entry) => {
      addOutputs(entry.pipeline_id ?? entry.pipelineId ?? entry.id, readGraph(entry), entry.pipeline_output);
    });
    return outputsById;
  });

  const pipelineGroups = $derived.by(() => {
    const rawCache = ctx.pipelineOutputOptionsCache;
    const cache = typeof rawCache === 'function' ? rawCache() : rawCache;
    const isMap = cache instanceof Map;
    const hasKey = (id: string) => {
      if (isMap) return (cache as Map<string, string[]>).has(id);
      if (cache && typeof cache === 'object') return Object.prototype.hasOwnProperty.call(cache, id);
      return false;
    };
    const readOutputs = (id: string): string[] | undefined => {
      if (isMap) return (cache as Map<string, string[]>).get(id);
      if (cache && typeof cache === 'object') return (cache as Record<string, string[]>)[id];
      return undefined;
    };
    return dedupePipelineIds(activePipelineIds)
      .filter((pipelineId) => pipelineId.length && pipelineId !== ctx.RAW_PIPELINE_ID)
      .map((pipelineId) => {
        const cached = readOutputs(pipelineId);
        const hasCached = cached != null;
        const fallback = manifestOutputOptionsById[pipelineId];
        const outputs = hasCached ? (Array.isArray(cached) ? cached : []) : Array.isArray(fallback) ? fallback : [];
        const loaded = hasCached || Array.isArray(fallback);
        return {
          id: pipelineId,
          label: typeof ctx.pipelineLabel === 'function' ? ctx.pipelineLabel(pipelineId) : pipelineId,
          outputs,
          loaded: loaded || hasKey(pipelineId)
        };
      });
  });
  let shadowRecorderSupported = $state(false);
  const unsubscribeBackendFeatures = backendFeatures.subscribe((value) => {
    shadowRecorderSupported = Boolean(value?.shadowRecorder);
  });
  onDestroy(() => unsubscribeBackendFeatures());

  const shadowEnabled = $derived.by(() => {
    if (!shadowRecorderSupported) return false;
    const manifest = manifestFor(ctx.stream);
    const backend = String(manifest?.capture?.backend ?? '').trim().toLowerCase();
    if (backend === 'file') return false;
    return recordingModeEnabled(manifest?.recording_mode);
  });
  const manifestEncoderId = (manifest: ReturnType<typeof manifestFor>): string => {
    const manifestRecord =
      manifest && typeof manifest === 'object' ? (manifest as Record<string, unknown>) : null;
    const encoderRecord =
      manifestRecord?.encoder && typeof manifestRecord.encoder === 'object'
        ? (manifestRecord.encoder as Record<string, unknown>)
        : null;
    if (typeof encoderRecord?.id === 'string' && encoderRecord.id.trim().length > 0) {
      return encoderRecord.id.trim().toLowerCase();
    }
    return String(manifestRecord?.encoder_id ?? '').trim().toLowerCase();
  };
  const preferredMultiplexCodec = $derived.by(() => {
    const encoderId = manifestEncoderId(manifestFor(ctx.stream));
    if (encoderId.includes('265') || encoderId.includes('hevc')) return 'h265';
    if (encoderId.includes('264') || encoderId.includes('avc')) return 'h264';
    return null;
  });

  const captureDisabled = $derived(!ctx.streamId || recordingBusy || snapshotBusy || recordingLive || !shadowEnabled);
  const captureDisabledTitle = $derived.by(() => {
    if (!ctx.streamId) return 'Stream not available';
    if (!shadowEnabled) return 'Enable shadow recorder in stream settings to use this';
    if (recordingBusy) return 'Recording busy';
    if (snapshotBusy) return 'Snapshot busy';
    if (recordingLive) return 'Stop recording before capturing';
    return '';
  });

  const encodePipelineSelection = (pipelineId: string, outputKey: string | null): string =>
    JSON.stringify({ kind: 'pipeline', pipeline_id: pipelineId, output_key: outputKey });

  const recordingSourceSelection = $derived.by(() => {
    if (recordingSource === 'raw') return 'raw';
    if (recordingSource === 'undistorted') return 'undistorted';
    if (recordingSource === 'multiplex') return 'multiplex';
    if (recordingSource === 'pipeline') {
      const pipelineId = recordingPipelineId?.trim() ?? '';
      const outputKey = recordingOutputKey?.trim() ?? '';
      return encodePipelineSelection(pipelineId, outputKey || null);
    }
    return 'multiplex';
  });

  const availablePipelineSelections = $derived.by(() => {
    const values = new Set<string>();
    pipelineGroups.forEach((group) => {
      group.outputs.forEach((output) => values.add(encodePipelineSelection(group.id, output)));
      if (!group.outputs.length) values.add(encodePipelineSelection(group.id, null));
    });
    return values;
  });

  const recordingSelectionCustomLabel = $derived.by(() => {
    if (recordingSource !== 'pipeline') return 'Custom selection';
    const pipelineId = recordingPipelineId?.trim() ?? '';
    const outputKey = recordingOutputKey?.trim() ?? '';
    if (pipelineId && outputKey) {
      const label = typeof ctx.pipelineLabel === 'function' ? ctx.pipelineLabel(pipelineId) : pipelineId;
      return `${label} · ${outputKey}`;
    }
    if (pipelineId) {
      const label = typeof ctx.pipelineLabel === 'function' ? ctx.pipelineLabel(pipelineId) : pipelineId;
      return `${label} · default`;
    }
    if (outputKey) return `Pipeline · ${outputKey}`;
    return 'Custom pipeline output';
  });

  function clearRecordingTimer() {
    if (recordingTimer) {
      clearTimeout(recordingTimer);
      recordingTimer = null;
    }
  }

  function buildRecordingPayload(durationMs?: number | null): Record<string, unknown> {
    const codec = recordingSource === 'multiplex' ? (preferredMultiplexCodec ?? recordingCodec) : recordingCodec;
    const payload: Record<string, unknown> = {
      codec,
      container: recordingFormat,
      duration_ms: durationMs ?? undefined
    };
    const sourcePayload = buildRecordingSourcePayload();
    if (sourcePayload) payload.source = sourcePayload;
    const fps = Number(recordingFps);
    if (Number.isFinite(fps) && fps > 0) payload.fps = fps;
    const bitrate = Number(recordingBitrate);
    if (Number.isFinite(bitrate) && bitrate > 0) payload.bitrate_bps = Math.round(bitrate);
    const gop = Number(recordingGop);
    if (Number.isFinite(gop) && gop > 0) payload.gop = Math.round(gop);
    const quality = Number(recordingQuality);
    if (Number.isFinite(quality) && quality >= 0) payload.quality = Math.min(51, Math.round(quality));
    payload.include_imu = recordingIncludeImu;
    return payload;
  }

  function buildRecordingSourcePayload(): Record<string, unknown> | null {
    if (recordingSource === 'multiplex') {
      return { kind: 'multiplex' };
    }
    if (recordingSource === 'raw') {
      return { kind: 'raw' };
    }
    if (recordingSource === 'undistorted') {
      const rawPipelineUuid = typeof ctx.RAW_PIPELINE_UUID === 'string' ? ctx.RAW_PIPELINE_UUID.trim() : '';
      if (rawPipelineUuid.length) {
        return { kind: 'pipeline', pipeline_id: rawPipelineUuid, output_key: 'undistorted' };
      }
      return { kind: 'raw' };
    }
    if (recordingSource === 'pipeline') {
      const payload: Record<string, unknown> = { kind: 'pipeline' };
      const pipelineId = recordingPipelineId?.trim() ?? '';
      const outputKey = recordingOutputKey?.trim() ?? '';
      if (pipelineId.length) payload.pipeline_id = pipelineId;
      if (outputKey.length) payload.output_key = outputKey;
      return payload;
    }
    return null;
  }

  function applyRecordingSelection(value: string) {
    if (value === 'multiplex') {
      recordingSource = 'multiplex';
      recordingPipelineId = null;
      recordingOutputKey = null;
      return;
    }
    if (value === 'raw') {
      recordingSource = 'raw';
      recordingPipelineId = null;
      recordingOutputKey = null;
      return;
    }
    if (value === 'undistorted') {
      recordingSource = 'undistorted';
      recordingPipelineId = null;
      recordingOutputKey = null;
      return;
    }
    if (value.startsWith('{')) {
      try {
        const parsed = JSON.parse(value);
        if (parsed && parsed.kind === 'pipeline') {
          const pipelineId = typeof parsed.pipeline_id === 'string' ? parsed.pipeline_id.trim() : '';
          const outputKey = typeof parsed.output_key === 'string' ? parsed.output_key.trim() : '';
          recordingSource = 'pipeline';
          recordingPipelineId = pipelineId.length ? pipelineId : null;
          recordingOutputKey = outputKey.length ? outputKey : null;
        }
      } catch {
        // ignore invalid selection payload
      }
    }
  }

  async function startRecording(durationMs?: number | null, label?: string) {
    const streamId = ctx.stream?.id ?? ctx.streamId ?? null;
    if (!streamId || recordingBusy || snapshotBusy) return;
    recordingBusy = true;
    try {
      const payload = buildRecordingPayload(durationMs);
      const response = await apiFetchResponse(ctx.apiPath(`/streams/${encodeURIComponent(streamId)}/recording/start`), {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Accept: 'application/json' },
        body: JSON.stringify(payload)
      });
      if (!response.ok) {
        const text = await response.text().catch(() => '');
        throw new Error(text || `Failed to start recording (${response.status})`);
      }
      recordingActive = true;
      if (typeof ctx.refresh === 'function') {
        void ctx.refresh();
      }
      const desc = label ?? (durationMs ? `Auto-stop in ${Math.round(durationMs / 1000)}s` : 'Saving to media');
      toaster.success({ title: 'Recording started', description: desc });
      if (durationMs && durationMs > 0) {
        clearRecordingTimer();
        recordingTimer = setTimeout(() => {
          void stopRecording({ auto: true });
        }, durationMs + 250);
      }
    } catch (error) {
      reportError({ title: 'Recording failed', error });
    } finally {
      recordingBusy = false;
    }
  }

  async function stopRecording(options: { auto?: boolean } = {}) {
    const streamId = ctx.stream?.id ?? ctx.streamId ?? null;
    if (!streamId || recordingBusy || snapshotBusy) return;
    recordingBusy = true;
    clearRecordingTimer();
    try {
      const response = await apiFetchResponse(ctx.apiPath(`/streams/${encodeURIComponent(streamId)}/recording/stop`), { method: 'POST' });
      if (!response.ok) {
        const text = await response.text().catch(() => '');
        throw new Error(text || `Failed to stop recording (${response.status})`);
      }
      recordingActive = false;
      if (typeof ctx.refresh === 'function') {
        void ctx.refresh();
      }
      emitMediaMutation({
        mutation: 'created',
        mediaKind: 'video',
        cameraSource: streamId
      });
      toaster.success({
        title: options.auto ? 'Recording complete' : 'Recording stopped',
        description: 'Saved to media'
      });
    } catch (error) {
      reportError({ title: 'Stop failed', error });
    } finally {
      recordingBusy = false;
    }
  }

  async function toggleRecording() {
    if (recordingLive) {
      await stopRecording();
    } else {
      await startRecording(null);
    }
  }

  async function captureWindow(seconds: number) {
    const streamId = ctx.stream?.id ?? ctx.streamId ?? null;
    if (!streamId || recordingBusy || snapshotBusy || !shadowEnabled) return;
    recordingBusy = true;
    try {
      const durationMs = Math.max(1, Math.round(seconds)) * 1000;
      const payload: Record<string, unknown> = { window_ms: durationMs, container: recordingFormat };
      const response = await apiFetchResponse(ctx.apiPath(`/streams/${encodeURIComponent(streamId)}/recording/capture`), {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Accept: 'application/json' },
        body: JSON.stringify(payload)
      });
      if (!response.ok) {
        const text = await response.text().catch(() => '');
        throw new Error(text || `Failed to capture recording (${response.status})`);
      }
      const item = await response.json().catch(() => null);
      const sizeBytes =
        typeof item?.size_bytes === 'number'
          ? item.size_bytes
          : Number.isFinite(Number(item?.size_bytes))
            ? Number(item?.size_bytes)
            : 0;
      if (!sizeBytes || sizeBytes <= 0) {
        throw new Error('Capture produced empty output. Check that the shadow recorder is running and the device software is up to date.');
      }
      if (typeof ctx.refresh === 'function') {
        void ctx.refresh();
      }
      emitMediaMutation({
        mutation: 'created',
        mediaKind: 'video',
        cameraSource: streamId,
        mediaId: typeof item?.name === 'string' ? item.name : undefined
      });
      const label = item?.name ? `Saved ${item.name}` : 'Saved to media';
      toaster.success({ title: `Captured ${seconds}s`, description: label });
    } catch (error) {
      reportError({ title: 'Capture failed', error });
    } finally {
      recordingBusy = false;
    }
  }

  async function takeSnapshot() {
    const streamId = ctx.stream?.id ?? ctx.streamId ?? null;
    if (!streamId || recordingBusy || snapshotBusy) return;
    snapshotBusy = true;
    try {
      const payload: Record<string, unknown> = {};
      const sourcePayload = buildRecordingSourcePayload();
      if (sourcePayload) payload.source = sourcePayload;
      const response = await apiFetchResponse(ctx.apiPath(`/streams/${encodeURIComponent(streamId)}/snapshot`), {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Accept: 'application/json' },
        body: JSON.stringify(payload)
      });
      if (!response.ok) {
        const text = await response.text().catch(() => '');
        throw new Error(text || `Failed to capture snapshot (${response.status})`);
      }
      const item = await response.json().catch(() => null);
      if (typeof ctx.refresh === 'function') {
        void ctx.refresh();
      }
      emitMediaMutation({
        mutation: 'created',
        mediaKind: 'image',
        cameraSource: streamId,
        mediaId: typeof item?.name === 'string' ? item.name : undefined
      });
      const label = item?.name ? `Saved ${item.name}` : 'Saved to media';
      toaster.success({ title: 'Snapshot captured', description: label });
    } catch (error) {
      reportError({ title: 'Snapshot failed', error });
    } finally {
      snapshotBusy = false;
    }
  }

  function handleRecordingSourceSelection(event: Event): void {
    const target = event.target;
    if (!(target instanceof HTMLSelectElement)) {
      return;
    }
    applyRecordingSelection(target.value);
  }

  function readRecordingNumber(event: Event): number | null {
    const target = event.currentTarget;
    if (!(target instanceof HTMLInputElement)) {
      return null;
    }
    const value = Number(target.value);
    return Number.isFinite(value) ? value : null;
  }

  function handleRecordingFpsInput(event: Event): void {
    const value = readRecordingNumber(event);
    recordingFps = value !== null && value > 0 ? value : null;
  }

  function handleRecordingBitrateInput(event: Event): void {
    const value = readRecordingNumber(event);
    recordingBitrate = value !== null && value > 0 ? value : null;
  }

  function handleRecordingGopInput(event: Event): void {
    const value = readRecordingNumber(event);
    recordingGop = value !== null && value > 0 ? value : null;
  }

  function handleRecordingQualityInput(event: Event): void {
    const value = readRecordingNumber(event);
    recordingQuality = value !== null && value >= 0 ? Math.min(51, value) : null;
  }

  onDestroy(() => {
    clearRecordingTimer();
  });

  $effect(() => {
    const loader = ctx.ensurePipelineOutputsLoaded;
    if (typeof loader !== 'function') return;
    activePipelineIds.forEach((pipelineId) => {
      if (!pipelineId) return;
      loader(pipelineId);
    });
  });
</script>

<div class="flex flex-wrap items-center gap-3">
  <div class="flex w-full flex-wrap items-center gap-2 sm:w-auto sm:flex-nowrap sm:gap-3">
    <select
      class="select select-2xs h-8 w-full max-w-full flex-none leading-none text-micro-tight uppercase tracking-[0.3em] text-surface-100 sm:w-56"
      aria-label="Recording source"
      value={recordingSourceSelection}
      onchange={handleRecordingSourceSelection}
    >
      <optgroup label="Stream">
        <option value="multiplex">Multiplex output</option>
        <option value="raw">Raw output</option>
        <option value="undistorted">Raw output (undistorted)</option>
      </optgroup>
      {#if pipelineGroups.length}
      {#each pipelineGroups as group (group.id)}
        <optgroup label={group.label}>
          {#if group.loaded && group.outputs.length}
            {#each group.outputs as output (output)}
              <option value={encodePipelineSelection(group.id, output)}>{output}</option>
            {/each}
          {:else if !group.loaded}
            <option value={encodePipelineSelection(group.id, null)} disabled>Loading outputs…</option>
          {:else}
            <option value={encodePipelineSelection(group.id, null)} disabled>No image outputs</option>
          {/if}
        </optgroup>
      {/each}
      {:else}
        <optgroup label="Pipelines">
          <option value="pipeline:none" disabled>No pipelines attached</option>
        </optgroup>
      {/if}
      {#if recordingSource === 'pipeline' && !availablePipelineSelections.has(recordingSourceSelection)}
        <optgroup label="Custom">
          <option value={recordingSourceSelection}>{recordingSelectionCustomLabel}</option>
        </optgroup>
      {/if}
    </select>
  <button
    class={`btn btn-2xs h-8 flex-none uppercase tracking-[0.3em] font-semibold ${recordingLive ? 'preset-filled-error-500' : 'preset-filled-primary-500'}`}
    type="button"
    disabled={!ctx.streamId || recordingBusy || snapshotBusy}
    onclick={takeSnapshot}
  >
      <span class="mr-1 inline-flex items-center">
        <FaIcon icon={faCamera} class="h-3 w-3" />
      </span>
      {snapshotBusy ? 'Working…' : 'Snapshot'}
    </button>
  <button
    class={`btn btn-2xs h-8 flex-none uppercase tracking-[0.3em] font-semibold ${recordingLive ? 'preset-filled-error-500' : 'preset-filled-primary-500'}`}
    type="button"
    disabled={!ctx.streamId || recordingBusy || snapshotBusy}
    onclick={toggleRecording}
  >
      <span class="mr-1 inline-flex items-center">
        <FaIcon icon={recordingLive ? faStop : faCircle} class="h-3 w-3" />
      </span>
      {recordingBusy ? 'Working…' : recordingLive ? 'Stop' : 'Record'}
    </button>
  </div>
  {#if shadowRecorderSupported}
    <button
      class="btn btn-2xs h-8 uppercase tracking-[0.3em] border border-primary-500/40 bg-primary-500/10 text-primary-100 hover:bg-primary-500/20"
      type="button"
      disabled={captureDisabled}
      title={captureDisabledTitle}
      onclick={() => captureWindow(5)}
    >
      <span class="mr-1 inline-flex items-center">
        <FaIcon icon={faClock} class="h-3 w-3" />
      </span>
      5s
    </button>
    <button
      class="btn btn-2xs h-8 uppercase tracking-[0.3em] border border-primary-500/40 bg-primary-500/10 text-primary-100 hover:bg-primary-500/20"
      type="button"
      disabled={captureDisabled}
      title={captureDisabledTitle}
      onclick={() => captureWindow(30)}
    >
      <span class="mr-1 inline-flex items-center">
        <FaIcon icon={faClock} class="h-3 w-3" />
      </span>
      30s
    </button>
    <button
      class="btn btn-2xs h-8 uppercase tracking-[0.3em] border border-primary-500/40 bg-primary-500/10 text-primary-100 hover:bg-primary-500/20"
      type="button"
      disabled={captureDisabled}
      title={captureDisabledTitle}
      onclick={() => captureWindow(60)}
    >
      <span class="mr-1 inline-flex items-center">
        <FaIcon icon={faClock} class="h-3 w-3" />
      </span>
      1m
    </button>
  {/if}
  <button
    class="btn btn-2xs h-8 preset-tonal text-surface-100"
    type="button"
    aria-label="Recording settings"
    title="Recording settings"
    onclick={() => (recordingConfigOpen = true)}
  >
    <FaIcon icon={faGear} class="h-3.5 w-3.5" />
	  </button>
	  {#if shadowRecorderSupported && !shadowEnabled}
	    <span class="group relative inline-flex">
	      <button
	        class="inline-flex h-8 w-8 items-center justify-center rounded border border-amber-500/60 bg-amber-500/10 text-amber-200 cursor-help"
	        type="button"
	        aria-label="Enable shadow recorder to capture clips"
	      >
	        <FaIcon icon={faTriangleExclamation} class="h-3.5 w-3.5" />
	      </button>
	      <span
	        class="pointer-events-none absolute left-1/2 top-full z-50 mt-2 w-max max-w-[18rem] -translate-x-1/2 translate-y-1 rounded border border-surface-700/80 bg-surface-950 px-2 py-1 text-micro-tight font-medium text-surface-100 opacity-0 shadow-lg shadow-black/40 transition group-hover:translate-y-0 group-hover:opacity-100 group-focus-within:translate-y-0 group-focus-within:opacity-100"
	        role="tooltip"
	      >
	        Enable shadow recorder to capture clips
	      </span>
	    </span>
	  {/if}
	</div>

{#if recordingConfigOpen}
  <div class="fixed inset-0 z-40 flex items-center justify-center bg-black/70 px-4 py-6" role="dialog" aria-modal="true" aria-label="Recording settings">
    <div class="w-full max-w-md rounded-lg border border-surface-800/70 bg-surface-950 p-4 shadow-xl">
      <div class="flex items-start justify-between gap-3">
        <div>
          <p class="text-2xs uppercase tracking-[0.3em] text-surface-500">Recording settings</p>
          <p class="mt-1 text-sm text-surface-300">Tune encoder settings used for captures.</p>
        </div>
        <button class="btn btn-3xs preset-tonal uppercase tracking-[0.3em]" type="button" onclick={() => (recordingConfigOpen = false)}>
          Close
        </button>
      </div>
      <div class="mt-4 grid gap-3 sm:grid-cols-2">
        <label class="flex flex-col gap-1 text-2xs uppercase tracking-[0.3em] text-surface-500">
          FPS
          <input
            class="mt-1 rounded border border-surface-700/70 bg-surface-900/60 px-2 py-1 text-xs text-surface-200"
            type="number"
            min="0"
            step="1"
            value={recordingFps ?? ''}
            oninput={handleRecordingFpsInput}
          />
          <span class="text-micro-tight uppercase tracking-[0.3em] text-surface-600">default auto</span>
        </label>
        <label class="flex flex-col gap-1 text-2xs uppercase tracking-[0.3em] text-surface-500">
          Codec
          <select
            class="mt-1 rounded border border-surface-700/70 bg-surface-900/60 px-2 py-1 text-xs text-surface-200"
            bind:value={recordingCodec}
          >
            <option value="h265">H.265 (HEVC)</option>
            <option value="h264">H.264 (AVC)</option>
          </select>
          <span class="text-micro-tight uppercase tracking-[0.3em] text-surface-600">default {DEFAULT_RECORDING_CODEC} (multiplex auto-matches stream)</span>
        </label>
        <label class="flex flex-col gap-1 text-2xs uppercase tracking-[0.3em] text-surface-500">
          Format
          <select
            class="mt-1 rounded border border-surface-700/70 bg-surface-900/60 px-2 py-1 text-xs text-surface-200"
            bind:value={recordingFormat}
          >
            <option value="mp4">MP4</option>
            <option value="raw">Raw (Annex B)</option>
          </select>
          <span class="text-micro-tight uppercase tracking-[0.3em] text-surface-600">default {DEFAULT_RECORDING_FORMAT}</span>
        </label>
        <label class="flex flex-col gap-1 text-2xs uppercase tracking-[0.3em] text-surface-500">
          Bitrate (bps)
          <input
            class="mt-1 rounded border border-surface-700/70 bg-surface-900/60 px-2 py-1 text-xs text-surface-200"
            type="number"
            min="0"
            step="1000"
            value={recordingBitrate ?? ''}
            oninput={handleRecordingBitrateInput}
          />
          <span class="text-micro-tight uppercase tracking-[0.3em] text-surface-600">optional (ignored for multiplex)</span>
        </label>
        <label class="flex flex-col gap-1 text-2xs uppercase tracking-[0.3em] text-surface-500">
          GOP
          <input
            class="mt-1 rounded border border-surface-700/70 bg-surface-900/60 px-2 py-1 text-xs text-surface-200"
            type="number"
            min="0"
            step="1"
            value={recordingGop ?? ''}
            oninput={handleRecordingGopInput}
          />
          <span class="text-micro-tight uppercase tracking-[0.3em] text-surface-600">optional</span>
        </label>
        <label class="flex flex-col gap-1 text-2xs uppercase tracking-[0.3em] text-surface-500">
          Quality (CRF)
          <input
            class="mt-1 rounded border border-surface-700/70 bg-surface-900/60 px-2 py-1 text-xs text-surface-200"
            type="number"
            min="0"
            max="51"
            step="1"
            value={recordingQuality ?? ''}
            oninput={handleRecordingQualityInput}
          />
          <span class="text-micro-tight uppercase tracking-[0.3em] text-surface-600">optional · lower = higher quality</span>
        </label>
        <label class="flex items-center gap-2 rounded border border-surface-800/60 bg-surface-900/40 px-2 py-2 text-2xs uppercase tracking-[0.3em] text-surface-400 sm:col-span-2">
          <input class="checkbox checkbox-xs" type="checkbox" bind:checked={recordingIncludeImu} />
          Record IMU sidecar
        </label>
      </div>
      <p class="mt-3 text-xs text-surface-500">
        Bitrate/GOP/quality apply only to MP4 and force transcode (higher CPU). Leave unset for fastest saves.
      </p>
    </div>
  </div>
{/if}
