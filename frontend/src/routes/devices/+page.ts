import { fetchDevicesPageData } from '$lib/api/devices/fetchers';
import type { PageLoad } from './$types';
import type { DevicesPayload } from '$lib/api/devices/types';

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

export const ssr = false;
export const prerender = false;

export const load: PageLoad<DevicesPageLoadData> = async () => {
  try {
    const initial = await fetchDevicesPageData();
    return {
      initial: clonePayload(initial),
      receivedAt: Date.now()
    };
  } catch {
    return {
      initial: clonePayload(EMPTY_DEVICES),
      receivedAt: Date.now()
    };
  }
};

function clonePayload<T>(payload: T): T {
  return JSON.parse(JSON.stringify(payload));
}
