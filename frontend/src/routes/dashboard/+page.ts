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

export const load: PageLoad<DashboardPageData> = () => {
  return {
    payload: cloneDashboardPayload(EMPTY_DASHBOARD)
  };
};

function cloneDashboardPayload(payload: DashboardPayload): DashboardPayload {
  return JSON.parse(JSON.stringify(payload));
}
