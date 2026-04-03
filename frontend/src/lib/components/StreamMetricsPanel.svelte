<script lang="ts">
	  import { resolve } from '$app/paths';
	  import { goto } from '$app/navigation';
	  import { onDestroy } from 'svelte';
	  import type { Snippet } from 'svelte';
	  import { openStreamMetricsSocket, type StreamMetricsError, type StreamMetricsEvent } from '$lib/api/streamMetrics';
	  import { StreamsApi } from '$lib/api/streamsApi';
	  import StreamMetricsBanners from '$lib/components/StreamMetricsBanners.svelte';
	  import type { CaptureStageMetrics, CodecMetrics, StreamMetrics } from '$lib/api/client';

  type MetricsSnapshot = {
    average_fps?: number;
    average_time_ms?: number;
    last_time_ms?: number;
    work_average_time_ms?: number;
    work_last_time_ms?: number;
    sample_count?: number;
    window_size?: number;
    processed?: number;
    backpressure?: number;
    errors?: number;
  };

  let {
    captureSessionId = $bindable<string | null>(null),
    captureSessionAlias = $bindable<string | null>(null),
    showHeader = $bindable(true),
    hideHostMetrics = $bindable(false),
    showBanners = $bindable(true),
    compact = false,
    summaryBar = false,
    hostSlotFullWidth = false,
    metrics = $bindable<StreamMetrics | null>(null),
    metricsError = $bindable<StreamMetricsError | null>(null),
    metricsStaleMessage = $bindable<string | null>(null),
    hostSlot = $bindable<Snippet<[{ metrics: StreamMetrics | null }]> | null>(null)
  } = $props();

  const hasHostSlot = $derived(Boolean(hostSlot));

  let status = $state<'idle' | 'connecting' | 'live' | 'error'>('idle');
  let streamError = $state<StreamMetricsError | null>(null);
  let errorHistory = $state<StreamMetricsError[]>([]);
  let streamStartedAt = $state<number | null>(null);
  let lastUpdated = $state<number | null>(null);
  let nowTick = $state(Date.now());
  let nowTimer: ReturnType<typeof setInterval> | null = null;
  let socketCleanup: (() => void) | null = null;
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  let reconnectAttempt = $state(0);
  let isMounted = true;
  let activeReferenceKey: string | null = null;

  const WS_RECONNECT_DELAY_MS = 3_000;
  const METRICS_STALE_MS = 12_000;
  const METRICS_GRACE_MS = 9_000;

  type ComponentMetricEntry = { id: string; label: string; data: MetricsSnapshot };
	  type PipelineErrorEntry = { id: string; label: string; error: string; ageMs: number | null };
	  type PipelineFocusRequest = { pipelineId: string; nodeId?: string | null; port?: string | null };
	  const PIPELINE_FOCUS_REQUEST_KEY = 'helios.pipelines.focus_request';

  const asRecord = (value: unknown): Record<string, unknown> | null =>
    value && typeof value === 'object' ? (value as Record<string, unknown>) : null;

  const captureSnapshot = (value: CaptureStageMetrics | null | undefined): MetricsSnapshot => ({
    average_fps: value?.fps,
    average_time_ms: value?.average_time_ms,
    last_time_ms: value?.last_time_ms,
    sample_count: value?.sample_count,
    window_size: value?.sample_count
  });

  const codecSnapshot = (value: CodecMetrics | null | undefined): MetricsSnapshot => ({
    average_fps: value?.fps,
    average_time_ms: value?.average_time_ms,
    last_time_ms: value?.last_time_ms,
    work_average_time_ms: value?.work_average_time_ms,
    work_last_time_ms: value?.work_last_time_ms,
    sample_count: value?.sample_count,
    window_size: value?.sample_count,
    processed: value?.processed,
    backpressure: value?.backpressure,
    errors: value?.errors
  });

  const componentMetrics = $derived<ComponentMetricEntry[]>((() => {
    if (!metrics) {
      const fallback: ComponentMetricEntry[] = [
        { id: 'capture', label: 'Capture', data: { sample_count: 0 } },
        { id: 'decoder', label: 'Decoder', data: { sample_count: 0 } },
        { id: 'encoder', label: 'Encoder', data: { sample_count: 0 } }
      ];
      if (!hideHostMetrics) {
        fallback.push({ id: 'host', label: 'Host', data: { sample_count: 0 } });
      } else if (hasHostSlot) {
        fallback.push({ id: 'host-slot', label: 'Host', data: {} });
      }
      return fallback;
    }
    const entries: ComponentMetricEntry[] = [
      {
        id: 'capture',
        label: 'Capture',
        data: captureSnapshot(metrics.capture)
      },
      {
        id: 'decoder',
        label: 'Decoder',
        data: codecSnapshot(metrics.decoder)
      },
      {
        id: 'encoder',
        label: 'Encoder',
        data: codecSnapshot(metrics.encoder)
      }
    ];
    if (!hideHostMetrics) {
      entries.push({
        id: 'host',
        label: 'Host',
        data: captureSnapshot(metrics.host)
      });
    } else if (hasHostSlot) {
      entries.push({ id: 'host-slot', label: 'Host', data: {} });
    }
    return entries;
  })());

  const pipelineErrors = $derived<PipelineErrorEntry[]>((() => {
    const nodes = metrics?.pipeline?.nodes;
    if (!nodes) return [];
    const entries = Object.keys(nodes).flatMap((id) => {
      const node = nodes[id];
      if (id === 'graph') return [];
      if (!node?.last_error) return [];
      const label = node.node_label || node.node_type || id;
      return [{ id, label, error: node.last_error, ageMs: node.last_error_at ?? null }];
    });
    return entries.sort((a, b) => (a.ageMs ?? Number.POSITIVE_INFINITY) - (b.ageMs ?? Number.POSITIVE_INFINITY));
  })());

  const pipelineWarnings = $derived<string[]>((() => {
    const pipeline = asRecord(metrics?.pipeline);
    const warnings = pipeline?.warnings;
    if (!Array.isArray(warnings)) return [];
    return warnings.filter((warning): warning is string => typeof warning === 'string' && warning.trim().length > 0);
  })());

  function activeReference(): string | null {
    return captureSessionId ?? captureSessionAlias ?? null;
  }

  $effect(() => {
    const reference = activeReference();
    if (!reference) {
      teardown();
      activeReferenceKey = null;
      metrics = null;
      streamError = null;
      streamStartedAt = null;
      lastUpdated = null;
      status = 'idle';
      return;
    }
    if (reference === activeReferenceKey && socketCleanup) {
      return;
    }
    activeReferenceKey = reference;
    reconnectAttempt = 0;
    startStream(reference);
  });

  onDestroy(() => {
    isMounted = false;
    teardown();
    if (nowTimer) {
      clearInterval(nowTimer);
      nowTimer = null;
    }
  });

  function teardown(): void {
    if (reconnectTimer) {
      clearTimeout(reconnectTimer);
      reconnectTimer = null;
    }
    if (socketCleanup) {
      socketCleanup();
      socketCleanup = null;
    }
  }

  function scheduleReconnect(reference: string | null): void {
    if (!reference || reconnectTimer) return;
    reconnectAttempt = Math.min(reconnectAttempt + 1, 6);
    const delay = Math.min(30_000, WS_RECONNECT_DELAY_MS * 2 ** (reconnectAttempt - 1));
    reconnectTimer = setTimeout(() => {
      reconnectTimer = null;
      if (reference !== activeReference()) return;
      startStream(reference);
    }, delay);
  }

  function handleMetrics(event: StreamMetricsEvent): void {
    if (!isMounted || event.stream_id !== activeReference()) return;
    metrics = event.metrics;
    reconnectAttempt = 0;
    const now = Date.now();
    const eventTs = typeof event.timestamp_ms === 'number' ? event.timestamp_ms : now;
    // Guard against clock skew between backend and UI (e.g. device time not synced).
    lastUpdated = Math.abs(now - eventTs) > 60_000 ? now : eventTs;
    status = 'live';
    streamError = null;
  }

  function handleError(err: StreamMetricsError): void {
    if (!isMounted) return;
    if (err.stream_id && err.stream_id !== activeReference()) return;
    streamError = err;
    status = 'error';
    if (err.timestamp_ms) {
      const last = errorHistory[0];
      const same =
        last?.error === err.error &&
        last?.code === err.code &&
        last?.detail === err.detail &&
        last?.operation === err.operation &&
        last?.source === err.source;
      if (!same) {
        errorHistory = [err, ...errorHistory].slice(0, 5);
      }
    }
    scheduleReconnect(activeReference());
  }

  async function fetchSnapshot(reference: string): Promise<void> {
    try {
      const snapshot = await StreamsApi.getMetrics({ id: reference });
      if (!isMounted || reference !== activeReference()) return;
      metrics = snapshot;
      lastUpdated = Date.now();
      if (status === 'idle') {
        status = 'connecting';
      }
    } catch (err) {
      if (!isMounted || reference !== activeReference()) return;
      console.warn('stream metrics snapshot failed', err);
    }
  }

  function startStream(reference: string): void {
    teardown();
    status = 'connecting';
    streamError = null;
    streamStartedAt = Date.now();
    lastUpdated = null;
    void fetchSnapshot(reference);
    socketCleanup = openStreamMetricsSocket(
      reference,
      {
        onMetrics: handleMetrics,
        onError: handleError,
        onClose: (info) => {
          if (info?.expected) return;
          scheduleReconnect(reference);
        }
      },
      { intervalMs: 800 }
    );
  }

  function formatFps(value: number | undefined = 0): string {
    if (!Number.isFinite(value) || value <= 0) return '—';
    return `${value.toFixed(1)} fps`;
  }

  function isIdleCodecComponent(component: ComponentMetricEntry): boolean {
    if (component.id !== 'encoder' && component.id !== 'decoder') return false;
    const sampleCount = Number(component.data.sample_count ?? 0);
    const processed = Number(component.data.processed ?? 0);
    const backpressure = Number(component.data.backpressure ?? 0);
    const errors = Number(component.data.errors ?? 0);
    return sampleCount <= 0 && processed <= 0 && backpressure <= 0 && errors <= 0;
  }

  function primaryMetricLabel(component: ComponentMetricEntry): string {
    if (isIdleCodecComponent(component)) {
      return component.id === 'encoder' ? 'Idle' : 'Standby';
    }
    return formatFps(component.data.average_fps ?? 0);
  }

  function formatTimeMs(value: number | undefined = 0): string {
    if (!Number.isFinite(value) || value <= 0) return '—';
    return `${value.toFixed(2)} ms`;
  }

  function formatSamples(component: MetricsSnapshot): string {
    const sample_count = Number(component.sample_count ?? 0);
    const window_size = Number(component.window_size ?? 0);
    return sample_count > 0 ? `${sample_count}/${window_size || '?'}` : `${window_size || '?'}`;
  }

  function formatSummaryFps(value: number | undefined = 0): string {
    if (!Number.isFinite(value) || value <= 0) return '—';
    return `${value.toFixed(1)} FPS`;
  }

  function buildSummaryTooltip(label: string, component: MetricsSnapshot): string {
    const details: string[] = [];
    details.push(`${label}`);
    details.push(`Average FPS: ${formatFps(component.average_fps ?? 0)}`);
    details.push(`Average time: ${formatTimeMs(component.average_time_ms ?? 0)}`);
    details.push(`Last frame time: ${formatTimeMs(component.last_time_ms ?? 0)}`);
    details.push(`Samples/window: ${formatSamples(component)}`);
    if (component.processed !== undefined) details.push(`Processed: ${component.processed ?? '—'}`);
    if (component.work_average_time_ms !== undefined && component.work_average_time_ms > 0) {
      details.push(`Work avg: ${formatTimeMs(component.work_average_time_ms)}`);
    }
    if (component.work_last_time_ms !== undefined && component.work_last_time_ms > 0) {
      details.push(`Work last: ${formatTimeMs(component.work_last_time_ms)}`);
    }
    if (component.backpressure !== undefined) {
      details.push(`Backpressure: ${component.backpressure?.toFixed?.(2) ?? component.backpressure ?? '—'}`);
    }
    if (component.errors !== undefined) details.push(`Errors: ${component.errors ?? '—'}`);
    return details.join('\n');
  }

  const summaryComponents = $derived.by(() => componentMetrics.filter((component) => component.id !== 'host-slot'));
  const hasHostSlotComponent = $derived.by(() => componentMetrics.some((component) => component.id === 'host-slot'));

  function formatStatus(): string {
    switch (status) {
      case 'connecting':
        return 'Connecting';
      case 'live':
        return 'Live';
      case 'error':
        return 'Error';
      default:
        return 'Idle';
    }
  }

  function formatTimestamp(timestamp: number | null): string {
    if (!timestamp) return '—';
    const delta = Date.now() - timestamp;
    if (delta < 1_000) return 'just now';
    if (delta < 60_000) return `${Math.round(delta / 1_000)}s ago`;
    const date = new Date(timestamp);
    return date.toLocaleTimeString();
  }

  function formatAge(ageMs: number | null): string {
    if (ageMs === null || !Number.isFinite(ageMs)) return 'unknown age';
    if (ageMs < 1_000) return 'just now';
    if (ageMs < 60_000) return `${Math.round(ageMs / 1_000)}s ago`;
    if (ageMs < 3_600_000) return `${Math.round(ageMs / 60_000)}m ago`;
    return `${Math.round(ageMs / 3_600_000)}h ago`;
  }

  function formatErrorMessage(message: string): string {
    if (message.length <= 160) return message;
    return `${message.slice(0, 157)}...`;
  }

  function describeMetricsError(source: StreamMetricsError | null): {
    summary: string;
    detail: string | null;
    code: string | null;
    source: string | null;
    operation: string | null;
    timestamp: number | null;
  } {
    if (!source) {
      return { summary: 'Metrics stream error', detail: null, code: null, source: null, operation: null, timestamp: null };
    }
    const summary = source.error || 'Metrics stream error';
    const detail = source.detail && source.detail !== summary ? source.detail : null;
    return {
      summary,
      detail,
      code: source.code ?? null,
      source: source.source ?? null,
      operation: source.operation ?? null,
      timestamp: typeof source.timestamp_ms === 'number' ? source.timestamp_ms : null
    };
  }

  async function viewGraphForPipelineError(entry: PipelineErrorEntry) {
    if (!captureSessionId) return;
    try {
      const stream = await StreamsApi.getStream({ id: captureSessionId }).catch(() => null);
      const manifest = stream?.manifest ?? null;
      const pipelineId =
        (typeof manifest?.active_pipeline_id === 'string' && manifest.active_pipeline_id.trim()) ||
        (typeof manifest?.pipelines?.[0]?.pipeline_id === 'string' && manifest.pipelines[0].pipeline_id.trim()) ||
        null;
      if (!pipelineId) return;
      const payload: PipelineFocusRequest = { pipelineId, nodeId: entry.id, port: null };
      if (typeof window !== 'undefined') {
        window.sessionStorage.setItem(PIPELINE_FOCUS_REQUEST_KEY, JSON.stringify(payload));
      }
      await goto(resolve('/pipelines'));
    } catch (err) {
      console.warn('Failed to navigate to pipeline graph', err);
    }
  }


  const derivedStaleMessage = $derived.by(() => {
    if (!activeReference()) return null;
    if (streamError) return null;
    if (status === 'idle') return null;
    const now = nowTick;
    if (lastUpdated) {
      const age = now - lastUpdated;
      if (age > METRICS_STALE_MS) {
        return `No metrics received for ${Math.round(age / 1000)}s. The stream may be stalled or the engine crashed.`;
      }
      return null;
    }
    if (streamStartedAt && now - streamStartedAt > METRICS_GRACE_MS) {
      return 'Metrics have not reported yet. The stream may be offline or failing to start.';
    }
    return null;
  });

  $effect(() => {
    metricsError = streamError;
    metricsStaleMessage = derivedStaleMessage;
  });

  $effect(() => {
    const reference = activeReference();
    if (!reference) return;
    if (!derivedStaleMessage) return;
    scheduleReconnect(reference);
  });

  $effect(() => {
    if (!activeReference()) {
      if (nowTimer) {
        clearInterval(nowTimer);
        nowTimer = null;
      }
      return;
    }
    if (!nowTimer) {
      nowTimer = setInterval(() => {
        nowTick = Date.now();
      }, 1000);
    }
    return () => {
      if (nowTimer) {
        clearInterval(nowTimer);
        nowTimer = null;
      }
    };
  });
</script>

<div class={compact ? (summaryBar ? 'space-y-1.5' : 'space-y-2') : 'space-y-3'}>
  {#if showHeader}
    <div class="space-y-1">
      <p class="text-2xs uppercase tracking-[0.3em] text-surface-500">Stream metrics</p>
      <p class="text-micro uppercase tracking-[0.3em] text-surface-500">
        <span class="font-semibold text-surface-300">{formatStatus()}</span>
        <span class="ml-2 text-surface-600">Updated {formatTimestamp(lastUpdated)}</span>
      </p>
    </div>
  {/if}

  {#if !activeReference()}
    <p class="text-xs text-surface-500">Metrics become available once a capture session is running.</p>
  {:else}
    {#if showBanners}
      <StreamMetricsBanners error={streamError} metricsStaleMessage={derivedStaleMessage} />
    {/if}
    {#if pipelineErrors.length}
      <div class="rounded border border-error-500/40 bg-error-500/10 px-3 py-2 text-xs text-error-200">
        <div class="flex items-center justify-between gap-2">
          <p class="font-semibold text-error-100">Pipeline error detected</p>
        </div>
        {#each pipelineErrors.slice(0, 3) as entry (entry.id)}
          <p class="mt-1">
            <span class="font-semibold">{entry.label}</span>: {formatErrorMessage(entry.error)}
            <span class="text-error-300">({formatAge(entry.ageMs)})</span>
            <button
              class="ml-2 underline decoration-error-300/60 underline-offset-2 hover:text-error-50"
              type="button"
              onclick={() => void viewGraphForPipelineError(entry)}
            >
              View graph
            </button>
          </p>
        {/each}
        {#if pipelineErrors.length > 3}
          <p class="mt-1 text-error-300">+{pipelineErrors.length - 3} more nodes reporting errors</p>
        {/if}
      </div>
    {/if}
    {#if pipelineWarnings.length > 0}
      <div class="rounded border border-warning-500/40 bg-warning-500/10 px-3 py-2 text-xs text-warning-200">
        <p class="font-semibold text-warning-100">Pipeline warning</p>
        <p class="mt-1">{formatErrorMessage(pipelineWarnings[0])}</p>
      </div>
    {/if}
    {#if errorHistory.length > 1}
      <div class="rounded border border-surface-800/70 bg-surface-950/40 px-3 py-2 text-xs text-surface-200">
        <p class="font-semibold text-surface-100">Recent metrics errors</p>
        {#each errorHistory.slice(1, 4) as entry, idx (entry.timestamp_ms ?? idx)}
          {@const entryDisplay = describeMetricsError(entry)}
          <p class="mt-1 text-surface-400">
            {entryDisplay.timestamp ? formatTimestamp(entryDisplay.timestamp) : 'unknown time'} · {entryDisplay.summary}
            {#if entryDisplay.code}
              <span class="text-surface-500">({entryDisplay.code})</span>
            {/if}
          </p>
        {/each}
      </div>
    {/if}

    {#if componentMetrics.length}
      {#if summaryBar}
        <div class="grid grid-cols-1 gap-1 sm:grid-cols-3" aria-live="polite">
          {#each summaryComponents as component (component.id)}
            {@const summaryLabel = component.id === 'encoder' ? 'Encode' : component.label}
            <div class="cursor-help rounded border border-surface-800/60 bg-surface-900/70 px-2 py-1 text-[0.7rem] text-surface-300" title={buildSummaryTooltip(summaryLabel, component.data)}>
              <span class="font-semibold text-surface-100">{summaryLabel}</span>
              <span class="mx-1 text-surface-600">|</span>
              <span>{isIdleCodecComponent(component) ? primaryMetricLabel(component) : formatSummaryFps(component.data.average_fps ?? 0)}</span>
              {#if !isIdleCodecComponent(component)}
                <span class="ml-1 text-surface-500">({formatTimeMs(component.data.average_time_ms ?? 0)})</span>
              {/if}
            </div>
          {/each}
        </div>
        {#if hasHostSlotComponent}
          <div class={`rounded border border-surface-800/60 bg-surface-900/70 ${compact ? 'p-2' : 'p-3'}`}>
            {@render hostSlot?.({ metrics })}
          </div>
        {/if}
      {:else}
        <div class={compact ? 'grid gap-2 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4' : 'grid gap-3 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4'} aria-live="polite">
          {#each componentMetrics as component (component.id)}
            {#if component.id === 'host-slot'}
              <div class={`rounded border border-surface-800/60 bg-surface-900/70 ${compact ? 'p-2.5' : 'p-3'} ${hostSlotFullWidth ? 'col-span-full' : ''}`}>
                {@render hostSlot?.({ metrics })}
              </div>
            {:else}
              <div class={`rounded border border-surface-800/60 bg-surface-900/70 ${compact ? 'p-2.5' : 'p-3'}`}>
                <p class={compact ? 'text-micro-tight uppercase tracking-[0.22em] text-surface-500' : 'text-2xs uppercase tracking-[0.3em] text-surface-500'}>{component.label}</p>
                {#if compact}
                  <div class="mt-1.5 space-y-1 text-xs">
                    <div class="flex items-center justify-between text-surface-300">
                      <span>FPS</span>
                      <span class="font-semibold text-surface-50">{primaryMetricLabel(component)}</span>
                    </div>
                    <div class="flex items-center justify-between text-surface-300">
                      <span>Avg / Last</span>
                      <span class="font-semibold text-surface-50">{formatTimeMs(component.data.average_time_ms ?? 0)} / {formatTimeMs(component.data.last_time_ms ?? 0)}</span>
                    </div>
                  </div>
                  <div class="mt-1.5 flex flex-wrap gap-1 text-[0.62rem] uppercase tracking-[0.2em] text-surface-500">
                    <span class="rounded border border-surface-800/80 bg-surface-950/60 px-1.5 py-0.5">S {formatSamples(component.data)}</span>
                    {#if component.data.processed !== undefined}
                      <span class="rounded border border-surface-800/80 bg-surface-950/60 px-1.5 py-0.5">P {component.data.processed ?? '—'}</span>
                    {/if}
                    {#if component.data.work_average_time_ms !== undefined && component.data.work_average_time_ms > 0}
                      <span class="rounded border border-surface-800/80 bg-surface-950/60 px-1.5 py-0.5">W {formatTimeMs(component.data.work_average_time_ms)}</span>
                    {/if}
                    {#if component.data.backpressure !== undefined}
                      <span class="rounded border border-surface-800/80 bg-surface-950/60 px-1.5 py-0.5">B {component.data.backpressure?.toFixed?.(2) ?? component.data.backpressure ?? '—'}</span>
                    {/if}
                    {#if component.data.errors !== undefined}
                      <span class="rounded border border-surface-800/80 bg-surface-950/60 px-1.5 py-0.5">E {component.data.errors ?? '—'}</span>
                    {/if}
                  </div>
                {:else}
                  <dl class="mt-2 space-y-1 text-sm">
                    <div class="flex justify-between text-surface-300">
                      <dt>Average FPS</dt>
                      <dd class="font-semibold text-surface-50">{primaryMetricLabel(component)}</dd>
                    </div>
                    <div class="flex justify-between text-surface-300">
                      <dt>Average time</dt>
                      <dd class="font-semibold text-surface-50">{formatTimeMs(component.data.average_time_ms ?? 0)}</dd>
                    </div>
                    <div class="flex justify-between text-surface-300">
                      <dt>Last frame time</dt>
                      <dd class="font-semibold text-surface-50">{formatTimeMs(component.data.last_time_ms ?? 0)}</dd>
                    </div>
                    <div class="flex justify-between text-surface-300">
                      <dt>Samples / window</dt>
                      <dd class="font-semibold text-surface-50">{formatSamples(component.data)}</dd>
                    </div>
                    {#if component.data.processed !== undefined}
                      <div class="flex justify-between text-surface-300">
                        <dt>Processed</dt>
                        <dd class="font-semibold text-surface-50">{component.data.processed ?? '—'}</dd>
                      </div>
                    {/if}
                    {#if component.data.work_average_time_ms !== undefined && component.data.work_average_time_ms > 0}
                      <div class="flex justify-between text-surface-300">
                        <dt>Work avg</dt>
                        <dd class="font-semibold text-surface-50">{formatTimeMs(component.data.work_average_time_ms)}</dd>
                      </div>
                    {/if}
                    {#if component.data.work_last_time_ms !== undefined && component.data.work_last_time_ms > 0}
                      <div class="flex justify-between text-surface-300">
                        <dt>Work last</dt>
                        <dd class="font-semibold text-surface-50">{formatTimeMs(component.data.work_last_time_ms)}</dd>
                      </div>
                    {/if}
                    {#if component.data.backpressure !== undefined}
                      <div class="flex justify-between text-surface-300">
                        <dt>Backpressure</dt>
                        <dd class="font-semibold text-surface-50">{component.data.backpressure?.toFixed?.(2) ?? component.data.backpressure ?? '—'}</dd>
                      </div>
                    {/if}
                    {#if component.data.errors !== undefined}
                      <div class="flex justify-between text-surface-300">
                        <dt>Errors</dt>
                        <dd class="font-semibold text-surface-50">{component.data.errors ?? '—'}</dd>
                      </div>
                    {/if}
                  </dl>
                {/if}
              </div>
            {/if}
          {/each}
        </div>
      {/if}
      {#if componentMetrics.every((component) => component.data.sample_count === 0)}
        <p class="text-xs text-surface-500">
          {metricsStaleMessage ?? 'Waiting for the first metrics sample…'}
        </p>
      {/if}
    {:else}
      <p class="text-xs text-surface-500">
        {metricsStaleMessage ?? 'Waiting for the first metrics sample…'}
      </p>
    {/if}

	  {/if}
	</div>
