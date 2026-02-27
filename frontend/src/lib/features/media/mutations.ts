import type { MediaAssetType } from './mediaKind';
import { invalidateSWRPrefix } from '$lib/utils/swrCache';

export type MediaMutationKind = 'created' | 'updated' | 'deleted';

export type MediaMutationEvent = {
  mutation: MediaMutationKind;
  mediaKind?: MediaAssetType | 'unknown';
  cameraSource?: string | null;
  mediaId?: string;
  atMs: number;
};

type MediaMutationListener = (event: MediaMutationEvent) => void;

const listeners = new Set<MediaMutationListener>();

export function emitMediaMutation(event: Omit<MediaMutationEvent, 'atMs'> & { atMs?: number }): void {
  invalidateSWRPrefix('media:');
  const payload: MediaMutationEvent = {
    atMs: Number.isFinite(event.atMs) ? Math.trunc(Number(event.atMs)) : Date.now(),
    mutation: event.mutation,
    mediaKind: event.mediaKind,
    cameraSource: event.cameraSource,
    mediaId: event.mediaId
  };

  for (const listener of Array.from(listeners)) {
    try {
      listener(payload);
    } catch (error) {
      console.warn('media mutation listener failed', error);
    }
  }
}

export function subscribeMediaMutations(listener: MediaMutationListener): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}
