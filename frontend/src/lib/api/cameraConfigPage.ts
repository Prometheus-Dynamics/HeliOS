import type { CameraConfigResponse } from '$lib/types/cameraConfig';
import { StreamsApi } from '$lib/api/streamsApi';

export async function fetchCameraConfigDetail(cameraId: string): Promise<CameraConfigResponse> {
  if (!cameraId || !cameraId.trim()) {
    return { drivers: [], activeDriverId: null, codecDisplayNames: {} };
  }

  try {
    const stream = await StreamsApi.getStream({ id: cameraId });
    if (!stream) {
      return { drivers: [], activeDriverId: null, codecDisplayNames: {} };
    }
  } catch {
    return { drivers: [], activeDriverId: null, codecDisplayNames: {} };
  }

  // The legacy camera configuration payload is not available on the current OpenAPI surface.
  // Return an empty response to keep existing routes/components functional without shims.
  return { drivers: [], activeDriverId: null, codecDisplayNames: {} };
}
