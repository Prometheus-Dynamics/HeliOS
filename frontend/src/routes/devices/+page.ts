import type { PageLoad } from './$types';
import type { DevicesPayload } from '$lib/api/devicesPage';

const EMPTY_DEVICES: DevicesPayload = {
  summary: [],
  cameras: [],
  peripherals: [],
  tasks: [],
  errors: { peripherals: null }
};

type DevicesPageLoadData = {
  initial: DevicesPayload;
  receivedAt: number;
};

export const load: PageLoad<DevicesPageLoadData> = () => {
  return {
    initial: clonePayload(EMPTY_DEVICES),
    receivedAt: Date.now()
  };
};

function clonePayload<T>(payload: T): T {
  return JSON.parse(JSON.stringify(payload));
}
