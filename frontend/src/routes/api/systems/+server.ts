import type { RequestHandler } from '@sveltejs/kit';

import { fetchSystemsPageData } from '$lib/api/systemsPage';

const JSON_HEADERS = { 'content-type': 'application/json' };

export const GET: RequestHandler = async () => {
  try {
    const payload = await fetchSystemsPageData();
    return new Response(JSON.stringify(payload), { headers: JSON_HEADERS });
  } catch (error) {
    console.error('Failed to load systems payload', error);
    return new Response(JSON.stringify({ message: 'Systems data unavailable' }), {
      status: 502,
      headers: JSON_HEADERS
    });
  }
};
