import { writable } from 'svelte/store';

export type BackendFeatures = {
  shadowRecorder: boolean;
};

const store = writable<BackendFeatures>({ shadowRecorder: false });

export const backendFeatures = {
  subscribe: store.subscribe
};

export function updateBackendFeaturesFromHealthPayload(payload: unknown): void {
  const obj = payload as any;
  const features = obj?.features ?? null;
  const shadowRecorder = Boolean(features?.shadow_recorder);
  store.set({ shadowRecorder });
}

