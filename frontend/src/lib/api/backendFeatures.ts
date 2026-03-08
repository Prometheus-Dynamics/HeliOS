import { writable } from 'svelte/store';

export type BackendFeatures = {
  shadowRecorder: boolean;
};

const store = writable<BackendFeatures>({ shadowRecorder: false });

export const backendFeatures = {
  subscribe: store.subscribe
};

export function updateBackendFeaturesFromHealthPayload(payload: unknown): void {
  const record = payload && typeof payload === 'object' ? (payload as Record<string, unknown>) : null;
  const features =
    record?.features && typeof record.features === 'object'
      ? (record.features as Record<string, unknown>)
      : null;
  const shadowRecorder = Boolean(features?.shadow_recorder);
  store.set({ shadowRecorder });
}
