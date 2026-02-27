import type { PageLoad } from './$types';
import { DEFAULT_ROBOT_DIMENSIONS } from '$lib/3d/rig';
import { emptyImuStatus } from '$lib/api/systemsPage';
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
  i2cInventory: { buses: [], devices: [] },
  imu: emptyImuStatus(),
  fetchedAt: 0,
  errorMessage: 'Systems data unavailable',
  errors: {
    logs: null,
    i2c: null,
    imu: null
  }
};

export const ssr = false;
export const prerender = false;

export const load: PageLoad<SystemsPageLoadResult> = async () => {
  return { payload: clonePayload(EMPTY_PAYLOAD) };
};

function clonePayload(value: SystemsPageData): SystemsPageData {
  return JSON.parse(JSON.stringify(value));
}
