import type { RequestHandler } from '@sveltejs/kit';

import { fetchDevicesPageData } from '$lib/api/devicesPage';

const JSON_HEADERS = { 'content-type': 'application/json' };

export const GET: RequestHandler = async () => {
  try {
    const payload = await fetchDevicesPageData();
    return new Response(JSON.stringify(payload), { headers: JSON_HEADERS });
  } catch (error) {
    console.error('Failed to load devices payload', error);
    return new Response(JSON.stringify({ message: 'Devices data unavailable' }), {
      status: 502,
      headers: JSON_HEADERS
    });
  }
};
