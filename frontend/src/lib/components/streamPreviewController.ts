import { buildHttpCandidateUrls } from '$lib/api/httpCandidates';
import { apiUrl } from '$lib/api/httpClient';
import { fetchPeerStreamFormat } from '$lib/api/peers';
import { resolveStreamPreviewFormat, type StreamPreviewFormat } from '$lib/api/streamPreviewFormat';
import { StreamsApi } from '$lib/api/streamsApi';
import type { FloatingStreamStatus } from '$lib/stores/floatingStreamViewer';
import { SvelteURLSearchParams } from 'svelte/reactivity';

export type ResolvedPreviewFormat = 'mjpeg' | 'h264' | 'h265' | 'unknown';
export type StreamPreviewConnectionStatus = 'unknown' | 'online' | 'offline' | 'degraded';
export type StreamPreviewExplicitFormat = 'auto' | 'mjpeg' | 'h264' | 'h265';

type PeerStreamRef = { peerId: string; streamId: string };

type StreamPreviewConfigSnapshot = {
  captureSessionId: string | null;
  captureSessionAlias: string | null;
  cameraUid: string | null;
  pipelineId: string | null;
  pipelineOutput: string | null;
  previewFormat: StreamPreviewExplicitFormat;
  autoPlay: boolean;
  canPreview: boolean;
  supportsLivePreview: boolean;
  livePreviewVisible: boolean;
  status: FloatingStreamStatus;
};

export type StreamPreviewRuntimeState = {
  isPlaying: boolean;
  previewUrl: string | null;
  frameUrl: string | null;
  previewError: string | null;
  lastKey: string | null;
  frameKey: string | null;
  lastAutoPlayKey: string | null;
  lastCaptureSessionId: string | null;
  lastSwitchAt: number | null;
  previewNonce: number;
  frameNonce: number;
  previewCandidates: string[];
  previewCandidateIndex: number;
  frameCandidates: string[];
  frameCandidateIndex: number;
  documentVisible: boolean;
  viewportVisible: boolean;
  resolvedFormat: ResolvedPreviewFormat;
};

export const createStreamPreviewRuntimeState = (): StreamPreviewRuntimeState => ({
  isPlaying: false,
  previewUrl: null,
  frameUrl: null,
  previewError: null,
  lastKey: null,
  frameKey: null,
  lastAutoPlayKey: null,
  lastCaptureSessionId: null,
  lastSwitchAt: null,
  previewNonce: 0,
  frameNonce: 0,
  previewCandidates: [],
  previewCandidateIndex: 0,
  frameCandidates: [],
  frameCandidateIndex: 0,
  documentVisible: true,
  viewportVisible: true,
  resolvedFormat: 'mjpeg'
});

const RECONNECT_BASE_DELAY_MS = 700;
const RECONNECT_MAX_DELAY_MS = 4000;
const STALL_BANNER_DELAY_MS = 3500;
const FRAME_RETRY_DELAY_MS = 1200;

function parsePeerStreamRef(raw: string | null | undefined): PeerStreamRef | null {
  const value = String(raw ?? '').trim();
  if (!value.startsWith('peer:')) return null;
  const rest = value.slice('peer:'.length);
  const splitIndex = rest.indexOf(':');
  if (splitIndex <= 0 || splitIndex >= rest.length - 1) return null;
  const peerId = rest.slice(0, splitIndex).trim();
  const streamId = rest.slice(splitIndex + 1).trim();
  if (!peerId || !streamId) return null;
  return { peerId, streamId };
}

function peerProxyPath(peer: PeerStreamRef, endpoint: 'preview' | 'frame' | 'format'): string {
  return `/peers/${encodeURIComponent(peer.peerId)}/streams/${encodeURIComponent(peer.streamId)}/${endpoint}`;
}

function normalizedPipelineOutputTag(raw: string | null | undefined): string {
  return String(raw ?? '').trim().toLowerCase();
}

function usesOutputOverridePreview(config: StreamPreviewConfigSnapshot): boolean {
  const output = normalizedPipelineOutputTag(config.pipelineOutput);
  if (config.pipelineId?.trim()) return true;
  if (!output) return false;
  return output !== 'frame' && output !== 'raw' && output !== 'undistorted';
}

function previewFormatCacheKey(config: StreamPreviewConfigSnapshot, peer: PeerStreamRef | null, sessionId: string): string {
  const pipelineTag = String(config.pipelineId ?? '').trim();
  const outputTag = normalizedPipelineOutputTag(config.pipelineOutput);
  const base = peer ? `peer:${peer.peerId}:${peer.streamId}` : `stream:${sessionId}`;
  return `${base}:${pipelineTag}:${outputTag}`;
}

function mapEncodedInfoFormat(raw: unknown): StreamPreviewFormat | null {
  if (!raw || typeof raw !== 'object' || !('format' in raw)) return null;
  const format = (raw as { format?: unknown }).format;
  if (format === 'mjpeg' || format === 'h264' || format === 'h265') return format;
  if (format === 'unknown') return 'unknown';
  return null;
}

export const createStreamPreviewController = (options: {
  state: StreamPreviewRuntimeState;
  readConfig: () => StreamPreviewConfigSnapshot;
}) => {
  const { state, readConfig } = options;
  let reconnectAttempt = 0;
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  let stallBannerTimer: ReturnType<typeof setTimeout> | null = null;
  let frameRetryTimer: ReturnType<typeof setTimeout> | null = null;
  let lastStatus: FloatingStreamStatus | null = null;
  let lastConnectionStatus: StreamPreviewConnectionStatus | null = null;

  const clearReconnectTimer = (): void => {
    if (!reconnectTimer) return;
    clearTimeout(reconnectTimer);
    reconnectTimer = null;
  };

  const clearStallBannerTimer = (): void => {
    if (!stallBannerTimer) return;
    clearTimeout(stallBannerTimer);
    stallBannerTimer = null;
  };

  const clearFrameRetryTimer = (): void => {
    if (!frameRetryTimer) return;
    clearTimeout(frameRetryTimer);
    frameRetryTimer = null;
  };

  const resetReconnectAttempts = (): void => {
    reconnectAttempt = 0;
  };

  const incrementReconnectAttempt = (): void => {
    reconnectAttempt = Math.min(reconnectAttempt + 1, 6);
  };

  const nextReconnectDelay = (): number => {
    if (reconnectAttempt <= 0) {
      return RECONNECT_BASE_DELAY_MS;
    }
    const delay = RECONNECT_BASE_DELAY_MS * 2 ** (reconnectAttempt - 1);
    return Math.min(delay, RECONNECT_MAX_DELAY_MS);
  };

  const nextNonce = (current: number): number => {
    const now = Date.now();
    return now <= current ? current + 1 : now;
  };

  const advancePreviewNonce = (): void => {
    state.previewNonce = nextNonce(state.previewNonce);
  };

  const advanceFrameNonce = (): void => {
    state.frameNonce = nextNonce(state.frameNonce);
  };

  const buildPreviewUrl = (): string | null => {
    const config = readConfig();
    if (state.resolvedFormat === 'unknown') {
      return null;
    }
    const params = new SvelteURLSearchParams();
    if (config.pipelineId?.trim()) params.set('pipeline', config.pipelineId.trim());
    if (config.pipelineOutput?.trim()) params.set('output', config.pipelineOutput.trim());
    if (state.previewNonce > 0) params.set('cb', String(state.previewNonce));
    const suffix = params.toString();
    const query = suffix.length ? `?${suffix}` : '';
    const peer = parsePeerStreamRef(config.captureSessionId);
    if (peer) {
      return apiUrl(`${peerProxyPath(peer, 'preview')}${query}`);
    }
    if (state.resolvedFormat === 'mjpeg') {
      const ref = config.captureSessionId ?? config.captureSessionAlias;
      if (!ref) return null;
      return apiUrl(`/streams/${encodeURIComponent(ref)}/preview${query}`);
    }
    if (!config.captureSessionId) return null;
    return apiUrl(`/streams/${encodeURIComponent(config.captureSessionId)}/preview${query}`);
  };

  const buildFrameUrl = (): string | null => {
    const config = readConfig();
    const ref = config.captureSessionId;
    if (!ref) return null;
    const peer = parsePeerStreamRef(ref);
    const params = new SvelteURLSearchParams({ t: String(state.frameNonce) });
    if (config.pipelineId?.trim()) params.set('pipeline', config.pipelineId.trim());
    if (config.pipelineOutput?.trim()) params.set('output', config.pipelineOutput.trim());
    if (peer) {
      return apiUrl(`${peerProxyPath(peer, 'frame')}?${params.toString()}`);
    }
    return apiUrl(`/streams/${encodeURIComponent(ref)}/frame?${params.toString()}`);
  };

  const buildPreviewUrlCandidates = (): string[] => {
    const url = buildPreviewUrl();
    return url ? buildHttpCandidateUrls(url) : [];
  };

  const buildFrameUrlCandidates = (): string[] => {
    const url = buildFrameUrl();
    return url ? buildHttpCandidateUrls(url) : [];
  };

  const switchToNextPreviewCandidate = (): boolean => {
    const nextIndex = state.previewCandidateIndex + 1;
    if (nextIndex >= state.previewCandidates.length) {
      return false;
    }
    const nextUrl = state.previewCandidates[nextIndex];
    if (!nextUrl) {
      return false;
    }
    state.previewCandidateIndex = nextIndex;
    state.previewUrl = nextUrl;
    return true;
  };

  const previewKey = (): string | null => {
    const config = readConfig();
    const ref = config.captureSessionId;
    if (!ref) return null;
    const base = config.cameraUid?.trim() ? `${ref}:${config.cameraUid.trim()}` : ref;
    const pipelineTag = config.pipelineId?.trim() ? config.pipelineId.trim() : '';
    const outputTag = config.pipelineOutput?.trim() ? config.pipelineOutput.trim() : '';
    return `${base}:${state.resolvedFormat}:${pipelineTag}:${outputTag}`;
  };

  const rebuildPreviewCandidates = (advanceNonce = false): boolean => {
    if (advanceNonce) {
      advancePreviewNonce();
    }
    const candidates = buildPreviewUrlCandidates();
    state.previewCandidates = candidates;
    state.previewCandidateIndex = 0;
    state.previewUrl = candidates[0] ?? null;
    return Boolean(state.previewUrl);
  };

  const scheduleReconnect = (immediate = false): void => {
    const config = readConfig();
    if (!state.isPlaying || !config.supportsLivePreview || !config.canPreview || !config.livePreviewVisible) return;
    clearReconnectTimer();
    const delay = immediate ? 0 : nextReconnectDelay();
    reconnectTimer = setTimeout(() => {
      reconnectTimer = null;
      const latest = readConfig();
      if (!state.isPlaying || !latest.supportsLivePreview || !latest.canPreview) return;
      if (!switchToNextPreviewCandidate() && !rebuildPreviewCandidates(true)) {
        incrementReconnectAttempt();
        scheduleReconnect();
        return;
      }
    }, delay);
  };

  const scheduleFrameRetry = (immediate = false): void => {
    const config = readConfig();
    if (state.isPlaying || !config.canPreview) return;
    clearFrameRetryTimer();
    frameRetryTimer = setTimeout(() => {
      frameRetryTimer = null;
      const latest = readConfig();
      if (state.isPlaying || !latest.canPreview) return;
      refreshFrame(true);
    }, immediate ? 0 : FRAME_RETRY_DELAY_MS);
  };

  const refreshFrame = (force = false): void => {
    const key = previewKey();
    if (!key) {
      state.frameUrl = null;
      state.frameKey = null;
      state.frameCandidates = [];
      state.frameCandidateIndex = 0;
      return;
    }
    if (!force && key === state.frameKey) {
      return;
    }
    advanceFrameNonce();
    const candidates = buildFrameUrlCandidates();
    state.frameCandidates = candidates;
    state.frameCandidateIndex = 0;
    const url = candidates[0] ?? null;
    if (url) {
      state.frameUrl = url;
      state.frameKey = key;
      clearFrameRetryTimer();
    } else {
      state.frameUrl = null;
      state.frameKey = null;
      scheduleFrameRetry();
    }
  };

  const startPreview = (): void => {
    const config = readConfig();
    if (!config.canPreview) return;
    if (!rebuildPreviewCandidates(true)) return;
    clearReconnectTimer();
    clearStallBannerTimer();
    state.isPlaying = true;
    state.previewError = null;
    state.lastKey = previewKey();
    state.frameUrl = null;
    state.frameKey = null;
    resetReconnectAttempts();
  };

  const stopPreview = (): void => {
    state.previewUrl = null;
    state.previewCandidates = [];
    state.previewCandidateIndex = 0;
    state.isPlaying = false;
    state.previewError = null;
    state.lastKey = null;
    clearReconnectTimer();
    clearStallBannerTimer();
    clearFrameRetryTimer();
    resetReconnectAttempts();
    refreshFrame(true);
  };

  const resolveConfiguredFormat = async (): Promise<void> => {
    const config = readConfig();
    if (config.previewFormat === 'mjpeg' || config.previewFormat === 'h264' || config.previewFormat === 'h265') {
      state.resolvedFormat = config.previewFormat;
      return;
    }
    if (usesOutputOverridePreview(config)) {
      state.resolvedFormat = 'mjpeg';
      return;
    }
    if (!config.captureSessionId) return;
    const sessionId = config.captureSessionId;
    const peer = parsePeerStreamRef(sessionId);
    try {
      const mapped = await resolveStreamPreviewFormat(previewFormatCacheKey(config, peer, sessionId), async () => {
        const json = peer
          ? await fetchPeerStreamFormat(peer.peerId, peer.streamId)
          : await StreamsApi.streamFormat({ id: sessionId });
        return mapEncodedInfoFormat(json);
      });
      if (mapped && readConfig().captureSessionId === sessionId) {
        state.resolvedFormat = mapped;
      }
    } catch {
      // Keep the current/default format.
    }
  };

  const syncSessionTarget = (): void => {
    const config = readConfig();
    const current = config.captureSessionId ?? null;
    if (current === state.lastCaptureSessionId) return;
    state.lastCaptureSessionId = current;
    state.lastSwitchAt = Date.now();
    state.previewError = null;
    state.lastAutoPlayKey = null;
    state.lastKey = null;
    state.previewUrl = null;
    state.previewCandidates = [];
    state.previewCandidateIndex = 0;
    state.frameUrl = null;
    state.frameKey = null;
    state.frameCandidates = [];
    state.frameCandidateIndex = 0;
    state.isPlaying = false;
    clearReconnectTimer();
    clearStallBannerTimer();
    clearFrameRetryTimer();
    if (config.previewFormat === 'auto') {
      state.resolvedFormat = 'mjpeg';
    }
  };

  const togglePreview = (): void => {
    const config = readConfig();
    if (!config.canPreview || !config.supportsLivePreview) return;
    if (state.isPlaying) {
      stopPreview();
      return;
    }
    startPreview();
  };

  const handleStreamError = (message?: string): void => {
    if (!state.isPlaying) return;
    if (switchToNextPreviewCandidate()) {
      clearStallBannerTimer();
      state.previewError = null;
      return;
    }
    state.previewUrl = null;
    const resolvedMessage = message ?? 'Stream preview error';
    const normalizedMessage = resolvedMessage.toLowerCase();
    const recentSwitch = state.lastSwitchAt != null && Date.now() - state.lastSwitchAt < 2000;
    const transientAbort = normalizedMessage.includes('abort');
    const transient404 = recentSwitch && normalizedMessage.includes('404');
    if (transientAbort || transient404) {
      refreshFrame(true);
      scheduleReconnect(true);
      return;
    }
    incrementReconnectAttempt();
    if (normalizedMessage.includes('stalled')) {
      clearStallBannerTimer();
      stallBannerTimer = setTimeout(() => {
        stallBannerTimer = null;
        if (!state.isPlaying || state.previewError) return;
        state.previewError = resolvedMessage;
      }, STALL_BANNER_DELAY_MS);
    } else {
      clearStallBannerTimer();
      state.previewError = resolvedMessage;
    }
    refreshFrame(true);
    scheduleReconnect();
  };

  const handleStreamLoad = (): void => {
    if (!state.isPlaying) return;
    clearReconnectTimer();
    clearStallBannerTimer();
    resetReconnectAttempts();
    state.previewError = null;
    state.frameUrl = null;
  };

  const syncPreviewKey = (): void => {
    if (!state.isPlaying) return;
    const key = previewKey();
    if (key === state.lastKey) return;
    if (rebuildPreviewCandidates(true)) {
      clearReconnectTimer();
      state.lastKey = key;
      resetReconnectAttempts();
    } else {
      stopPreview();
    }
  };

  const syncAutoPlay = (): void => {
    const config = readConfig();
    if (!config.autoPlay) return;
    const key = previewKey();
    if (!key || key === state.lastAutoPlayKey) return;
    if (!config.canPreview || !config.supportsLivePreview) return;
    state.lastAutoPlayKey = key;
    startPreview();
  };

  const syncVisibilityDemand = (): void => {
    const config = readConfig();
    if (!state.isPlaying || !config.supportsLivePreview || !config.canPreview) return;
    if (!config.livePreviewVisible) {
      state.previewUrl = null;
      state.previewCandidates = [];
      state.previewCandidateIndex = 0;
      clearReconnectTimer();
      clearStallBannerTimer();
      return;
    }
    if (!state.previewUrl && rebuildPreviewCandidates(true)) {
      clearReconnectTimer();
      state.lastKey = previewKey();
      resetReconnectAttempts();
    }
  };

  const syncStatusDemand = (): void => {
    const config = readConfig();
    const currentStatus = config.status;
    if (!state.isPlaying || !config.supportsLivePreview || !config.canPreview || !config.livePreviewVisible) {
      lastStatus = currentStatus;
      return;
    }
    if ((currentStatus === 'live' || currentStatus === 'degraded') && lastStatus !== currentStatus) {
      resetReconnectAttempts();
    }
    if ((currentStatus === 'live' || currentStatus === 'degraded') && !state.previewUrl) {
      scheduleReconnect(true);
    }
    lastStatus = currentStatus;
  };

  const syncConnectionStatus = (backendStatus: StreamPreviewConnectionStatus): void => {
    const config = readConfig();
    const cameOnline =
      backendStatus === 'online' &&
      lastConnectionStatus !== null &&
      lastConnectionStatus !== 'online';
    if (cameOnline && state.isPlaying && config.supportsLivePreview && config.canPreview && config.livePreviewVisible) {
      clearReconnectTimer();
      clearStallBannerTimer();
      resetReconnectAttempts();
      state.previewError = null;
      if (rebuildPreviewCandidates(true)) {
        state.lastKey = previewKey();
      }
    }
    lastConnectionStatus = backendStatus;
  };

  const mount = (previewHost: HTMLElement | null): (() => void) => {
    const onVisibilityChange = () => {
      state.documentVisible = typeof document === 'undefined' ? true : !document.hidden;
    };
    onVisibilityChange();
    if (typeof document !== 'undefined') {
      document.addEventListener('visibilitychange', onVisibilityChange);
    }

    let observer: IntersectionObserver | null = null;
    if (typeof IntersectionObserver !== 'undefined' && previewHost) {
      observer = new IntersectionObserver(
        (entries) => {
          const entry = entries[0];
          state.viewportVisible = Boolean(entry?.isIntersecting || (entry?.intersectionRatio ?? 0) > 0);
        },
        {
          rootMargin: '200px 0px 200px 0px',
          threshold: 0.01
        }
      );
      observer.observe(previewHost);
    } else {
      state.viewportVisible = true;
    }

    return () => {
      if (typeof document !== 'undefined') {
        document.removeEventListener('visibilitychange', onVisibilityChange);
      }
      observer?.disconnect();
    };
  };

  const handleFrameLoad = (): void => {
    clearFrameRetryTimer();
  };

  const handleFrameError = (): void => {
    const nextIndex = state.frameCandidateIndex + 1;
    if (nextIndex < state.frameCandidates.length) {
      state.frameCandidateIndex = nextIndex;
      state.frameUrl = state.frameCandidates[nextIndex] ?? null;
      return;
    }
    state.frameUrl = null;
    scheduleFrameRetry();
  };

  const destroy = (): void => {
    stopPreview();
    clearFrameRetryTimer();
  };

  return {
    destroy,
    handleFrameError,
    handleFrameLoad,
    handleStreamError,
    handleStreamFrame: handleStreamLoad,
    handleStreamLoad,
    mount,
    refreshFrame,
    resolveConfiguredFormat,
    startPreview,
    stopPreview,
    syncAutoPlay,
    syncConnectionStatus,
    syncPreviewKey,
    syncSessionTarget,
    syncStatusDemand,
    syncVisibilityDemand,
    togglePreview
  };
};
