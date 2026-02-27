import type { PipelineMutationsDeps } from './types';
import { createPipelineIoPortMutations } from './pipelineIoPortMutations';
import { createPipelineIoConfigMutations } from './pipelineIoConfigMutations';

export const createPipelineIoMutations = (deps: PipelineMutationsDeps) => {
  return {
    ...createPipelineIoPortMutations(deps),
    ...createPipelineIoConfigMutations(deps)
  };
};
