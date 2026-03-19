import type { RequestHandler } from '@sveltejs/kit';
import { OpenAPI } from '$lib/ts-bindings/http/client';
import { fetchDashboardPageData } from '$lib/api/dashboardPage';

const JSON_HEADERS = { 'content-type': 'application/json' };

export const GET: RequestHandler = async ({ url }) => {
  try {
    OpenAPI.BASE = url.origin;
    const payload = await fetchDashboardPageData();
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
