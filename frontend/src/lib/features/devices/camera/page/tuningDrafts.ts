import type { PipelineDataType, PipelineNodeValue } from '$lib/types/pipeline';
import type { PipelineInputDescriptor } from '$lib/features/devices/camera/types';
import { buildNodeValueFromInput, getDataTypeVariants, resolveDataTypeKey } from '$lib/features/pipelines/valueFormatting';

export type NodeValueDraftDescriptor = {
  nodeId: string;
  portKey: string;
  dataType: PipelineDataType | null;
};

export type PipelineTuningDraftState = {
  get pipelineInputDraftsById(): Record<string, Record<string, string>>;
  set pipelineInputDraftsById(value: Record<string, Record<string, string>>);
  get pipelineNodeDraftsById(): Record<string, Record<string, Record<string, string>>>;
  set pipelineNodeDraftsById(value: Record<string, Record<string, Record<string, string>>>);
  get pipelineInputErrorsById(): Record<string, Record<string, string>>;
  set pipelineInputErrorsById(value: Record<string, Record<string, string>>);
  get pipelineNodeErrorsById(): Record<string, Record<string, Record<string, string>>>;
  set pipelineNodeErrorsById(value: Record<string, Record<string, Record<string, string>>>);
  get pipelineInputOverridesById(): Record<string, Record<string, PipelineNodeValue>>;
  set pipelineInputOverridesById(value: Record<string, Record<string, PipelineNodeValue>>);
  get pipelineNodeOverridesById(): Record<string, Record<string, Record<string, PipelineNodeValue>>>;
  set pipelineNodeOverridesById(value: Record<string, Record<string, Record<string, PipelineNodeValue>>>);
};

export type PipelineTuningDraftDeps = {
  normalizeNodePortKey: (value: string) => string;
  schedulePipelineTuningApply: (pipelineId: string) => void;
};

export function createPipelineTuningDrafts(state: PipelineTuningDraftState, deps: PipelineTuningDraftDeps) {
  function readPipelineInputDraft(pipelineId: string, key: string): string | null {
    const drafts = state.pipelineInputDraftsById?.[pipelineId];
    if (!drafts || typeof drafts !== 'object') return null;
    if (!Object.prototype.hasOwnProperty.call(drafts, key)) return null;
    return drafts[key];
  }

  function readPipelineNodeDraft(pipelineId: string, nodeId: string, portKey: string): string | null {
    const nodeDrafts = state.pipelineNodeDraftsById?.[pipelineId];
    if (!nodeDrafts || typeof nodeDrafts !== 'object') return null;
    const portDrafts = nodeDrafts[nodeId];
    if (!portDrafts || typeof portDrafts !== 'object') return null;
    const normalized = deps.normalizeNodePortKey(portKey);
    if (Object.prototype.hasOwnProperty.call(portDrafts, normalized)) {
      return portDrafts[normalized];
    }
    if (!Object.prototype.hasOwnProperty.call(portDrafts, portKey)) return null;
    return portDrafts[portKey];
  }

  function setPipelineInputDraft(pipelineId: string, key: string, value: string): void {
    const prev = state.pipelineInputDraftsById?.[pipelineId] ?? {};
    state.pipelineInputDraftsById = { ...state.pipelineInputDraftsById, [pipelineId]: { ...prev, [key]: value } };
  }

  function clearPipelineInputDraft(pipelineId: string, key: string): void {
    const prev = state.pipelineInputDraftsById?.[pipelineId];
    if (!prev || typeof prev !== 'object') return;
    const next = { ...prev };
    delete next[key];
    state.pipelineInputDraftsById = { ...state.pipelineInputDraftsById, [pipelineId]: next };
  }

  function setPipelineNodeDraft(pipelineId: string, nodeId: string, portKey: string, value: string): void {
    const prev = state.pipelineNodeDraftsById?.[pipelineId] ?? {};
    const nodeDrafts = prev[nodeId] ?? {};
    state.pipelineNodeDraftsById = {
      ...state.pipelineNodeDraftsById,
      [pipelineId]: { ...prev, [nodeId]: { ...nodeDrafts, [portKey]: value } }
    };
  }

  function clearPipelineNodeDraft(pipelineId: string, nodeId: string, portKey: string): void {
    const prev = state.pipelineNodeDraftsById?.[pipelineId];
    if (!prev || typeof prev !== 'object') return;
    const nodeDrafts = prev[nodeId];
    if (!nodeDrafts || typeof nodeDrafts !== 'object') return;
    const nextNodeDrafts = { ...nodeDrafts };
    delete nextNodeDrafts[portKey];
    state.pipelineNodeDraftsById = {
      ...state.pipelineNodeDraftsById,
      [pipelineId]: { ...prev, [nodeId]: nextNodeDrafts }
    };
  }

  function setPipelineInputError(pipelineId: string, key: string, message: string | null): void {
    const prev = state.pipelineInputErrorsById?.[pipelineId] ?? {};
    const next = { ...prev };
    if (message) {
      next[key] = message;
    } else {
      delete next[key];
    }
    state.pipelineInputErrorsById = { ...state.pipelineInputErrorsById, [pipelineId]: next };
  }

  function setPipelineNodeError(pipelineId: string, nodeId: string, portKey: string, message: string | null): void {
    const prev = state.pipelineNodeErrorsById?.[pipelineId] ?? {};
    const nodeErrors = prev[nodeId] ?? {};
    const nextNodeErrors = { ...nodeErrors };
    if (message) {
      nextNodeErrors[portKey] = message;
    } else {
      delete nextNodeErrors[portKey];
    }
    state.pipelineNodeErrorsById = {
      ...state.pipelineNodeErrorsById,
      [pipelineId]: { ...prev, [nodeId]: nextNodeErrors }
    };
  }

  function setPipelineInputOverride(pipelineId: string, key: string, value: PipelineNodeValue | null): void {
    const prev = state.pipelineInputOverridesById?.[pipelineId] ?? {};
    const next = { ...prev };
    if (value) {
      next[key] = value;
    } else {
      delete next[key];
    }
    state.pipelineInputOverridesById = { ...state.pipelineInputOverridesById, [pipelineId]: next };
  }

  function setPipelineNodeOverride(pipelineId: string, nodeId: string, portKey: string, value: PipelineNodeValue | null): void {
    const prev = state.pipelineNodeOverridesById?.[pipelineId] ?? {};
    const nodeOverrides = prev[nodeId] ?? {};
    const nextNodeOverrides = { ...nodeOverrides };
    if (value) {
      nextNodeOverrides[portKey] = value;
    } else {
      delete nextNodeOverrides[portKey];
    }
    state.pipelineNodeOverridesById = {
      ...state.pipelineNodeOverridesById,
      [pipelineId]: { ...prev, [nodeId]: nextNodeOverrides }
    };
  }

  function updatePipelineInputDraft(pipelineId: string, entry: PipelineInputDescriptor, raw: string): void {
    const key = entry.canonicalKey;
    setPipelineInputDraft(pipelineId, key, raw);
    if (!raw.trim()) {
      setPipelineInputOverride(pipelineId, key, null);
      clearPipelineInputDraft(pipelineId, key);
      setPipelineInputError(pipelineId, key, null);
      deps.schedulePipelineTuningApply(pipelineId);
      return;
    }
    const typeKey = resolveDataTypeKey(entry.dataType ?? undefined) ?? 'string';
    const variants = getDataTypeVariants(entry.dataType ?? undefined);
    const parsed = buildNodeValueFromInput(raw, typeKey, variants);
    if (parsed.success) {
      setPipelineInputOverride(pipelineId, key, { ...parsed.value, dataType: entry.dataType ?? parsed.value.dataType });
      setPipelineInputError(pipelineId, key, null);
      deps.schedulePipelineTuningApply(pipelineId);
    } else {
      setPipelineInputError(pipelineId, key, parsed.error);
    }
  }

  function updatePipelineNodeDraft(pipelineId: string, descriptor: NodeValueDraftDescriptor, raw: string): void {
    updatePipelineNodeValue(pipelineId, descriptor.nodeId, descriptor.portKey, descriptor.dataType, raw);
  }

  function updatePipelineNodeValue(
    pipelineId: string,
    nodeId: string,
    portKey: string,
    dataType: PipelineDataType | null,
    raw: string
  ): void {
    const normalizedPort = deps.normalizeNodePortKey(portKey);
    setPipelineNodeDraft(pipelineId, nodeId, normalizedPort, raw);
    if (!raw.trim()) {
      setPipelineNodeOverride(pipelineId, nodeId, normalizedPort, null);
      clearPipelineNodeDraft(pipelineId, nodeId, normalizedPort);
      setPipelineNodeError(pipelineId, nodeId, normalizedPort, null);
      deps.schedulePipelineTuningApply(pipelineId);
      return;
    }
    const typeKey = resolveDataTypeKey(dataType ?? undefined) ?? 'string';
    const variants = getDataTypeVariants(dataType ?? undefined);
    const parsed = buildNodeValueFromInput(raw, typeKey, variants);
    if (parsed.success) {
      setPipelineNodeOverride(pipelineId, nodeId, normalizedPort, {
        ...parsed.value,
        dataType: dataType ?? parsed.value.dataType
      });
      setPipelineNodeError(pipelineId, nodeId, normalizedPort, null);
      deps.schedulePipelineTuningApply(pipelineId);
    } else {
      setPipelineNodeError(pipelineId, nodeId, normalizedPort, parsed.error);
    }
  }

  return {
    readPipelineInputDraft,
    readPipelineNodeDraft,
    setPipelineInputDraft,
    clearPipelineInputDraft,
    setPipelineNodeDraft,
    clearPipelineNodeDraft,
    setPipelineInputError,
    setPipelineNodeError,
    setPipelineInputOverride,
    setPipelineNodeOverride,
    updatePipelineInputDraft,
    updatePipelineNodeDraft,
    updatePipelineNodeValue
  };
}
