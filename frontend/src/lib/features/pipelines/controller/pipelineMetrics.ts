import { browser } from "$app/environment";
import { get } from "svelte/store";
import { toaster } from "$lib";
import { StreamsApi } from "$lib/api/streamsApi";
import { cancellableWithTimeout } from "$lib/api/requestUtils";
import type { Readable, Writable } from "svelte/store";
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
      const listResult = await cancellableWithTimeout(
        () => StreamsApi.listStreams(),
        PIPELINE_METRICS_REQUEST_TIMEOUT_MS,
      );
      const streams: any[] = Array.isArray(listResult)
        ? listResult
        : Array.isArray((listResult as any)?.items)
          ? (listResult as any).items
          : [];
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

      const extractAlias = (graph: any): string | null => {
        if (!graph || typeof graph !== "object") return null;
        const raw = graph?.metadata?.["helios.pipeline.alias"] ?? null;
        if (typeof raw === "string") return raw.trim();
        if (raw && typeof raw === "object") {
          const value = (raw as any).value;
          if (typeof value === "string") return value.trim();
        }
        return null;
      };
      const aliasMatches = (graph: any): boolean => {
        if (!pipelineAliases.size) return false;
        const alias = extractAlias(graph);
        if (!alias) return false;
        return pipelineAliases.has(alias.toLowerCase());
      };

      const matching = streams.filter((stream) => {
        const streamId = typeof stream?.id === "string" ? stream.id.trim() : "";
        if (streamId && attachmentStreamIds.has(streamId)) {
          return true;
        }
        const manifest: any = stream?.manifest ?? null;
        const direct =
          String(manifest?.active_pipeline_id ?? "").trim() ||
          String(manifest?.pipeline_id ?? "").trim() ||
          String(manifest?.pipelineId ?? "").trim() ||
          String(manifest?.pipeline?.id ?? "").trim();
        if (direct && direct === pipelineId) {
          return true;
        }

        const assignments: any = manifest?.pipelines ?? null;
        if (Array.isArray(assignments)) {
          return assignments.some((entry: any) => {
            const id = String(entry?.pipeline_id ?? entry?.pipelineId ?? entry?.pipeline?.id ?? "").trim();
            return id === pipelineId;
          });
        }
        if (assignments && typeof assignments === "object") {
          return Object.values(assignments).some((entry: any) => {
            const id = String(entry?.pipeline_id ?? entry?.pipelineId ?? entry?.pipeline?.id ?? "").trim();
            return id === pipelineId;
          });
        }
        if (aliasMatches(manifest?.pipeline_graph)) {
          return true;
        }
        if (Array.isArray(manifest?.pipelines)) {
          return manifest.pipelines.some((entry: any) => aliasMatches(entry?.pipeline_graph));
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

      const snapshots: Array<{ stream: any; metrics: unknown }> = [];
      let errorMessage: string | null = null;
      results.forEach((result, index) => {
        if (result.status === "fulfilled") {
          snapshots.push(result.value);
          return;
        }
        if (!errorMessage) {
          const streamId = String((matching[index] as any)?.id ?? "").trim() || "<unknown stream>";
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

      function mapRuntimeMetrics(runtime: any): PipelineNodeRuntimeMetrics {
        const stats = runtime?.metrics ?? {};
        const mappedChildren = mapNodeMetrics(runtime?.children ?? null);
        return {
          metrics: {
            averageTimeMs: stats.average_time_ms ?? 0,
            averageFps: stats.average_fps ?? 0,
            sampleCount: stats.sample_count ?? 0,
            windowSize: stats.window_size ?? 60,
            lastSampleAgeMs:
              typeof stats.last_sample_age_ms === "number" ? stats.last_sample_age_ms : null,
          },
          outputEdges: [],
          inputQueues: undefined,
          outputSinks: undefined,
          inputSources: undefined,
          inputSync: undefined,
          lastError: runtime?.last_error ?? null,
          lastErrorAt: runtime?.last_error_at ?? null,
          children: mappedChildren ?? null,
        };
      }

      function mapNodeMetrics(nodes: any): PipelineNodeMetricMap | null {
        if (!nodes || typeof nodes !== "object") return null;
        const mapped: PipelineNodeMetricMap = {};
        Object.entries(nodes).forEach(([runtimeNodeId, runtime]) => {
          const mappedId = runtimeIdToPlanNodeId.get(runtimeNodeId) ?? runtimeNodeId;
          mapped[mappedId] = mapRuntimeMetrics(runtime);
        });
        return mapped;
      }

      const mapped: PipelineStreamNodeMetrics[] = snapshots.map(({ stream, metrics }) => {
        const nodeMetrics: Record<string, any> = {};
        const captureStats = (metrics as any)?.capture ?? null;
        nodeMetrics["capture"] = toRuntime(
          captureStats?.average_time_ms ?? 0,
          captureStats?.fps ?? 0,
          captureStats?.sample_count ?? 0,
        );
        const encoderStats = (metrics as any)?.encoder ?? null;
        if (encoderStats) {
          nodeMetrics["encoder"] = toRuntime(
            encoderStats?.average_time_ms ?? 0,
            encoderStats?.fps ?? 0,
            encoderStats?.sample_count ?? 0,
          );
        }
        const decoderStats = (metrics as any)?.decoder ?? null;
        if (decoderStats) {
          nodeMetrics["decoder"] = toRuntime(
            decoderStats?.average_time_ms ?? 0,
            decoderStats?.fps ?? 0,
            decoderStats?.sample_count ?? 0,
          );
        }

        const pipelineNodes = mapNodeMetrics((metrics as any)?.pipeline?.nodes ?? null);
        if (pipelineNodes) {
          Object.assign(nodeMetrics, pipelineNodes);
        }

        const groupNodes = mapNodeMetrics((metrics as any)?.pipeline?.groups ?? null);

        const perf = (metrics as any)?.pipeline?.perf ?? null;
        const flamegraph = (metrics as any)?.pipeline?.flamegraph ?? null;
        const perfMetrics = perf
          ? {
              averageCacheMisses: perf.average_cache_misses ?? 0,
              averageBranchInstructions: perf.average_branch_instructions ?? 0,
              averageBranchMisses: perf.average_branch_misses ?? 0,
              sampleCount: perf.sample_count ?? 0,
              windowSize: perf.window_size ?? 0,
              lastSampleAgeMs:
                typeof perf.last_sample_age_ms === "number"
                  ? perf.last_sample_age_ms
                  : null,
            }
          : null;
        const flamegraphMetrics = flamegraph
          ? {
              path: flamegraph.path ?? "",
              sizeBytes: flamegraph.size_bytes ?? 0,
              capturedAtMs: flamegraph.captured_at_ms ?? 0,
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
    void fetchPipelineMetricsSnapshot(pipelineId, { quiet: true });
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
