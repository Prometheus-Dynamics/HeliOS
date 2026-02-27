import type {
  PipelineDataType,
  PipelineGraphPlan,
  PipelineNodeSyncConfig,
  PipelineNodeValue
} from '$lib/types/pipeline';
import type { PipelineOutputEntry, PipelinePortEntry } from '../../../components/pipelines/types';
import { collectPipelineInputs, collectPipelineOutputs } from '../boundary';

type CloneDataTypeFn = (value: PipelineDataType) => PipelineDataType;
type CloneNodeValueFn = (value: PipelineNodeValue) => PipelineNodeValue;
type CloneNodeSyncConfigFn = (value: PipelineNodeSyncConfig | null | undefined) => PipelineNodeSyncConfig | null;

type BuildInputOptions = {
  cloneDataType: CloneDataTypeFn;
  cloneNodeValue: CloneNodeValueFn;
  cloneNodeSyncConfig: CloneNodeSyncConfigFn;
};

type BuildOutputOptions = {
  cloneDataType: CloneDataTypeFn;
};

export function buildPipelineInputEntries(plan: PipelineGraphPlan, options: BuildInputOptions): PipelinePortEntry[] {
  const { cloneDataType, cloneNodeValue, cloneNodeSyncConfig } = options;
  const values = plan.pipelineInputValues ?? {};
  const configs = plan.pipelineInputConfigs ?? {};
  return collectPipelineInputs(plan)
    .map(({ nodeId, name, dataType, node }) => ({
      nodeId,
      name,
      dataType: cloneDataType(dataType),
      value: values[name] ? cloneNodeValue(values[name]) : null,
      queueConfig: configs[name] ? { ...configs[name] } : undefined,
      syncConfig: node?.sync ? cloneNodeSyncConfig(node.sync) ?? null : null
    }))
    .sort((a, b) => a.name.localeCompare(b.name));
}

export function buildPipelineOutputEntries(
  plan: PipelineGraphPlan,
  options: BuildOutputOptions
): PipelineOutputEntry[] {
  const { cloneDataType } = options;
  return collectPipelineOutputs(plan)
    .map(({ name, dataType, nodeId }) => ({
      nodeId,
      name,
      dataType: cloneDataType(dataType),
      sinkConfig: plan.pipelineOutputConfigs?.[name] ?? undefined
    }))
    .sort((a, b) => a.name.localeCompare(b.name));
}
