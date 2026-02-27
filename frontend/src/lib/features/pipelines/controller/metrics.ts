import { writable, type Writable } from 'svelte/store';
import type { PipelineMetricsState } from './pipelineMetrics';

export const createPipelineMetricsState = () =>
  writable<Record<string, PipelineMetricsState>>({});

export const removePipelineMetricsState = (
  state: Writable<Record<string, PipelineMetricsState>>,
  pipelineId: string
) => {
  state.update((current) => {
    const copy = { ...current };
    delete copy[pipelineId];
    return copy;
  });
};
