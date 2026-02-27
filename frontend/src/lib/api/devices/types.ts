import type { DevicesPageData, PeripheralEntry } from '$lib/types/devices';

export type DevicesPayload = DevicesPageData;

export type DevicesPeripheralsSnapshot = {
  peripherals: PeripheralEntry[];
  error: string | null;
};
