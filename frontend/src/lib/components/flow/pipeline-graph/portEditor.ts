import type { PipelineGraphPlan } from '$lib/types/pipeline';
import {
  buildNodeValueFromInput,
  formatPipelineValue,
  isDataTypeSettable,
  resolveDataTypeKey,
  getDataTypeVariants
} from '$lib/features/pipelines/valueFormatting';
import type { PortEditorState, NodePortEditorState, PipelineInputEditorState } from './types';
import { getPipelineInputDataType, normalizePipelinePortName } from '$lib/features/pipelines/boundary';

export type PortEditorBuildResult =
  | {
      state: PortEditorState;
      draft: string;
      error: string | null;
    }
  | null;

export const buildNodePortEditorState = (
  plan: PipelineGraphPlan,
  nodeId: string,
  port: string,
  anchor: { x: number; y: number }
): PortEditorBuildResult => {
  const node = plan.nodes[nodeId];
  if (!node) return null;
  const dataType = node.inputs?.[port];
  const existingValue = node.info?.values?.[port] ?? null;
  const variants = getDataTypeVariants(dataType);
  const dataTypeKey = resolveDataTypeKey(dataType);
  const state: NodePortEditorState = {
    mode: 'node',
    nodeId,
    port,
    dataType,
    dataTypeKey,
    existingValue,
    settable: isDataTypeSettable(dataType),
    anchor,
    variants
  };
  const draft = (() => {
    if (dataTypeKey?.toLowerCase() === 'enum') {
      if (existingValue && typeof existingValue.value === 'string') {
        return existingValue.value;
      }
      return variants[0] ?? '';
    }
    const formatted = formatPipelineValue(existingValue);
    if (formatted) return formatted;
    return variants[0] ?? '';
  })();
  return {
    state,
    draft,
    error: null
  };
};

export const buildPipelineInputEditorState = (
  plan: PipelineGraphPlan,
  port: string,
  anchor: { x: number; y: number }
): PortEditorBuildResult => {
  const key = normalizePipelinePortName(port);
  const dataType = getPipelineInputDataType(plan, key);
  const existingValue = plan.pipelineInputValues?.[key] ?? null;
  const variants = getDataTypeVariants(dataType);
  const dataTypeKey = resolveDataTypeKey(dataType);
  const state: PipelineInputEditorState = {
    mode: 'pipeline-input',
    port,
    dataType,
    dataTypeKey,
    existingValue,
    settable: isDataTypeSettable(dataType),
    anchor,
    variants
  };
  const draft = (() => {
    if (dataTypeKey?.toLowerCase() === 'enum') {
      if (existingValue && typeof existingValue.value === 'string') {
        return existingValue.value;
      }
      return variants[0] ?? '';
    }
    const formatted = formatPipelineValue(existingValue);
    if (formatted) return formatted;
    return variants[0] ?? '';
  })();
  return {
    state,
    draft,
    error: null
  };
};

type PortEditorApplyResult =
  | { success: true; plan: PipelineGraphPlan; error?: string }
  | { success: false; error: string };

export const applyPortEditorDraft = (
  plan: PipelineGraphPlan,
  state: PortEditorState,
  draft: string
): PortEditorApplyResult => {
  if (!state.settable) {
    return { success: false, error: 'This port is not settable.' };
  }
  if (!state.dataTypeKey) {
    return { success: false, error: 'Unknown data type for this port.' };
  }
  const parsed = buildNodeValueFromInput(draft, state.dataTypeKey, state.variants);
  if (!parsed.success) {
    return { success: false, error: parsed.error };
  }
  if (state.mode === 'node') {
    const node = plan.nodes[state.nodeId];
    if (!node) {
      return { success: false, error: 'Node unavailable.' };
    }
    const existingValues = node.info?.values ?? {};
    const values = { ...existingValues, [state.port]: parsed.value };
    const updatedNode = {
      ...node,
      info: {
        ...node.info,
        values
      }
    };
    return {
      success: true,
      plan: {
        ...plan,
        nodes: {
          ...plan.nodes,
          [state.nodeId]: updatedNode
        }
      }
    };
  }
  const key = normalizePipelinePortName(state.port);
  const existing = plan.pipelineInputValues ?? {};
  const values = { ...existing, [key]: parsed.value };
  return {
    success: true,
    plan: {
      ...plan,
      pipelineInputValues: values
    }
  };
};

export const clearPortEditorValue = (
  plan: PipelineGraphPlan,
  state: PortEditorState
): PortEditorApplyResult => {
  if (state.mode === 'node') {
    const node = plan.nodes[state.nodeId];
    if (!node) {
      return { success: false, error: 'Node unavailable.' };
    }
    const existingValues = { ...(node.info?.values ?? {}) };
    if (existingValues[state.port] === undefined) {
      return { success: true, plan };
    }
    delete existingValues[state.port];
    const sanitized = Object.keys(existingValues).length > 0 ? existingValues : undefined;
    const updatedNode = {
      ...node,
      info: {
        ...node.info,
        values: sanitized
      }
    };
    return {
      success: true,
      plan: {
        ...plan,
        nodes: {
          ...plan.nodes,
          [state.nodeId]: updatedNode
        }
      }
    };
  }
  const key = normalizePipelinePortName(state.port);
  const existing = { ...(plan.pipelineInputValues ?? {}) };
  if (existing[key] === undefined) {
    return { success: true, plan };
  }
  delete existing[key];
  const sanitized = Object.keys(existing).length > 0 ? existing : {};
  return {
    success: true,
    plan: {
      ...plan,
      pipelineInputValues: sanitized
    }
  };
};
