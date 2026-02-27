import { json, type RequestHandler } from '@sveltejs/kit';
import { fetchPeerInventory } from '$lib/api/peers';

const FALLBACK_MESSAGE = 'Peer inventory unavailable';

export const GET: RequestHandler = async () => {
  try {
    const payload = await fetchPeerInventory();
    return json(payload);
  } catch (error) {
    console.error('Failed to load peer inventory', error);
    return json(
      {
        peers: [],
        discovery: null,
        fetchedAt: Date.now(),
        errorMessage: FALLBACK_MESSAGE,
      },
      { status: 502 },
    );
  }
};
