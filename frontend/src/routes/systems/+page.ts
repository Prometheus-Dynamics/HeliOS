import type { PageLoad } from './$types';
import { DEFAULT_ROBOT_DIMENSIONS } from '$lib/3d/rigDefaults';
import { emptyImuStatus, emptySystemsRuntime, fetchSystemsPageData } from '$lib/api/systemsPage';
import type { SystemsPageData } from '$lib/types/systems';

type SystemsPageLoadResult = {
  payload: SystemsPageData;
};

const EMPTY_PAYLOAD: SystemsPageData = {
  summary: [],
  device: null,
  interfaces: [],
  sessions: [],
  rig: {
    robot: { ...DEFAULT_ROBOT_DIMENSIONS },
    cameras: []
  },
  logs: [],
  runtime: emptySystemsRuntime(),
  i2cInventory: { buses: [], devices: [] },
  imu: emptyImuStatus(),
  fetchedAt: 0,
  errorMessage: 'Systems data unavailable',
  errors: {
    logs: null,
    runtime: null,
    i2c: null,
    imu: null
  }
};

export const ssr = false;
export const prerender = false;

export const load: PageLoad<SystemsPageLoadResult> = async () => {
  try {
    return { payload: clonePayload(await fetchSystemsPageData()) };
  } catch {
    return { payload: clonePayload(EMPTY_PAYLOAD) };
  }
};

function clonePayload(value: SystemsPageData): SystemsPageData {
  return JSON.parse(JSON.stringify(value));
}
