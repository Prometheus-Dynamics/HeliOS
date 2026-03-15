import { writable } from 'svelte/store';

export type BackendFeatures = {
  shadowRecorder: boolean;
  pipelineRegistryStartupWarm: boolean;
  pipelineRegistryPrefetch: boolean;
};

const store = writable<BackendFeatures>({
  shadowRecorder: false,
  pipelineRegistryStartupWarm: false,
  pipelineRegistryPrefetch: false
});

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
  const pipelineRegistryStartupWarm = Boolean(features?.pipeline_registry_startup_warm);
  const pipelineRegistryPrefetch = Boolean(features?.pipeline_registry_prefetch);
  store.set({ shadowRecorder, pipelineRegistryStartupWarm, pipelineRegistryPrefetch });
}
