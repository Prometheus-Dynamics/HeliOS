import { describe, expect, test } from 'bun:test';

import { buildStreamPreviewProps, normalizeStreamViewerStatus } from './streamViewerSurface';

const baseSource = {
  name: 'Front Camera',
  status: 'live',
  captureSessionId: 'cam-1',
  captureSessionAlias: 'Front Camera',
  cameraUid: 'front-cam'
};

describe('stream viewer surface policy', () => {
  test('normalizes unknown statuses to idle', () => {
    expect(normalizeStreamViewerStatus('live')).toBe('live');
    expect(normalizeStreamViewerStatus('DEGRADED')).toBe('degraded');
    expect(normalizeStreamViewerStatus('mystery')).toBe('idle');
    expect(normalizeStreamViewerStatus(null)).toBe('idle');
  });

  test('builds dashboard card props from the shared policy', () => {
    expect(buildStreamPreviewProps(baseSource, 'dashboard-card')).toEqual({
      name: 'Front Camera',
      status: 'live',
      recording: false,
      captureSessionId: 'cam-1',
      captureSessionAlias: 'Front Camera',
      cameraUid: 'front-cam',
      pipelineId: null,
      pipelineOutput: null,
      previewFormat: 'auto',
      autoPlay: false,
      enablePopout: true,
      fitMode: 'cover',
      enforceAspect: true,
      hideControls: false,
      fillParent: false,
      showCaption: true,
      showFrame: true
    });
  });

  test('builds device detail props from the shared policy', () => {
    expect(
      buildStreamPreviewProps(
        {
          ...baseSource,
          status: 'degraded',
          recordingActive: true,
          pipelineId: 'pipeline-a',
          pipelineOutput: 'raw'
        },
        'device-detail'
      )
    ).toEqual({
      name: 'Front Camera',
      status: 'degraded',
      recording: true,
      captureSessionId: 'cam-1',
      captureSessionAlias: 'Front Camera',
      cameraUid: 'front-cam',
      pipelineId: 'pipeline-a',
      pipelineOutput: 'raw',
      previewFormat: 'auto',
      autoPlay: false,
      enablePopout: false,
      fitMode: 'contain',
      enforceAspect: false,
      hideControls: false,
      fillParent: true,
      showCaption: false,
      showFrame: false
    });
  });

  test('builds floating and pipeline policies without per-surface overrides', () => {
    expect(buildStreamPreviewProps(baseSource, 'floating')).toMatchObject({
      autoPlay: true,
      enablePopout: false,
      fitMode: 'contain',
      fillParent: true,
      showCaption: false,
      showFrame: false
    });

    expect(buildStreamPreviewProps(baseSource, 'pipeline-modal')).toMatchObject({
      autoPlay: false,
      enablePopout: false,
      hideControls: true,
      fitMode: 'cover',
      fillParent: true,
      showCaption: false,
      showFrame: false
    });

    expect(buildStreamPreviewProps(baseSource, 'pipeline-tune')).toMatchObject({
      autoPlay: false,
      enablePopout: true,
      hideControls: false,
      fitMode: 'contain',
      showCaption: false,
      showFrame: false
    });
  });
});
