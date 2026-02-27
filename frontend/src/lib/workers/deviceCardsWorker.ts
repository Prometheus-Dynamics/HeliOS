import type { CameraCard, PeripheralEntry } from '$lib/types/devices';
import type { CameraCardItem, PeripheralItem } from '$lib/features/devices/types';
import { buildCameraCardsList, buildPeripheralItems } from '$lib/features/devices/utils';

type Payload = {
  requestId: number;
  cameras: CameraCard[];
  peripherals: PeripheralEntry[];
};

type Response = {
  requestId: number;
  cameraCards: CameraCardItem[];
  peripheralItems: PeripheralItem[];
};

self.onmessage = (event: MessageEvent<Payload>) => {
  const { requestId, cameras, peripherals } = event.data;
  const cameraCards = buildCameraCardsList(Array.isArray(cameras) ? cameras : []);
  const peripheralItems = buildPeripheralItems(Array.isArray(peripherals) ? peripherals : []);

  const response: Response = { requestId, cameraCards, peripheralItems };
  self.postMessage(response);
};
