import { DEFAULT_REQUEST_TIMEOUT_MS } from '$lib/api/requestUtils';

export const REQUEST_TIMEOUT_MS = DEFAULT_REQUEST_TIMEOUT_MS;
export const SENSOR_REQUEST_TIMEOUT_MS = 12_000;
export const SYSTEMS_RETRY_OPTIONS = {
  maxAttempts: 2,
  baseDelayMs: 1000,
  maxDelayMs: 4000
};

export const FAILURE_MESSAGE_ALL = 'Systems data unavailable';
