import { DEFAULT_REQUEST_TIMEOUT_MS } from '$lib/api/requestUtils';
import type { PeripheralEntry } from '$lib/types/devices';

export const PERIPHERAL_ROW_LIMIT = 8;
export const REQUEST_TIMEOUT_MS = DEFAULT_REQUEST_TIMEOUT_MS;
export const I2C_TIMEOUT_MS = 2_000;

export const CORAL_ICON = Object.freeze({
  url: '/logos/coral_logo.png',
  label: 'Coral Edge TPU'
}) satisfies PeripheralEntry['icon'];
