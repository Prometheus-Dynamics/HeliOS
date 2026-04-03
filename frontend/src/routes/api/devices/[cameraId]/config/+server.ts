import type { RequestHandler } from '@sveltejs/kit';
import { fetchCameraConfigDetail } from '$lib/api/cameraConfigPage';
import { OpenAPI } from '$lib/api/client';

const JSON_HEADERS = { 'content-type': 'application/json' };

export const GET: RequestHandler = async ({ params, url }) => {
  const param = params.cameraId ?? '';
  const cameraId = decodeURIComponent(param);
  if (!cameraId) {
    return new Response(JSON.stringify({ message: 'Camera identifier missing' }), {
      status: 400,
      headers: JSON_HEADERS
    });
  }
  try {
    OpenAPI.BASE = url.origin;
    const payload = await fetchCameraConfigDetail(cameraId);
    if (!payload.drivers.length) {
      return new Response(JSON.stringify({ message: 'Camera configuration not found' }), {
        status: 404,
        headers: JSON_HEADERS
      });
    }
    return new Response(JSON.stringify(payload), { headers: JSON_HEADERS });
  } catch (error) {
    console.error('Failed to load camera configuration payload', error);
    return new Response(JSON.stringify({ message: 'Camera configuration unavailable' }), {
      status: 502,
      headers: JSON_HEADERS
    });
  }
};
