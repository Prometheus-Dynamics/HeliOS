import { describe, expect, test } from 'bun:test';

import {
  createStreamPreviewController,
  createStreamPreviewRuntimeState,
  type StreamPreviewConfigSnapshot,
  type StreamPreviewConnectionStatus,
  type StreamPreviewControllerDeps
} from './streamPreviewController';

const createConfig = (): StreamPreviewConfigSnapshot => ({
  captureSessionId: 'cam-1',
  captureSessionAlias: 'Front Camera',
  cameraUid: 'front-cam',
  pipelineId: null,
  pipelineOutput: null,
  previewFormat: 'mjpeg' as const,
  autoPlay: false,
  canPreview: true,
  supportsLivePreview: true,
  livePreviewVisible: true,
  status: 'live' as const
});

const createDeps = (format: 'mjpeg' | 'h264' | 'h265' | 'unknown' = 'h264') => {
  let now = 10_000;
  return {
    setNow(value: number) {
      now = value;
    },
    deps: {
      apiUrl: (path: string) => `http://127.0.0.1:5800/v1${path}`,
      now: () => now,
      setTimeout: ((fn: () => void) => {
        fn();
        return 1 as unknown as ReturnType<typeof setTimeout>;
      }) as StreamPreviewControllerDeps['setTimeout'],
      clearTimeout: (() => undefined) as typeof clearTimeout,
      buildHttpCandidateUrls: (url: string) => [url, `${url}&candidate=2`],
      resolveStreamPreviewFormat: async (
        _key: string,
        loader: () => Promise<'mjpeg' | 'h264' | 'h265' | 'unknown' | null>
      ) => (await loader()) ?? null,
      fetchPeerStreamFormat: async () => ({ format }),
      fetchStreamFormat: async () => ({ format })
    } satisfies StreamPreviewControllerDeps
  };
};

const createController = (config: StreamPreviewConfigSnapshot = createConfig()) => {
  const state = createStreamPreviewRuntimeState();
  const deps = createDeps();
  const controller = createStreamPreviewController({
    state,
    readConfig: () => config,
    deps: deps.deps
  });
  return { config, controller, deps, state };
};

describe('stream preview controller', () => {
  test('autoplay starts preview once for a new preview key', () => {
    const { config, controller, state } = createController();
    config.autoPlay = true;

    controller.syncSessionTarget();
    controller.syncAutoPlay();

    expect(state.isPlaying).toBe(true);
    expect(state.previewUrl).toContain('/v1/streams/cam-1/preview');
    expect(state.previewCandidates).toHaveLength(2);

    const previousNonce = state.previewNonce;
    controller.syncAutoPlay();
    expect(state.previewNonce).toBe(previousNonce);
  });

  test('visibility demand tears down and restores live preview from the shared runtime', () => {
    const { config, controller, state } = createController();

    controller.startPreview();
    expect(state.previewUrl).toContain('/v1/streams/cam-1/preview');

    config.livePreviewVisible = false;
    controller.syncVisibilityDemand();
    expect(state.isPlaying).toBe(true);
    expect(state.previewUrl).toBeNull();
    expect(state.previewCandidates).toEqual([]);

    config.livePreviewVisible = true;
    controller.syncVisibilityDemand();
    expect(state.previewUrl).toContain('/v1/streams/cam-1/preview');
  });

  test('session changes reset playback state before the next viewer attaches', () => {
    const { config, controller, deps, state } = createController();

    controller.startPreview();
    expect(state.isPlaying).toBe(true);

    deps.setNow(20_000);
    config.captureSessionId = 'cam-2';
    controller.syncSessionTarget();

    expect(state.isPlaying).toBe(false);
    expect(state.previewUrl).toBeNull();
    expect(state.frameUrl).toBeNull();
    expect(state.lastCaptureSessionId).toBe('cam-2');
    expect(state.lastSwitchAt).toBe(20_000);
  });

  test('coming back online rebuilds preview transport candidates from the shared controller', () => {
    const { controller, state } = createController();

    controller.startPreview();
    state.previewUrl = null;
    controller.syncConnectionStatus('offline' as StreamPreviewConnectionStatus);
    controller.syncConnectionStatus('online' as StreamPreviewConnectionStatus);

    expect(state.previewUrl).toContain('/v1/streams/cam-1/preview');
    expect(state.previewError).toBeNull();
  });

  test('auto format resolution is shared through the controller instead of per-surface guesswork', async () => {
    const { config, controller, state } = createController();
    config.previewFormat = 'auto';

    await controller.resolveConfiguredFormat();

    expect(state.resolvedFormat).toBe('h264');
  });
});
