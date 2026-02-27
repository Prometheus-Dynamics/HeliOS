import type { PipelineInputQueueConfig, PipelineOutputSinkConfig } from '$lib/types/pipeline';

const sanitizeCapacity = (value: number | undefined): number => {
  if (typeof value !== 'number' || !Number.isFinite(value)) {
    return 1;
  }
  return Math.max(1, Math.trunc(value));
};

export const normalizeInputConfig = (config: PipelineInputQueueConfig): PipelineInputQueueConfig => ({
  policy: config?.policy ?? 'NewestWins',
  capacity: sanitizeCapacity(config?.capacity)
});

export const normalizeOutputConfig = (config: PipelineOutputSinkConfig): PipelineOutputSinkConfig => ({
  capacity: sanitizeCapacity(config?.capacity)
});
