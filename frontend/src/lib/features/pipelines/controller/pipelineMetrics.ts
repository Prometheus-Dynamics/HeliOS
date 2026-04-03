import { browser } from "$app/environment";
import { get } from "svelte/store";
import { toaster } from "$lib";
import { loadOwnedStreams } from "$lib/api/streamResources";
import { StreamsApi } from "$lib/api/streamsApi";
import { cancellableWithTimeout } from "$lib/api/requestUtils";
import type { Readable, Writable } from "svelte/store";
import type { StreamInfo } from "$lib/api/client";
import type {
  PipelineOverviewPipeline,
  PipelineStreamNodeMetrics,
  PipelineNodeMetricMap,
} from "$lib/types/pipeline";
import type { PipelineGraphPlan, PipelineNodeRuntimeMetrics } from "$lib/types/pipeline";
import { resolveNodeOrder } from "$lib/features/pipelines/daedalusGraph";
import { describeError } from "./utils";
import { resolveStreamLabel } from "$lib/utils/streamLabels";

export type PipelineMetricsState = {
  status: "idle" | "connecting" | "connected" | "error";
  metrics: PipelineStreamNodeMetrics[] | null;
  error: string | null;
  updatedAt: number | null;
};

type PipelineMetricsManagerDeps = {
  pipelineMetricsState: Writable<Record<string, PipelineMetricsState>>;
  pipelines: Readable<PipelineOverviewPipeline[]>;
  selectedPipeline: Readable<PipelineOverviewPipeline | null>;
};

export type PipelineMetricsManager = ReturnType<
  typeof createPipelineMetricsManager
>;

export function createPipelineMetricsManager(deps: PipelineMetricsManagerDeps) {
  const PIPELINE_METRICS_ERROR_SUPPRESSION_MS = 15_000;
  const PIPELINE_METRICS_SNAPSHOT_POLL_MS = 3_000;
  // Keep below poll interval so one stuck stream can't freeze the UI.
  const PIPELINE_METRICS_REQUEST_TIMEOUT_MS = 2500;
  const asRecord = (value: unknown): Record<string, unknown> | null =>
    value && typeof value === "object" ? (value as Record<string, unknown>) : null;
  const extractGraphAlias = (graph: unknown): string | null => {
    const graphRecord = asRecord(graph);
    if (!graphRecord) return null;
    const metadata = asRecord(graphRecord.metadata);
    if (!metadata) return null;
    const raw = metadata["helios.pipeline.alias"] ?? null;
    if (typeof raw === "string") return raw.trim();
    const rawRecord = asRecord(raw);
    return typeof rawRecord?.value === "string" ? rawRecord.value.trim() : null;
  };
  const asStreamInfo = (value: unknown): StreamInfo | null => {
    const record = asRecord(value);
    return typeof record?.id === "string" && asRecord(record.manifest) ? (value as StreamInfo) : null;
  };
  const numberOr = (value: unknown, fallback = 0): number =>
    typeof value === "number" && Number.isFinite(value) ? value : fallback;
  const nullableNumber = (value: unknown): number | null =>
    typeof value === "number" && Number.isFinite(value) ? value : null;
  let pipelineMetricsSnapshotTimer: ReturnType<typeof setInterval> | null = null;
  let activeMetricsPipelineId: string | null = null;
  const pipelineMetricsErrorLog = new Map<
    string,
    { message: string; timestamp: number }
  >();

  function ensurePipelineMetricsEntry(
    pipelineId: string,
  ): PipelineMetricsState {
    const snapshot = get(deps.pipelineMetricsState)[pipelineId];
    if (snapshot) return snapshot;
    const initial: PipelineMetricsState = {
      status: "idle",
      metrics: null,
      error: null,
      updatedAt: null,
    };
    deps.pipelineMetricsState.update((map) => ({
      ...map,
      [pipelineId]: initial,
    }));
    return initial;
  }

  function updatePipelineMetricsState(
    pipelineId: string,
    patch: Partial<PipelineMetricsState>,
  ) {
    deps.pipelineMetricsState.update((map) => {
      const current = map[pipelineId] ?? {
        status: "idle",
        metrics: null,
        error: null,
        updatedAt: null,
      };
      return {
        ...map,
        [pipelineId]: { ...current, ...patch },
      };
    });
  }

  function notifyPipelineMetricsIssue(
    pipelineId: string,
    message: string,
    quiet: boolean,
  ) {
    if (quiet || !message) {
      return;
    }
    const key = `${pipelineId}:${message}`;
    const prior = pipelineMetricsErrorLog.get(key);
    const now = Date.now();
    if (
      prior &&
      now - prior.timestamp < PIPELINE_METRICS_ERROR_SUPPRESSION_MS
    ) {
      return;
    }
    pipelineMetricsErrorLog.set(key, { message, timestamp: now });
    toaster.error({
      title: "Pipeline metrics unavailable",
      description: message,
    });
  }

  async function fetchPipelineMetricsSnapshot(
    pipelineId: string,
    options: { quiet?: boolean } = {},
  ) {
    const { quiet = false } = options;
    const current = ensurePipelineMetricsEntry(pipelineId);
    try {
      const streams = await cancellableWithTimeout(
        () => loadOwnedStreams({ preferCached: false }),
        PIPELINE_METRICS_REQUEST_TIMEOUT_MS,
      );
      const pipelineRecord = get(deps.pipelines).find((entry) => entry.id === pipelineId) ?? null;
      const pipelineAliases = new Set<string>();
      const attachmentStreamIds = new Set<string>();
      if (pipelineRecord?.name) pipelineAliases.add(pipelineRecord.name.trim().toLowerCase());
      if (pipelineRecord?.alias) pipelineAliases.add(pipelineRecord.alias.trim().toLowerCase());
      if (Array.isArray(pipelineRecord?.attachments)) {
        for (const attachment of pipelineRecord.attachments) {
          const captureId = typeof attachment?.captureSessionId === "string" ? attachment.captureSessionId.trim() : "";
          if (captureId) attachmentStreamIds.add(captureId);
        }
      }

      const aliasMatches = (graph: unknown): boolean => {
        if (!pipelineAliases.size) return false;
        const alias = extractGraphAlias(graph);
        if (!alias) return false;
        return pipelineAliases.has(alias.toLowerCase());
      };

      const matching = streams.filter((stream) => {
        const streamInfo = asStreamInfo(stream);
        if (!streamInfo) return false;
        const streamId = streamInfo.id.trim();
        if (streamId && attachmentStreamIds.has(streamId)) {
          return true;
        }
        const manifest = asRecord(streamInfo.manifest);
        const direct = String(manifest?.active_pipeline_id ?? '').trim();
        if (direct === pipelineId) {
          return true;
        }

        const assignments = manifest?.pipelines ?? null;
        if (Array.isArray(assignments)) {
          return assignments.some((entry) => {
            const entryRecord = asRecord(entry);
            const resolvedId = String(entryRecord?.pipeline_id ?? '').trim();
            return resolvedId === pipelineId;
          });
        }
        if (Array.isArray(manifest?.pipelines)) {
          return manifest.pipelines.some((entry) => aliasMatches(asRecord(entry)?.pipeline_graph));
        }
        return false;
      });
      if (matching.length === 0) {
        updatePipelineMetricsState(pipelineId, {
          status: "idle",
          metrics: null,
          error: null,
          updatedAt: current?.updatedAt ?? null,
        });
        return;
      }

      const results = await Promise.allSettled(
        matching.map(async (stream) => {
          const metrics = await cancellableWithTimeout(
            () => StreamsApi.getMetrics({ id: stream.id }),
            PIPELINE_METRICS_REQUEST_TIMEOUT_MS,
          );
          return { stream, metrics };
        }),
      );

      const snapshots: Array<{ stream: StreamInfo; metrics: unknown }> = [];
      let errorMessage: string | null = null;
      results.forEach((result, index) => {
        if (result.status === "fulfilled") {
          snapshots.push(result.value);
          return;
        }
        if (!errorMessage) {
          const streamId = asStreamInfo(matching[index])?.id?.trim() || "<unknown stream>";
          errorMessage = `Unable to load metrics for ${streamId}.`;
        }
      });

      if (snapshots.length === 0) {
        updatePipelineMetricsState(pipelineId, {
          status: "error",
          metrics: current?.metrics ?? null,
          error: errorMessage ?? "Unable to load metrics.",
          updatedAt: current?.updatedAt ?? null,
        });
        notifyPipelineMetricsIssue(pipelineId, errorMessage ?? "Unable to load metrics.", quiet);
        return;
      }

      const toRuntime = (
        averageTimeMs: number,
        averageFps: number,
        sampleCount: number,
        windowSize = 60,
        lastSampleAgeMs: number | null = null,
        lastError: string | null = null,
        lastErrorAt: number | null = null,
      ): PipelineNodeRuntimeMetrics => ({
        metrics: {
          averageTimeMs,
          averageFps,
          sampleCount,
          windowSize,
          lastSampleAgeMs,
        },
        outputEdges: [],
        inputQueues: undefined,
        outputSinks: undefined,
        inputSources: undefined,
        inputSync: undefined,
        lastError,
        lastErrorAt,
        children: null,
      });

      const runtimeIdMapForPlan = (plan: PipelineGraphPlan | null | undefined): Map<string, string> => {
        const map = new Map<string, string>();
        if (!plan?.nodes) return map;
        const order = resolveNodeOrder(plan);
        const counts = new Map<string, number>();
        for (const nodeId of order) {
          const backendId = String(plan.nodes?.[nodeId]?.backendId ?? "").trim();
          if (!backendId) continue;
          counts.set(backendId, (counts.get(backendId) ?? 0) + 1);
        }
        const seen = new Map<string, number>();
        for (const nodeId of order) {
          const backendId = String(plan.nodes?.[nodeId]?.backendId ?? "").trim();
          if (!backendId) continue;
          const total = counts.get(backendId) ?? 0;
          if (total <= 1) {
            map.set(backendId, nodeId);
            continue;
          }
          const idx = (seen.get(backendId) ?? 0) + 1;
          seen.set(backendId, idx);
          map.set(`${backendId}#${idx}`, nodeId);
        }
        return map;
      };

      const pipelinePlan: PipelineGraphPlan | null = pipelineRecord?.graph ?? null;
      const runtimeIdToPlanNodeId = runtimeIdMapForPlan(pipelinePlan);

      function mapRuntimeMetrics(runtime: unknown): PipelineNodeRuntimeMetrics {
        const runtimeRecord = asRecord(runtime);
        const stats = asRecord(runtimeRecord?.metrics) ?? {};
        const mappedChildren = mapNodeMetrics(runtimeRecord?.children ?? null);
        return {
          metrics: {
            averageTimeMs: numberOr(stats.average_time_ms),
            averageFps: numberOr(stats.average_fps),
            sampleCount: numberOr(stats.sample_count),
            windowSize: numberOr(stats.window_size, 60),
            lastSampleAgeMs: nullableNumber(stats.last_sample_age_ms),
          },
          outputEdges: [],
          inputQueues: undefined,
          outputSinks: undefined,
          inputSources: undefined,
          inputSync: undefined,
          lastError: typeof runtimeRecord?.last_error === "string" ? runtimeRecord.last_error : null,
          lastErrorAt: typeof runtimeRecord?.last_error_at === "number" ? runtimeRecord.last_error_at : null,
          children: mappedChildren ?? null,
        };
      }

      function mapNodeMetrics(nodes: unknown): PipelineNodeMetricMap | null {
        if (!nodes || typeof nodes !== "object") return null;
        const mapped: PipelineNodeMetricMap = {};
        Object.entries(nodes).forEach(([runtimeNodeId, runtime]) => {
          const mappedId = runtimeIdToPlanNodeId.get(runtimeNodeId) ?? runtimeNodeId;
          mapped[mappedId] = mapRuntimeMetrics(runtime);
        });
        return mapped;
      }

      const mapped: PipelineStreamNodeMetrics[] = snapshots.map(({ stream, metrics }) => {
        const metricsRecord = asRecord(metrics);
        const pipelineRecord = asRecord(metricsRecord?.pipeline);
        const nodeMetrics: PipelineNodeMetricMap = {};
        const captureStats = asRecord(metricsRecord?.capture);
        nodeMetrics["capture"] = toRuntime(
          numberOr(captureStats?.average_time_ms),
          numberOr(captureStats?.fps),
          numberOr(captureStats?.sample_count),
        );
        const encoderStats = asRecord(metricsRecord?.encoder);
        if (encoderStats) {
          nodeMetrics["encoder"] = toRuntime(
            numberOr(encoderStats.average_time_ms),
            numberOr(encoderStats.fps),
            numberOr(encoderStats.sample_count),
          );
        }
        const decoderStats = asRecord(metricsRecord?.decoder);
        if (decoderStats) {
          nodeMetrics["decoder"] = toRuntime(
            numberOr(decoderStats.average_time_ms),
            numberOr(decoderStats.fps),
            numberOr(decoderStats.sample_count),
          );
        }

        const pipelineNodes = mapNodeMetrics(pipelineRecord?.nodes ?? null);
        if (pipelineNodes) {
          Object.assign(nodeMetrics, pipelineNodes);
        }

        const groupNodes = mapNodeMetrics(pipelineRecord?.groups ?? null);

        const perf = asRecord(pipelineRecord?.perf);
        const flamegraph = asRecord(pipelineRecord?.flamegraph);
        const perfMetrics = perf
          ? {
              averageCacheMisses: numberOr(perf.average_cache_misses),
              averageBranchInstructions: numberOr(perf.average_branch_instructions),
              averageBranchMisses: numberOr(perf.average_branch_misses),
              sampleCount: numberOr(perf.sample_count),
              windowSize: numberOr(perf.window_size),
              lastSampleAgeMs: nullableNumber(perf.last_sample_age_ms),
            }
          : null;
        const flamegraphMetrics = flamegraph
          ? {
              path: typeof flamegraph.path === "string" ? flamegraph.path : "",
              sizeBytes: numberOr(flamegraph.size_bytes),
              capturedAtMs: numberOr(flamegraph.captured_at_ms),
            }
          : null;

        return {
          streamId: stream.id,
          streamPath: (() => {
            const label = resolveStreamLabel(stream, "");
            return label ? label : null;
          })(),
          metrics: nodeMetrics,
          groups: groupNodes,
          perf: perfMetrics,
          flamegraph: flamegraphMetrics,
        };
      });

      updatePipelineMetricsState(pipelineId, {
        status: "connected",
        metrics: mapped,
        error: errorMessage,
        updatedAt: Date.now(),
      });
    } catch (error) {
      const message = describeError(error);
      updatePipelineMetricsState(pipelineId, {
        status: "error",
        metrics: current?.metrics ?? null,
        error: message,
        updatedAt: current?.updatedAt ?? null,
      });
      notifyPipelineMetricsIssue(pipelineId, message, quiet);
    }
  }

  function teardownPipelineMetrics(
    pipelineId: string | null = activeMetricsPipelineId,
    options: { resetState?: boolean } = {},
  ) {
    const { resetState = true } = options;
    if (pipelineMetricsSnapshotTimer) {
      clearInterval(pipelineMetricsSnapshotTimer);
      pipelineMetricsSnapshotTimer = null;
    }
    if (resetState && pipelineId) {
      updatePipelineMetricsState(pipelineId, {
        status: "idle",
        metrics: null,
        error: null,
        updatedAt: null,
      });
    }
    if (pipelineId && activeMetricsPipelineId === pipelineId) {
      activeMetricsPipelineId = null;
    }
  }

  function ensureSnapshotPolling(pipelineId: string) {
    if (pipelineMetricsSnapshotTimer) {
      return;
    }
    pipelineMetricsSnapshotTimer = setInterval(() => {
      if (activeMetricsPipelineId !== pipelineId) {
        return;
      }
      void fetchPipelineMetricsSnapshot(pipelineId, { quiet: true });
    }, PIPELINE_METRICS_SNAPSHOT_POLL_MS);
  }

  function connectPipelineMetrics(
    pipelineId: string,
    options: { force?: boolean; quiet?: boolean } = {},
  ) {
    const { force = false, quiet = false } = options;
    if (!force && pipelineId === activeMetricsPipelineId) {
      ensureSnapshotPolling(pipelineId);
      return;
    }
    teardownPipelineMetrics(undefined, { resetState: false });
    activeMetricsPipelineId = pipelineId;
    updatePipelineMetricsState(pipelineId, {
      status: "connecting",
      error: null,
    });
    void fetchPipelineMetricsSnapshot(pipelineId, { quiet });
    ensureSnapshotPolling(pipelineId);
  }

  async function refreshPipelineMetrics(
    pipelineId?: string,
    options: { quiet?: boolean } = {},
  ) {
    const { quiet = false } = options;
    const target = pipelineId ?? get(deps.selectedPipeline)?.id;
    if (!target) return;
    await fetchPipelineMetricsSnapshot(target, { quiet });
    connectPipelineMetrics(target, { force: true, quiet: true });
  }

  if (browser) {
    deps.selectedPipeline.subscribe((selected) => {
      const pipelineId = selected?.id ?? null;
      if (!pipelineId) {
        teardownPipelineMetrics();
        return;
      }
      if (pipelineId !== activeMetricsPipelineId) {
        teardownPipelineMetrics();
      }
    });
  }

  return {
    refreshPipelineMetrics,
    teardownPipelineMetrics,
  };
}
