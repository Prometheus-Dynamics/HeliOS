import { describe, expect, test } from 'bun:test';

import {
  buildOwnedStreamRecords,
  findOwnedStreamByEffectiveId,
  normalizeResolvedStreams,
  streamLookupKeys,
  type OwnedStreamRecord
} from './streamResources';
import type { StreamInfo } from '$lib/api/client';

function makeStream(overrides: Partial<StreamInfo> = {}): StreamInfo {
  return {
    id: 'stream-1',
    manifest: {
      identity: {
        display: 'CAM-A',
        keys: ['usb:1-1']
      },
      active_pipeline_id: 'pipe-1',
      active_pipeline_output: 'raw',
      calibration: null,
      pipelines: []
    },
    status: 'live',
    ...overrides
  } as StreamInfo;
}

describe('stream resources', () => {
  test('normalizes resolved stream payloads from arrays and wrapped items', () => {
    const direct = normalizeResolvedStreams([makeStream({ id: 'a' }), null, { nope: true }]);
    const wrapped = normalizeResolvedStreams({
      items: [makeStream({ id: 'b' }), { nope: true }]
    });

    expect(direct.map((entry) => entry.id)).toEqual(['a']);
    expect(wrapped.map((entry) => entry.id)).toEqual(['b']);
  });

  test('builds sorted owned stream records with shared labels and runtime state', () => {
    const records = buildOwnedStreamRecords([
      makeStream({
        id: 'stream-2',
        manifest: {
          identity: { display: 'CAM-B', alias: 'Beta', keys: ['usb:2-2'] },
          active_pipeline_id: 'pipe-b',
          active_pipeline_output: 'undistorted',
          calibration: null,
          pipelines: []
        },
        runtime: {
          capture: {
            state: 'disabled'
          }
        }
      }),
      makeStream({
        id: 'stream-1',
        manifest: {
          identity: { display: 'CAM-A', alias: 'Alpha', keys: ['usb:1-1'] },
          active_pipeline_id: 'pipe-a',
          active_pipeline_output: 'raw',
          calibration: null,
          pipelines: []
        },
        status: 'live'
      })
    ]);

    expect(records.map((entry) => entry.optionLabel)).toEqual(['Alpha · stream-1', 'Beta · stream-2']);
    expect(records[0]).toMatchObject<Partial<OwnedStreamRecord>>({
      id: 'stream-1',
      label: 'Alpha',
      alias: 'Alpha',
      pipelineId: 'pipe-a',
      pipelineOutput: 'raw',
      status: 'live'
    });
    expect(records[1]?.status).toBe('degraded');
  });

  test('matches streams by id, alias, identity keys, and device keys', () => {
    const stream = makeStream({
      id: 'stream-lookup',
      manifest: {
        identity: {
          display: 'CAM-L',
          alias: 'Front Cam',
          keys: ['usb:3-3']
        },
        capture: {
          device_keys: ['pci:0000:00:14.0']
        },
        active_pipeline_id: null,
        active_pipeline_output: null,
        calibration: null,
        pipelines: []
      }
    });

    expect(streamLookupKeys(stream)).toEqual(
      expect.arrayContaining(['stream-lookup', 'Front Cam', 'CAM-L', 'usb:3-3', 'pci:0000:00:14.0'])
    );
    expect(findOwnedStreamByEffectiveId([stream], 'stream-lookup')?.id).toBe('stream-lookup');
    expect(findOwnedStreamByEffectiveId([stream], 'Front Cam')?.id).toBe('stream-lookup');
    expect(findOwnedStreamByEffectiveId([stream], 'pci:0000:00:14.0')?.id).toBe('stream-lookup');
  });
});
