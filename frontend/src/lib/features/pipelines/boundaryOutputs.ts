import type { PipelineDataType, PipelineGraphPlan } from '$lib/types/pipeline';
import type { PipelineBoundaryInfo } from './boundaryUtils';
import {
  buildBoundaryNode,
  collectBoundaryPortsFromMetadata,
  normalizePipelinePortName
} from './boundaryUtils';

export const createPipelineOutputNode = (
  name: string,
  dataType: PipelineDataType,
  options?: { location?: { x: number; y: number }; nodeId?: string }
) => buildBoundaryNode(name, dataType, 'output', options);

export const collectPipelineOutputs = (plan: PipelineGraphPlan): PipelineBoundaryInfo[] =>
  collectBoundaryPortsFromMetadata(plan, 'output');

export const findPipelineOutputNode = (
  plan: PipelineGraphPlan,
  name: string
): PipelineBoundaryInfo | undefined => {
  const normalized = normalizePipelinePortName(name);
  return collectPipelineOutputs(plan).find((entry) => entry.name === normalized);
};
