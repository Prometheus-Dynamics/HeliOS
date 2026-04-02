export type StreamViewerStatus = 'live' | 'degraded' | 'offline' | 'idle';
export type StreamViewerFormat = 'auto' | 'mjpeg' | 'h264' | 'h265';
export type ResolvedStreamViewerFormat = Exclude<StreamViewerFormat, 'auto'> | 'unknown';

export type StreamViewerSource = {
  name: string;
  status: string | null | undefined;
  captureSessionId: string | null | undefined;
  captureSessionAlias: string | null | undefined;
  cameraUid: string | null | undefined;
  recordingActive?: boolean;
  recordingSinceMs?: number | null;
  pipelineId?: string | null;
  pipelineOutput?: string | null;
  previewFormat?: StreamViewerFormat;
};

export type StreamViewerVariant =
  | 'dashboard-card'
  | 'device-card'
  | 'device-detail'
  | 'floating'
  | 'pipeline-modal'
  | 'pipeline-tune';

export type StreamPreviewSurfaceProps = {
  name: string;
  status: StreamViewerStatus;
  recording: boolean;
  captureSessionId: string | null;
  captureSessionAlias: string | null;
  cameraUid: string | null;
  pipelineId: string | null;
  pipelineOutput: string | null;
  previewFormat: StreamViewerFormat;
  autoPlay: boolean;
  enablePopout: boolean;
  fitMode: 'cover' | 'contain';
  enforceAspect: boolean;
  hideControls: boolean;
  fillParent: boolean;
  showCaption: boolean;
  showFrame: boolean;
};

type StreamViewerVariantPolicy = Pick<
  StreamPreviewSurfaceProps,
  | 'autoPlay'
  | 'enablePopout'
  | 'fitMode'
  | 'enforceAspect'
  | 'hideControls'
  | 'fillParent'
  | 'showCaption'
  | 'showFrame'
>;

const STREAM_VIEWER_VARIANT_POLICIES: Record<StreamViewerVariant, StreamViewerVariantPolicy> = {
  'dashboard-card': {
    autoPlay: false,
    enablePopout: true,
    fitMode: 'cover',
    enforceAspect: true,
    hideControls: false,
    fillParent: false,
    showCaption: true,
    showFrame: true
  },
  'device-card': {
    autoPlay: false,
    enablePopout: false,
    fitMode: 'cover',
    enforceAspect: false,
    hideControls: true,
    fillParent: false,
    showCaption: true,
    showFrame: true
  },
  'device-detail': {
    autoPlay: false,
    enablePopout: false,
    fitMode: 'contain',
    enforceAspect: false,
    hideControls: false,
    fillParent: true,
    showCaption: false,
    showFrame: false
  },
  floating: {
    autoPlay: true,
    enablePopout: false,
    fitMode: 'contain',
    enforceAspect: false,
    hideControls: false,
    fillParent: true,
    showCaption: false,
    showFrame: false
  },
  'pipeline-modal': {
    autoPlay: false,
    enablePopout: false,
    fitMode: 'cover',
    enforceAspect: false,
    hideControls: true,
    fillParent: true,
    showCaption: false,
    showFrame: false
  },
  'pipeline-tune': {
    autoPlay: false,
    enablePopout: true,
    fitMode: 'contain',
    enforceAspect: true,
    hideControls: false,
    fillParent: false,
    showCaption: false,
    showFrame: false
  }
};

export const normalizeStreamViewerStatus = (raw: string | null | undefined): StreamViewerStatus => {
  switch (String(raw ?? '').trim().toLowerCase()) {
    case 'live':
      return 'live';
    case 'degraded':
      return 'degraded';
    case 'offline':
      return 'offline';
    default:
      return 'idle';
  }
};

export const buildStreamPreviewProps = (
  source: StreamViewerSource,
  variant: StreamViewerVariant
): StreamPreviewSurfaceProps => {
  const policy = STREAM_VIEWER_VARIANT_POLICIES[variant];
  return {
    ...policy,
    name: source.name,
    status: normalizeStreamViewerStatus(source.status),
    recording: Boolean(source.recordingActive),
    captureSessionId: source.captureSessionId ?? null,
    captureSessionAlias: source.captureSessionAlias ?? null,
    cameraUid: source.cameraUid ?? null,
    pipelineId: source.pipelineId ?? null,
    pipelineOutput: source.pipelineOutput ?? null,
    previewFormat: source.previewFormat ?? 'auto'
  };
};
