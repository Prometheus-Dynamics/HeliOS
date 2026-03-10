import { fetchDashboardPageData } from '$lib/api/dashboardPage';
import type { DashboardPayload } from '$lib/types/dashboard';
import type { PageLoad } from './$types';

const EMPTY_DASHBOARD: DashboardPayload = {
  meta: {
    fetchedAt: null,
    failures: 0,
    sources: {
      streams: 'unknown',
      cameras: 'unknown',
      pipelines: 'unknown',
      metrics: 'unknown'
    }
  },
  summaryStats: [],
  timelineItems: [],
  pipelineWatch: [],
  streamGallery: []
};

type DashboardPageData = {
  payload: DashboardPayload;
};

export const ssr = false;
export const prerender = false;

export const load: PageLoad<DashboardPageData> = async () => {
  try {
    return {
      payload: cloneDashboardPayload(await fetchDashboardPageData())
    };
  } catch {
    return {
      payload: cloneDashboardPayload(EMPTY_DASHBOARD)
    };
  }
};

function cloneDashboardPayload(payload: DashboardPayload): DashboardPayload {
  return JSON.parse(JSON.stringify(payload));
}
