import type { PipelineDataType, PipelineGraphPlan } from '$lib/types/pipeline';
import type { PipelineBoundaryInfo } from './boundaryUtils';
import {
  buildBoundaryNode,
  collectBoundaryPortsFromMetadata,
  normalizePipelinePortName
} from './boundaryUtils';

export const createPipelineInputNode = (
  name: string,
  dataType: PipelineDataType,
  options?: { location?: { x: number; y: number }; nodeId?: string }
) => buildBoundaryNode(name, dataType, 'input', options);

export const collectPipelineInputs = (plan: PipelineGraphPlan): PipelineBoundaryInfo[] =>
  collectBoundaryPortsFromMetadata(plan, 'input');

export const findPipelineInputNode = (
  plan: PipelineGraphPlan,
  name: string
): PipelineBoundaryInfo | undefined => {
  const normalized = normalizePipelinePortName(name);
  return collectPipelineInputs(plan).find((entry) => entry.name === normalized);
};

export const getPipelineInputDataType = (
  plan: PipelineGraphPlan,
  port: string
): PipelineDataType | undefined => findPipelineInputNode(plan, port)?.dataType;
