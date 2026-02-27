export type SummaryStat = {
  label: string;
  value: string;
  detail?: string;
};

export type TimelineItem = {
  title: string;
  time: string;
  detail?: string;
};

export type PipelineState = 'Healthy' | 'Degraded' | 'Verifying' | 'Idle';

export type PipelineWatchEntry = {
  name: string;
  fps: number;
  latency: string;
  state: PipelineState;
};

export type StreamGalleryStatus = 'live' | 'degraded' | 'offline' | 'idle';

export type StreamGalleryItem = {
  name: string;
  status: StreamGalleryStatus;
  captureSessionId: string | null;
  captureSessionAlias: string | null;
  cameraUid: string | null;
  recordingActive?: boolean;
  recordingSinceMs?: number | null;
};

export type DashboardSourceStatus = 'ok' | 'error' | 'unknown';

export type DashboardFetchMeta = {
  fetchedAt: string | null;
  failures: number;
  sources: {
    streams: DashboardSourceStatus;
    cameras: DashboardSourceStatus;
    pipelines: DashboardSourceStatus;
    metrics: DashboardSourceStatus;
  };
};

export type DashboardPayload = {
  meta: DashboardFetchMeta;
  summaryStats: SummaryStat[];
  timelineItems: TimelineItem[];
  pipelineWatch: PipelineWatchEntry[];
  streamGallery: StreamGalleryItem[];
};
