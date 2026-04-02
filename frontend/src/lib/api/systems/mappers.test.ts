import { describe, expect, test } from 'bun:test';

import { emptySystemsRuntime, mapSystemsRuntime } from './mappers';

describe('systems runtime mappers', () => {
  test('maps lib-cv scratch high-water metrics from runtime payloads', () => {
    const runtime = mapSystemsRuntime({
      observability: {
        cv_runtime_scratch_high_water: [
          { name: 'aruco.pose', high_water_bytes: 8192 },
          { name: 'image.gray', high_water_bytes: 16384 }
        ]
      }
    });

    expect(runtime.observability.cvRuntimeScratchHighWater).toEqual([
      { name: 'image.gray', highWaterBytes: 16384 },
      { name: 'aruco.pose', highWaterBytes: 8192 }
    ]);
  });

  test('keeps runtime scratch metrics empty when payload omits them', () => {
    expect(emptySystemsRuntime().observability.cvRuntimeScratchHighWater).toEqual([]);
    expect(mapSystemsRuntime(null).observability.cvRuntimeScratchHighWater).toEqual([]);
  });
});
