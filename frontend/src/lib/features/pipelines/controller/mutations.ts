import type { PipelineEndpoint, PipelineGraphPlan } from '$lib/types/pipeline';
import { dataTypeForConnection, inferPortDataType } from './mutations/dataTypeHelpers';
import { createGraphMutations } from './mutations/graphMutations';
import { createNodeMutations } from './mutations/nodeMutations';
import { createPipelineIoMutations } from './mutations/pipelineIoMutations';
import { createRegistryMutations } from './mutations/registryMutations';
import type { PipelineMutationsDeps } from './mutations/types';

export const createPipelineMutations = (deps: PipelineMutationsDeps) => {
  const graphMutations = createGraphMutations(deps);
  const registryMutations = createRegistryMutations(deps);
  const pipelineIoMutations = createPipelineIoMutations(deps);
  const nodeMutations = createNodeMutations(deps);

  return {
    dataTypeForConnection: (plan: PipelineGraphPlan, endpoint: PipelineEndpoint, direction: 'in' | 'out') =>
      dataTypeForConnection(plan, endpoint, direction),
    inferPortDataType: (plan: PipelineGraphPlan | null | undefined, nodeId: string, port: string, direction: 'input' | 'output') =>
      inferPortDataType({
        plan,
        nodeId,
        port,
        direction,
        cloneDataType: deps.cloneDataType,
        resolveDataTypeKey: deps.resolveDataTypeKey
      }),
    ...registryMutations,
    ...graphMutations,
    ...pipelineIoMutations,
    ...nodeMutations
  };
};
