import { writable, type Readable } from 'svelte/store';
import type { CameraCard, CameraStatus, PeripheralEntry } from '$lib/types/devices';
import type { CameraCardItem, CameraStatusCounts, PeripheralItem } from './types';
import { buildCameraCardsList, buildPeripheralItems, countCameraStatuses, EMPTY_CAMERA_COUNTS } from './utils';
import { createDeviceCardsWorker, createDeviceCountsWorker } from '$lib/workers/factories';

type CountsWorkerPayload = { requestId: number; statuses: CameraStatus[] };
type CountsWorkerResponse = { requestId: number; counts: CameraStatusCounts };
type CardsWorkerPayload = { requestId: number; cameras: CameraCard[]; peripherals: PeripheralEntry[] };
type CardsWorkerResponse = { requestId: number; cameraCards: CameraCardItem[]; peripheralItems: PeripheralItem[] };

export type DevicesUiStore = {
  cameraCards: Readable<CameraCardItem[]>;
  peripheralItems: Readable<PeripheralItem[]>;
  cameraStatusCounts: Readable<CameraStatusCounts>;
  update: (cameras: CameraCard[], peripherals: PeripheralEntry[]) => void;
  destroy: () => void;
};

export function createDevicesUiStore(options: { useWorkers?: boolean } = {}): DevicesUiStore {
  const cameraCards = writable<CameraCardItem[]>([]);
  const peripheralItems = writable<PeripheralItem[]>([]);
  const cameraStatusCounts = writable<CameraStatusCounts>({ ...EMPTY_CAMERA_COUNTS });

  const useWorkers = options.useWorkers ?? typeof Worker !== 'undefined';
  let cardsWorker: Worker | null = null;
  let countsWorker: Worker | null = null;
  let cardsRequestId = 0;
  let cardsLastHandled = 0;
  let countsRequestId = 0;
  let countsLastHandled = 0;

  function ensureWorkers() {
    if (!useWorkers) return;
    if (!countsWorker && typeof Worker !== 'undefined') {
      countsWorker = createDeviceCountsWorker();
      countsWorker.onmessage = (event: MessageEvent<CountsWorkerResponse>) => {
        const data = event.data;
        if (data.requestId < countsLastHandled) return;
        countsLastHandled = data.requestId;
        if (data.counts) {
          cameraStatusCounts.set(data.counts);
        }
      };
    }
    if (!cardsWorker && typeof Worker !== 'undefined') {
      cardsWorker = createDeviceCardsWorker();
      cardsWorker.onmessage = (event: MessageEvent<CardsWorkerResponse>) => {
        const data = event.data;
        if (data.requestId < cardsLastHandled) return;
        cardsLastHandled = data.requestId;
        if (Array.isArray(data.cameraCards)) {
          cameraCards.set(data.cameraCards);
        }
        if (Array.isArray(data.peripheralItems)) {
          peripheralItems.set(data.peripheralItems);
        }
      };
    }
  }

  function update(cameras: CameraCard[], peripherals: PeripheralEntry[]): void {
    if (!useWorkers) {
      cameraCards.set(buildCameraCardsList(cameras));
      peripheralItems.set(buildPeripheralItems(peripherals));
      cameraStatusCounts.set(countCameraStatuses(cameras.map((camera) => camera.status)));
      return;
    }

    ensureWorkers();
    if (!countsWorker || !cardsWorker) {
      cameraCards.set(buildCameraCardsList(cameras));
      peripheralItems.set(buildPeripheralItems(peripherals));
      cameraStatusCounts.set(countCameraStatuses(cameras.map((camera) => camera.status)));
      return;
    }

    const statuses = cameras.map((camera) => camera.status);
    const nextCountsId = ++countsRequestId;
    const countsPayload: CountsWorkerPayload = { requestId: nextCountsId, statuses };
    countsWorker.postMessage(countsPayload);

    const nextCardsId = ++cardsRequestId;
    const cardsPayload: CardsWorkerPayload = {
      requestId: nextCardsId,
      cameras: cloneForWorker(cameras),
      peripherals: cloneForWorker(peripherals)
    };
    cardsWorker.postMessage(cardsPayload);
  }

  function destroy(): void {
    if (countsWorker) {
      countsWorker.terminate();
      countsWorker = null;
    }
    if (cardsWorker) {
      cardsWorker.terminate();
      cardsWorker = null;
    }
  }

  return {
    cameraCards,
    peripheralItems,
    cameraStatusCounts,
    update,
    destroy
  };
}

function cloneForWorker<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}
