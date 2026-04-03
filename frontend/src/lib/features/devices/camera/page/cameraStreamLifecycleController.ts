import type { StreamInfo, StreamManifest, ControlMeta } from '$lib/api/client';
import type { DeviceService } from '$lib/api/client';
import type { StreamsApi } from '$lib/api/streamsApi';
import { connectStreamControls, type StreamControlSocket } from '$lib/api/streamControls';
import { connectStreamUpdates } from '$lib/api/streamUpdates';
import { resolveStreamInfo } from './cameraStreamLookup';

export type StreamUpdatesSocket = ReturnType<typeof connectStreamUpdates> | null;

type StreamLifecycleState = {
  get stream(): StreamInfo | null;
  set stream(value: StreamInfo | null);
  get streamId(): string;
  get manifestState(): StreamManifest | null;
  set manifestState(value: StreamManifest | null);
  get loading(): boolean;
  set loading(value: boolean);
  get refreshing(): boolean;
  set refreshing(value: boolean);
  get stopping(): boolean;
  set stopping(value: boolean);
  get error(): string | null;
  set error(value: string | null);
  get streamLookupDebug(): string | null;
  set streamLookupDebug(value: string | null);
  get streamApiBase(): string | null;
  set streamApiBase(value: string | null);
  get controls(): ControlMeta[];
  set controls(value: ControlMeta[]);
  get controlState(): Record<number, number | boolean | null>;
  set controlState(value: Record<number, number | boolean | null>);
  get controlAppliedState(): Record<number, number | boolean | null>;
  set controlAppliedState(value: Record<number, number | boolean | null>);
  get controlSocket(): StreamControlSocket | null;
  set controlSocket(value: StreamControlSocket | null);
  get controlSocketStreamId(): string | null;
  set controlSocketStreamId(value: string | null);
  get streamUpdatesSocket(): StreamUpdatesSocket;
  set streamUpdatesSocket(value: StreamUpdatesSocket);
  get streamUpdatesSocketStreamId(): string | null;
  set streamUpdatesSocketStreamId(value: string | null);
};

type StreamLifecycleDeps = {
  streamsApi: typeof StreamsApi;
  deviceService: typeof DeviceService;
  getHttpClientBase: () => string | null;
  toaster: {
    success: (payload: { title: string; description?: string }) => void;
  };
  reportError: (args: { title: string; error: unknown; fallback: string }) => void;
  loadBackends: (manifest: StreamManifest | null) => Promise<void>;
  loadCodecs: (manifest: StreamManifest | null) => Promise<void>;
  applyManifestSelections: (manifest: StreamManifest | null) => void;
  refreshCalibrationImages: () => Promise<void>;
  refreshCalibrationImportSources: () => Promise<void>;
  refreshIpaStatus: () => Promise<void>;
  seedControlState: (entries: ControlMeta[]) => Record<number, number | boolean | null>;
};

export function createCameraStreamLifecycleController(state: StreamLifecycleState, deps: StreamLifecycleDeps) {
  async function refresh(): Promise<void> {
    if (state.refreshing) return;
    state.refreshing = true;
    state.error = null;
    try {
      const effectiveId = state.stream?.id ?? state.streamId;
      state.streamLookupDebug = null;
      state.streamApiBase = deps.getHttpClientBase();
      const lookup = await resolveStreamInfo({ effectiveId, streamsApi: deps.streamsApi, deviceService: deps.deviceService });
      if (lookup.error) {
        state.streamLookupDebug = lookup.debug;
        state.error = lookup.error;
        return;
      }
      state.streamLookupDebug = lookup.debug;
      const info = lookup.info;
      state.stream = info;
      state.manifestState = info?.manifest ?? null;

      const [controlResp] = await Promise.all([
        deps.streamsApi.getControls({ id: info?.id ?? effectiveId }).catch(() => [])
      ]);
      state.controls = controlResp ?? [];
      state.controlState = deps.seedControlState(state.controls);
      state.controlAppliedState = { ...state.controlState };

      state.loading = false;
      await Promise.allSettled([deps.loadBackends(state.manifestState), deps.loadCodecs(state.manifestState)]);
      deps.applyManifestSelections(state.manifestState);
      await Promise.allSettled([deps.refreshCalibrationImages(), deps.refreshCalibrationImportSources(), deps.refreshIpaStatus()]);
    } catch (err) {
      console.error('Failed to load stream info', err);
      state.error = state.streamLookupDebug
        ? `Stream unavailable or not running.\n\nAPI: ${state.streamApiBase ?? 'unknown'}\n\nDebug:\n${state.streamLookupDebug}`
        : `Stream unavailable or not running.${state.streamApiBase ? `\n\nAPI: ${state.streamApiBase}` : ''}`;
    } finally {
      state.loading = false;
      state.refreshing = false;
    }
  }

  function closeControlSocket(): void {
    if (state.controlSocket) {
      state.controlSocket.close();
      state.controlSocket = null;
      state.controlSocketStreamId = null;
    }
  }

  function ensureControlSocket(streamId: string): void {
    if (state.controlSocket && state.controlSocketStreamId === streamId) return;
    closeControlSocket();
    state.controlSocket = connectStreamControls(streamId, {
      onError: (message) => {
        console.warn(message);
        deps.reportError({
          title: 'Stream control socket error',
          error: new Error(message),
          fallback: message
        });
      }
    });
    state.controlSocketStreamId = streamId;
  }

  function closeStreamUpdatesSocket(): void {
    if (state.streamUpdatesSocket) {
      state.streamUpdatesSocket.close();
      state.streamUpdatesSocket = null;
      state.streamUpdatesSocketStreamId = null;
    }
  }

  function ensureStreamUpdatesSocket(streamId: string): void {
    if (state.streamUpdatesSocket && state.streamUpdatesSocketStreamId === streamId && state.streamUpdatesSocket.ready()) return;
    closeStreamUpdatesSocket();
    state.streamUpdatesSocket = connectStreamUpdates(streamId, {
      onError: (message) => {
        console.warn(message);
        deps.reportError({
          title: 'Stream updates socket error',
          error: new Error(message),
          fallback: message
        });
      }
    });
    state.streamUpdatesSocketStreamId = streamId;
  }

  async function awaitStreamUpdatesSocket(streamId: string, timeoutMs = 500): Promise<StreamUpdatesSocket> {
    ensureStreamUpdatesSocket(streamId);
    const socket = state.streamUpdatesSocket;
    if (!socket) return null;
    if (socket.ready()) return socket;
    const started = typeof performance !== 'undefined' ? performance.now() : Date.now();
    while (true) {
      await new Promise((resolve) => setTimeout(resolve, 50));
      if (socket.ready()) return socket;
      const now = typeof performance !== 'undefined' ? performance.now() : Date.now();
      if (now - started >= timeoutMs) break;
    }
    return socket.ready() ? socket : null;
  }

  async function stopStream(): Promise<void> {
    if (!state.stream) return;
    if (state.stopping) return;
    state.stopping = true;
    try {
      await deps.streamsApi.deleteStream({ id: state.stream.id });
      deps.toaster.success({ title: 'Stream stopped', description: state.stream.id });
      state.stream = null;
    } catch (err) {
      console.error('Failed to stop stream', err);
      deps.reportError({
        title: 'Stop failed',
        error: err,
        fallback: 'Unable to stop the stream right now.'
      });
    } finally {
      state.stopping = false;
    }
  }

  return {
    refresh,
    closeControlSocket,
    ensureControlSocket,
    closeStreamUpdatesSocket,
    ensureStreamUpdatesSocket,
    awaitStreamUpdatesSocket,
    stopStream
  };
}
