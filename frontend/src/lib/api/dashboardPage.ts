import { PeripheralsApi } from '$lib/api/peripheralsApi';
import { PipelinesApi } from '$lib/api/pipelinesApi';
import { StreamsApi } from '$lib/api/streamsApi';
import { DeviceApi } from '$lib/api/deviceApi';
import { DEFAULT_REQUEST_TIMEOUT_MS } from '$lib/api/requestUtils';
import type { DashboardFetchMeta, DashboardPayload, DashboardSourceStatus, PipelineWatchEntry, StreamGalleryItem, SummaryStat, TimelineItem } from '$lib/types/dashboard';
import type { DeviceMetrics, PipelineSummary, ProbedDevice, StreamInfo } from '$lib/ts-bindings/http/client';
import { resolveStreamAlias, resolveStreamLabel } from '$lib/utils/streamLabels';

const REQUEST_TIMEOUT_MS = DEFAULT_REQUEST_TIMEOUT_MS;

export async function fetchDashboardPageData(): Promise<DashboardPayload> {
  const [streamsResult, camerasResult, pipelinesResult, metricsResult] = await Promise.allSettled([
    StreamsApi.resolvedStreams({ timeoutMs: REQUEST_TIMEOUT_MS }),
    PeripheralsApi.listCameras({ timeoutMs: REQUEST_TIMEOUT_MS }),
    PipelinesApi.listGraphs({ timeoutMs: REQUEST_TIMEOUT_MS }),
    DeviceApi.metrics({ timeoutMs: REQUEST_TIMEOUT_MS })
  ]);

  const failures = [streamsResult, camerasResult, pipelinesResult, metricsResult].filter((result) => result.status === 'rejected').length;
  if (failures === 4) {
    throw new Error('All dashboard data requests failed');
  }

  if (streamsResult.status === 'rejected') {
    console.warn('Stream inventory request failed', streamsResult.reason);
  }
  if (camerasResult.status === 'rejected') {
    console.warn('Camera inventory request failed', camerasResult.reason);
  }
  if (pipelinesResult.status === 'rejected') {
    console.warn('Pipeline graph list request failed', pipelinesResult.reason);
  }
  if (metricsResult.status === 'rejected') {
    console.warn('Device metrics request failed', metricsResult.reason);
  }

  const streams = streamsResult.status === 'fulfilled' && Array.isArray(streamsResult.value)
    ? streamsResult.value.filter(Boolean)
    : [];
  const cameras = camerasResult.status === 'fulfilled' && Array.isArray(camerasResult.value.cameras)
    ? camerasResult.value.cameras
    : [];
  const pipelines = pipelinesResult.status === 'fulfilled' && Array.isArray(pipelinesResult.value)
    ? pipelinesResult.value
    : [];
  const metricsPayload = metricsResult.status === 'fulfilled' ? metricsResult.value : null;

  const meta = buildDashboardMeta(streamsResult.status, camerasResult.status, pipelinesResult.status, metricsResult.status, failures);
  const summaryStats = buildSummaryStats(streams, cameras, pipelines, metricsPayload);
  const timelineItems = buildTimeline(metricsPayload);
  const pipelineWatch = buildPipelineWatch(streams);
  const streamGallery = buildStreamGallery(streams);

  return {
    meta,
    summaryStats,
    timelineItems,
    pipelineWatch,
    streamGallery
  };
}

function buildDashboardMeta(
  streamsStatus: PromiseSettledResult<unknown>['status'],
  camerasStatus: PromiseSettledResult<unknown>['status'],
  pipelinesStatus: PromiseSettledResult<unknown>['status'],
  metricsStatus: PromiseSettledResult<unknown>['status'],
  failures: number
): DashboardFetchMeta {
  const statusFor = (status: PromiseSettledResult<unknown>['status']): DashboardSourceStatus => (status === 'fulfilled' ? 'ok' : 'error');
  return {
    fetchedAt: new Date().toISOString(),
    failures,
    sources: {
      streams: statusFor(streamsStatus),
      cameras: statusFor(camerasStatus),
      pipelines: statusFor(pipelinesStatus),
      metrics: statusFor(metricsStatus)
    }
  };
}

function buildSummaryStats(
  streams: StreamInfo[],
  cameras: ProbedDevice[],
  pipelines: PipelineSummary[],
  metrics: DeviceMetrics | null
): SummaryStat[] {
  const healthStatus = metrics ? 'OK' : 'Unknown';

  return [
    {
      label: 'Active Sessions',
      value: String(streams.length)
    },
    {
      label: 'Saved Pipelines',
      value: String(pipelines.length)
    },
    {
      label: 'Connected Cameras',
      value: String(cameras.length)
    },
    {
      label: 'Device Health',
      value: healthStatus
    }
  ];
}

function buildTimeline(metrics: DeviceMetrics | null): TimelineItem[] {
  const checkedLabel = formatTimestamp(new Date().toISOString());
  const issues: Array<{ code?: string; description?: string }> = [];
  const items: TimelineItem[] = [
    {
      title: 'Health check',
      time: checkedLabel,
      detail: metrics ? 'Status · OK' : 'Status unavailable'
    }
  ];

  if (issues.length === 0) {
    items.push({
      title: 'All systems nominal',
      time: checkedLabel,
      detail: 'No open issues reported'
    });
    return items;
  }

  for (const issue of issues) {
    items.push({
      title: titleCase(issue?.code ?? 'Issue'),
      time: checkedLabel,
      detail: issue?.description ?? 'No description provided'
    });
  }

  return items;
}

function buildPipelineWatch(slots: StreamInfo[]): PipelineWatchEntry[] {
  return slots.slice(0, 5).map((slot) => {
    const fps = 0;
    const latencyMs = null;
    const state = slot.status?.state === 'disabled' ? 'Degraded' : 'Healthy';
    return {
      name: resolveStreamLabel(slot, 'Stream'),
      fps: typeof fps === 'number' && Number.isFinite(fps) ? Math.round(fps) : 0,
      latency: latencyMs ? `${latencyMs} ms` : 'n/a',
      state
    };
  });
}

function buildStreamGallery(slots: StreamInfo[]): StreamGalleryItem[] {
  if (!slots.length) return [];
  return slots.slice(0, 6).map((slot) => {
    const status = slot.status?.state === 'disabled' ? 'degraded' : 'live';
    const recordingActive = Boolean(slot.status?.recording_active);
    const recordingSinceMs = slot.status?.recording_since_ms ?? null;
    return {
      name: resolveStreamLabel(slot, 'Stream'),
      status,
      captureSessionId: slot.id ?? null,
      captureSessionAlias: resolveStreamAlias(slot),
      cameraUid: null,
      recordingActive,
      recordingSinceMs
    };
  });
}

function formatTimestamp(input: string | null | undefined): string {
  if (!input) return 'n/a';
  const parsed = Date.parse(input);
  if (Number.isNaN(parsed)) return 'n/a';
  return new Date(parsed).toLocaleString();
}

function titleCase(input: string | undefined | null): string {
  if (!input || typeof input !== 'string') return '';
  return input
    .split(/\s+/)
    .map((token) => token.charAt(0).toUpperCase() + token.slice(1).toLowerCase())
    .join(' ');
}
