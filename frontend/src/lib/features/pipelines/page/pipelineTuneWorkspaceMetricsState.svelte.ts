import { get, type Readable } from 'svelte/store';
import { onDestroy, untrack } from 'svelte';
import { SvelteSet } from 'svelte/reactivity';
import { loadOwnedStreams } from '$lib/api/streamResources';
import { openStreamMetricsSocket } from '$lib/api/streamMetrics';
import { canUseWebSockets } from '$lib/api/core/ws';
import { cancellableWithTimeout } from '$lib/api/requestUtils';
import type { StreamsApi as SharedStreamsApi } from '$lib/api/streamsApi';
import type { PipelineOverviewPipeline } from '$lib/types/pipeline';
import type { PipelineStreamNodeMetrics } from '$lib/types/pipeline';
import type { StreamInfo } from '$lib/api/client';
import {
  buildPipelineMetricsSummary,
  buildTuneMetricsSnapshot,
  createTuneMetricsRuntime,
  type StreamMetricsSummary,
  type TuneMetricsStreamRef
} from './pipelineTuneMetricsRuntime';
import { runTuneMetricsRefresh } from './pipelineTuneEffects';

type StreamsApi = Pick<typeof SharedStreamsApi, 'getMetrics'>;
type UnknownObject = { [key: string]: unknown };

type Args = {
  activeTab: Readable<'pipeline' | 'tune'>;
  selectedPipeline: Readable<PipelineOverviewPipeline | null>;
  getTunePerformanceTab: () => 'metrics' | 'controls' | 'layout' | 'outputs';
  getTuneScopeTab: () => 'global' | string;
  getTuneMetricsWantedKey: () => string;
  getTuneMetricsWantedRefs: () => TuneMetricsStreamRef[];
  StreamsApi: StreamsApi;
  buildErrorMessage: (input: { error: unknown; fallback: string }) => string;
  streamUsesPipeline: (stream: StreamInfo, pipelineId: string) => boolean;
  streamLabel: (stream: StreamInfo) => string;
  extractGraphAlias: (graph: unknown) => string | null;
  asRecord: (value: unknown) => UnknownObject | null;
  asStreamInfo: (value: unknown) => StreamInfo | null;
  refreshPipelineMetrics: () => void;
};

export function createTuneWorkspaceMetricsState(args: Args) {
  const {
    activeTab,
    selectedPipeline,
    getTunePerformanceTab,
    getTuneScopeTab,
    getTuneMetricsWantedKey,
    getTuneMetricsWantedRefs,
    StreamsApi,
    buildErrorMessage,
    streamUsesPipeline,
    streamLabel,
    extractGraphAlias,
    asRecord,
    asStreamInfo,
    refreshPipelineMetrics
  } = args;

  const TUNE_METRICS_POLL_MS = 3_000;
  const TUNE_METRICS_WS_INTERVAL_MS = 800;
  const TUNE_METRICS_SNAPSHOT_TIMEOUT_MS = 2500;

  let tuneMetricsSnapshots = $state<PipelineStreamNodeMetrics[]>([]);
  let tuneMetricsStatus = $state<'idle' | 'connecting' | 'connected' | 'error'>('idle');
  let tuneMetricsError = $state<string | null>(null);
  let tuneMetricsUpdatedAt = $state<number | null>(null);
  let tuneMetricsPollTimer: number | null = null;
  let tuneMetricsRequestId = $state(0);
  let tuneMetricsInFlight = false;
  let tuneMetricsRefreshPipelineId = $state<string | null>(null);

  type MetricsSource = {
    metrics: PipelineStreamNodeMetrics[];
    status: 'idle' | 'connecting' | 'connected' | 'error';
    error: string | null;
    updatedAt: number | null;
  };

  const metricsSource: MetricsSource = $derived.by(() => ({
    metrics: tuneMetricsSnapshots,
    status: tuneMetricsStatus,
    error: tuneMetricsError,
    updatedAt: tuneMetricsUpdatedAt
  }));

  const pipelineMetricsSummary: StreamMetricsSummary[] = $derived.by(() =>
    buildPipelineMetricsSummary(metricsSource.metrics ?? [])
  );

  const metricsUpdatedLabel: string = $derived.by(() => {
    const ts = metricsSource.updatedAt ?? null;
    if (!ts || !Number.isFinite(ts)) return 'Not updated yet';
    const date = new Date(ts);
    return Number.isFinite(date.valueOf()) ? date.toLocaleTimeString() : 'Unknown';
  });

  const metricsStatusLabel: string = $derived.by(() => {
    const status = metricsSource.status ?? 'idle';
    if (status === 'connected') return 'Live';
    if (status === 'connecting') return 'Connecting…';
    if (status === 'error') return 'Offline';
    return 'Idle';
  });

  const { fetchTuneMetricsSnapshots } = untrack(() =>
    createTuneMetricsRuntime({
      StreamsApi,
      buildErrorMessage,
      getSelectedPipelineId: () => get(selectedPipeline)?.id ?? null,
      getTuneMetricsSnapshots: () => tuneMetricsSnapshots,
      setTuneMetricsSnapshots: (next) => {
        tuneMetricsSnapshots = next;
      },
      getTuneMetricsStatus: () => tuneMetricsStatus,
      setTuneMetricsStatus: (next) => {
        tuneMetricsStatus = next;
      },
      getTuneMetricsError: () => tuneMetricsError,
      setTuneMetricsError: (next) => {
        tuneMetricsError = next;
      },
      getTuneMetricsUpdatedAt: () => tuneMetricsUpdatedAt,
      setTuneMetricsUpdatedAt: (next) => {
        tuneMetricsUpdatedAt = next;
      },
      getTuneMetricsRequestId: () => tuneMetricsRequestId,
      setTuneMetricsRequestId: (next) => {
        tuneMetricsRequestId = next;
      },
      getTuneMetricsInFlight: () => tuneMetricsInFlight,
      setTuneMetricsInFlight: (next) => {
        tuneMetricsInFlight = next;
      }
    })
  );

  $effect(() => {
    if (get(activeTab) !== 'tune' || getTunePerformanceTab() !== 'metrics') {
      tuneMetricsSnapshots = [];
      tuneMetricsStatus = 'idle';
      tuneMetricsError = null;
      tuneMetricsUpdatedAt = null;
      if (tuneMetricsPollTimer) {
        clearInterval(tuneMetricsPollTimer);
        tuneMetricsPollTimer = null;
      }
      return;
    }

    let isMounted = true;
    let reconnectTimer: number | null = null;
    let connectTimeoutTimer: number | null = null;
    let reconnectAttempts = 0;
    let sawAnyMetrics = false;
    let pollInFlight = false;
    const cleanups: Record<string, () => void> = {};

    const stopTimers = () => {
      if (reconnectTimer) {
        clearTimeout(reconnectTimer);
        reconnectTimer = null;
      }
      if (connectTimeoutTimer) {
        clearTimeout(connectTimeoutTimer);
        connectTimeoutTimer = null;
      }
      if (tuneMetricsPollTimer) {
        clearInterval(tuneMetricsPollTimer);
        tuneMetricsPollTimer = null;
      }
      reconnectAttempts = 0;
    };

    const stopSockets = () => {
      Object.values(cleanups).forEach((fn) => fn());
      for (const key of Object.keys(cleanups)) delete cleanups[key];
    };

    const upsertSnapshot = (snapshot: PipelineStreamNodeMetrics) => {
      const rest = tuneMetricsSnapshots.filter((entry) => entry.streamId !== snapshot.streamId);
      tuneMetricsSnapshots = [snapshot, ...rest];
    };

    const fetchSnapshotFor = async (ref: TuneMetricsStreamRef) => {
      try {
        const metrics = await cancellableWithTimeout(
          () => StreamsApi.getMetrics({ id: ref.id }),
          TUNE_METRICS_SNAPSHOT_TIMEOUT_MS
        );
        if (!isMounted) return;
        upsertSnapshot(
          buildTuneMetricsSnapshot(ref, metrics, {
            pipelineId: get(selectedPipeline)?.id ?? null
          })
        );
        sawAnyMetrics = true;
        if (tuneMetricsStatus === 'connecting') tuneMetricsStatus = 'connected';
        tuneMetricsUpdatedAt = Date.now();
      } catch (error) {
        if (!isMounted) return;
        const message = buildErrorMessage({ error, fallback: `Unable to load metrics for ${ref.label}.` });
        tuneMetricsError = tuneMetricsError ?? message;
      }
    };

    const scheduleReconnect = (wanted: TuneMetricsStreamRef[], connect: () => void) => {
      if (reconnectTimer) return;
      const delay = Math.min(15_000, 1_500 * 2 ** Math.min(reconnectAttempts, 4));
      reconnectAttempts += 1;
      reconnectTimer = window.setTimeout(() => {
        reconnectTimer = null;
        if (!isMounted) return;
        stopSockets();
        tuneMetricsSnapshots = wanted.map((ref) =>
          buildTuneMetricsSnapshot(ref, null, { pipelineId: get(selectedPipeline)?.id ?? null })
        );
        tuneMetricsStatus = 'connecting';
        tuneMetricsError = tuneMetricsError ?? null;
        tuneMetricsUpdatedAt = null;
        connect();
      }, delay);
    };

    const openSocketFor = (ref: TuneMetricsStreamRef, wanted: TuneMetricsStreamRef[], connect: () => void) =>
      openStreamMetricsSocket(
        ref.id,
        {
          onMetrics: (event) => {
            if (!isMounted || event.stream_id !== ref.id) return;
            sawAnyMetrics = true;
            reconnectAttempts = 0;
            upsertSnapshot(
              buildTuneMetricsSnapshot(ref, event.metrics, {
                pipelineId: get(selectedPipeline)?.id ?? null
              })
            );
            tuneMetricsStatus = 'connected';
            tuneMetricsError = null;
            tuneMetricsUpdatedAt = typeof event.timestamp_ms === 'number' ? event.timestamp_ms : Date.now();
            if (tuneMetricsPollTimer) {
              clearInterval(tuneMetricsPollTimer);
              tuneMetricsPollTimer = null;
            }
          },
          onError: (err) => {
            if (!isMounted) return;
            tuneMetricsError = err?.error ?? 'Metrics stream error';
            if (!sawAnyMetrics) tuneMetricsStatus = 'error';
            tuneMetricsUpdatedAt = tuneMetricsUpdatedAt ?? Date.now();
            scheduleReconnect(wanted, connect);
          },
          onClose: (info) => {
            if (!isMounted || info?.expected) return;
            if (!sawAnyMetrics) {
              tuneMetricsStatus = 'error';
              tuneMetricsError = tuneMetricsError ?? 'Metrics stream closed';
              tuneMetricsUpdatedAt = tuneMetricsUpdatedAt ?? Date.now();
            }
            scheduleReconnect(wanted, connect);
          }
        },
        { intervalMs: TUNE_METRICS_WS_INTERVAL_MS }
      );

    const startMetrics = (wanted: TuneMetricsStreamRef[]) => {
      tuneMetricsSnapshots = wanted.map((ref) =>
        buildTuneMetricsSnapshot(ref, null, { pipelineId: get(selectedPipeline)?.id ?? null })
      );
      tuneMetricsStatus = 'connecting';
      tuneMetricsError = null;
      tuneMetricsUpdatedAt = null;
      sawAnyMetrics = false;
      reconnectAttempts = 0;

      const connect = () => {
        for (const ref of wanted) void fetchSnapshotFor(ref);

        if (!canUseWebSockets()) {
          void fetchTuneMetricsSnapshots(wanted);
          if (tuneMetricsPollTimer) clearInterval(tuneMetricsPollTimer);
          tuneMetricsPollTimer = window.setInterval(() => void fetchTuneMetricsSnapshots(wanted), TUNE_METRICS_POLL_MS);
          return;
        }

        for (const ref of wanted) {
          cleanups[ref.id] = openSocketFor(ref, wanted, connect);
        }

        if (!tuneMetricsPollTimer) {
          tuneMetricsPollTimer = window.setInterval(() => {
            if (!isMounted || pollInFlight) return;
            pollInFlight = true;
            Promise.allSettled(wanted.map((ref) => fetchSnapshotFor(ref))).finally(() => {
              pollInFlight = false;
            });
          }, TUNE_METRICS_POLL_MS);
        }

        if (connectTimeoutTimer) clearTimeout(connectTimeoutTimer);
        connectTimeoutTimer = window.setTimeout(() => {
          connectTimeoutTimer = null;
          if (!isMounted || sawAnyMetrics || tuneMetricsStatus !== 'connecting') return;
          tuneMetricsStatus = 'error';
          tuneMetricsError = tuneMetricsError ?? 'Metrics stream is not responding.';
          tuneMetricsUpdatedAt = tuneMetricsUpdatedAt ?? Date.now();
          scheduleReconnect(wanted, connect);
        }, Math.max(1500, TUNE_METRICS_SNAPSHOT_TIMEOUT_MS));
      };

      connect();
    };

    const wantedKey = getTuneMetricsWantedKey();
    const wanted = untrack(() => getTuneMetricsWantedRefs());
    void wantedKey;

    if (wanted.length > 0) {
      startMetrics(wanted);
      return () => {
        isMounted = false;
        stopTimers();
        stopSockets();
      };
    }

    const pipeline = get(selectedPipeline);
    const pipelineId = pipeline?.id ?? '';
    if (getTuneScopeTab() === 'global' && pipelineId) {
      tuneMetricsSnapshots = [];
      tuneMetricsStatus = 'connecting';
      tuneMetricsError = null;
      tuneMetricsUpdatedAt = null;

      void (async () => {
        try {
          const streams = await cancellableWithTimeout(
            () => loadOwnedStreams({ preferCached: false }),
            TUNE_METRICS_SNAPSHOT_TIMEOUT_MS
          );
          if (!isMounted) return;

          const aliases = new SvelteSet<string>();
          const name = typeof pipeline?.name === 'string' ? pipeline.name.trim().toLowerCase() : '';
          const alias = typeof pipeline?.alias === 'string' ? pipeline.alias.trim().toLowerCase() : '';
          if (name) aliases.add(name);
          if (alias) aliases.add(alias);

          const aliasMatches = (graph: unknown): boolean => {
            if (!aliases.size) return false;
            const value = extractGraphAlias(graph);
            if (!value) return false;
            return aliases.has(value.toLowerCase());
          };

          const matching = streams.filter((stream) => {
            const streamInfo = asStreamInfo(stream);
            if (!streamInfo) return false;
            const id = streamInfo.id;
            if (!id.trim()) return false;
            const manifest = asRecord(streamInfo.manifest);
            if (manifest?.internal === true) return false;
            if (streamUsesPipeline(streamInfo, pipelineId)) return true;
            if (aliasMatches(manifest?.pipeline_graph)) return true;
            if (Array.isArray(manifest?.pipelines)) {
              return manifest.pipelines.some((binding) => aliasMatches(asRecord(binding)?.pipeline_graph));
            }
            return false;
          });

          const refs: TuneMetricsStreamRef[] = [];
          const seen = new SvelteSet<string>();
          for (const stream of matching) {
            const streamInfo = asStreamInfo(stream);
            if (!streamInfo) continue;
            const id = streamInfo.id.trim();
            if (!id || seen.has(id)) continue;
            refs.push({ id, label: streamLabel(streamInfo) || id });
            seen.add(id);
          }

          if (refs.length === 0) {
            tuneMetricsSnapshots = [];
            tuneMetricsStatus = 'idle';
            tuneMetricsError = null;
            tuneMetricsUpdatedAt = null;
            return;
          }

          startMetrics(refs);
        } catch (error) {
          if (!isMounted) return;
          tuneMetricsSnapshots = [];
          tuneMetricsStatus = 'error';
          tuneMetricsError = buildErrorMessage({ error, fallback: 'Unable to discover attached streams for metrics.' });
          tuneMetricsUpdatedAt = Date.now();
        }
      })();

      return () => {
        isMounted = false;
        stopTimers();
        stopSockets();
      };
    }

    tuneMetricsSnapshots = [];
    tuneMetricsStatus = 'idle';
    tuneMetricsError = null;
    tuneMetricsUpdatedAt = null;
    stopTimers();
    stopSockets();
    return () => {
      isMounted = false;
      stopTimers();
      stopSockets();
    };
  });

  $effect(() => {
    runTuneMetricsRefresh({
      activeTab: get(activeTab),
      selectedPipelineId: get(selectedPipeline)?.id ?? null,
      tuneMetricsRefreshPipelineId,
      setTuneMetricsRefreshPipelineId: (next) => {
        tuneMetricsRefreshPipelineId = next;
      },
      refreshPipelineMetrics
    });
  });

  onDestroy(() => {
    if (tuneMetricsPollTimer) {
      clearInterval(tuneMetricsPollTimer);
    }
  });

  return {
    TUNE_METRICS_POLL_MS,
    fetchTuneMetricsSnapshots,
    get metricsSource() {
      return metricsSource;
    },
    get metricsStatusLabel() {
      return metricsStatusLabel;
    },
    get metricsUpdatedLabel() {
      return metricsUpdatedLabel;
    },
    get pipelineMetricsSummary() {
      return pipelineMetricsSummary;
    },
    get tuneMetricsError() {
      return tuneMetricsError;
    },
    get tuneMetricsInFlight() {
      return tuneMetricsInFlight;
    },
    get tuneMetricsPollTimer() {
      return tuneMetricsPollTimer;
    },
    get tuneMetricsRefreshPipelineId() {
      return tuneMetricsRefreshPipelineId;
    },
    get tuneMetricsRequestId() {
      return tuneMetricsRequestId;
    },
    get tuneMetricsSnapshots() {
      return tuneMetricsSnapshots;
    },
    get tuneMetricsStatus() {
      return tuneMetricsStatus;
    },
    get tuneMetricsUpdatedAt() {
      return tuneMetricsUpdatedAt;
    },
    getTuneMetricsPollTimer: () => tuneMetricsPollTimer,
    setTuneMetricsError: (next: string | null) => {
      tuneMetricsError = next;
    },
    setTuneMetricsPollTimer: (next: number | null) => {
      tuneMetricsPollTimer = next;
    },
    setTuneMetricsSnapshots: (next: PipelineStreamNodeMetrics[]) => {
      tuneMetricsSnapshots = next;
    },
    setTuneMetricsStatus: (next: 'idle' | 'connecting' | 'connected' | 'error') => {
      tuneMetricsStatus = next;
    },
    setTuneMetricsUpdatedAt: (next: number | null) => {
      tuneMetricsUpdatedAt = next;
    }
  };
}
