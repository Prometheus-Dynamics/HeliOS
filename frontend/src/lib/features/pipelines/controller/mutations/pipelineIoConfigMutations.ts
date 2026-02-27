import { get } from 'svelte/store';
import type {
  PipelineInputQueueConfig,
  PipelineNodeValue,
  PipelineOutputSinkConfig
} from '$lib/types/pipeline';
import { normalizePipelinePortName } from '../../boundary';
import type { PipelineMutationsDeps } from './types';
import { normalizeInputConfig, normalizeOutputConfig } from './pipelineIoMutationsUtils';

export const createPipelineIoConfigMutations = (deps: PipelineMutationsDeps) => {
  const {
    selectedPipeline,
    updateCurrentPlan,
    cloneNodeValue
  } = deps;

  function setPipelineInputValue(name: string, value: PipelineNodeValue | null) {
    const pipeline = get(selectedPipeline);
    if (!pipeline) return;
    if (pipeline.graph?.format === 'daedalus' || pipeline.graph?.daedalus) return;
    const portName = normalizePipelinePortName(name);
    if (!portName) return;
    updateCurrentPlan((plan) => {
      const existing = plan.pipelineInputValues ?? {};
      const values = { ...existing };
      if (value) {
        values[portName] = cloneNodeValue(value);
      } else {
        delete values[portName];
      }
      plan.pipelineInputValues = values;
    });
  }

  function setPipelinePortConfig(
    direction: 'input' | 'output',
    name: string,
    config: PipelineInputQueueConfig | PipelineOutputSinkConfig
  ) {
    const pipeline = get(selectedPipeline);
    if (!pipeline) return;
    if (pipeline.graph?.format === 'daedalus' || pipeline.graph?.daedalus) return;
    const portName = normalizePipelinePortName(name);
    if (!portName) return;
    updateCurrentPlan((plan) => {
      if (direction === 'input') {
        const existing = { ...(plan.pipelineInputConfigs ?? {}) };
        existing[portName] = normalizeInputConfig(config as PipelineInputQueueConfig);
        plan.pipelineInputConfigs = existing;
      } else {
        const existing = { ...(plan.pipelineOutputConfigs ?? {}) };
        existing[portName] = normalizeOutputConfig(config as PipelineOutputSinkConfig);
        plan.pipelineOutputConfigs = existing;
      }
    });
  }

  return {
    setPipelineInputValue,
    setPipelinePortConfig
  };
};
