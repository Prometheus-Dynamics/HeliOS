import type { RequestHandler } from '@sveltejs/kit';
import { fetchDashboardPageData } from '$lib/api/dashboardPage';

const JSON_HEADERS = { 'content-type': 'application/json' };

export const GET: RequestHandler = async ({ url }) => {
  try {
    const payload = await fetchDashboardPageData({ baseUrl: url.origin });
    return new Response(JSON.stringify(payload), {
      headers: JSON_HEADERS
    });
  } catch (error) {
    console.error('Failed to load dashboard payload', error);
    return new Response(JSON.stringify({ message: 'Dashboard data unavailable' }), {
      status: 502,
      headers: JSON_HEADERS
    });
  }
};
