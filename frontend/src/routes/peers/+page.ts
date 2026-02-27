import type { PageLoad } from './$types';
import type { PeerInventoryPayload } from '$lib/types/peer';

function emptyPayload(errorMessage?: string): PeerInventoryPayload {
  return {
    peers: [],
    discovery: null,
    fetchedAt: Date.now(),
    errorMessage,
  };
}

export const load: PageLoad = () => {
  return {
    payload: emptyPayload()
  };
};
