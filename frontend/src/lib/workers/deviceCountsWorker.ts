import type { CameraStatus } from '$lib/types/devices';
import { countCameraStatuses } from '$lib/features/devices/utils';

type DeviceCountsPayload = {
  requestId: number;
  statuses: CameraStatus[];
};

type DeviceCountsResponse = {
  requestId: number;
  counts: Record<CameraStatus, number>;
};

self.onmessage = (event: MessageEvent<DeviceCountsPayload>) => {
  const { requestId, statuses } = event.data;
  const counts = countCameraStatuses(Array.isArray(statuses) ? statuses : []);
  const response: DeviceCountsResponse = { requestId, counts };
  self.postMessage(response);
};
