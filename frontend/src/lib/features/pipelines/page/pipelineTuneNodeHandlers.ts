import type { PipelineDataType, PipelineGraphPlan, PipelineNodeValue } from '$lib/types/pipeline';

export type TuneNodeHandlerDeps = {
  getTunePlan: () => PipelineGraphPlan | null;
  isDaedalusPlan: (plan: PipelineGraphPlan) => boolean;
  safeClonePlan: (plan: PipelineGraphPlan) => PipelineGraphPlan;
  handlePlanChange: (plan: PipelineGraphPlan) => void;
  pipelineUpdates: { sendPipeline: (payload: unknown) => boolean | void };
  setNodeConstantValue: (nodeId: string, portKey: string, value: PipelineNodeValue) => void;
  scheduleTuneGlobalAutoSave: () => void;
  normalizePortKey: (key: string) => string;
  resolveDataTypeKey: (dataType: PipelineDataType | undefined) => string | null;
  getDataTypeVariants: (dataType: PipelineDataType | undefined) => string[];
  buildNodeValueFromInput: (
    raw: string,
    typeKey: string,
    variants?: string[]
  ) =>
    | { success: true; value: PipelineNodeValue; error?: string }
    | { success: false; error: string };
  setTuneNodeDraft: (nodeId: string, portKey: string, raw: string) => void;
  setTuneNodeError: (nodeId: string, portKey: string, message: string | null) => void;
  setTuneStreamNodeDraft: (streamId: string, nodeId: string, portKey: string, raw: string) => void;
  clearTuneStreamNodeDraft: (streamId: string, nodeId: string, portKey: string) => void;
  setTuneStreamNodeError: (streamId: string, nodeId: string, portKey: string, message: string | null) => void;
  getTuneStreamNodeOverridesById: () => Record<string, Record<string, Record<string, PipelineNodeValue>>>;
  setTuneStreamNodeOverridesById: (next: Record<string, Record<string, Record<string, PipelineNodeValue>>>) => void;
  scheduleTuneStreamAutoApply: (streamId: string) => void;
};

export const createTuneNodeHandlers = (deps: TuneNodeHandlerDeps) => {
  const updateGlobalNodeValue = (nodeId: string, portKey: string, dataType: PipelineDataType | null, raw: string): void => {
    const normalizedPort = deps.normalizePortKey(portKey);
    deps.setTuneNodeDraft(nodeId, normalizedPort, raw);
    if (!raw.trim()) {
      deps.setTuneNodeError(nodeId, normalizedPort, null);
      return;
    }
    const typeKey = deps.resolveDataTypeKey(dataType ?? undefined) ?? 'string';
    const variants = deps.getDataTypeVariants(dataType ?? undefined);
    const parsed = deps.buildNodeValueFromInput(raw, typeKey, variants);
    if (!parsed.success) {
      deps.setTuneNodeError(nodeId, normalizedPort, parsed.error ?? 'Invalid value');
      return;
    }
    deps.setTuneNodeError(nodeId, normalizedPort, null);
    const tunePlan = deps.getTunePlan();
    if (tunePlan && deps.isDaedalusPlan(tunePlan)) {
      const next = deps.safeClonePlan(tunePlan);
      const node = next.nodes?.[nodeId];
      if (node) {
        const existing = node.info?.values ?? {};
        const existingKey =
          Object.keys(existing).find((key) => deps.normalizePortKey(key) === normalizedPort) ?? portKey;
        node.info = { ...node.info, values: { ...existing, [existingKey]: parsed.value } };
        deps.handlePlanChange(next);
      }
      const sent = deps.pipelineUpdates.sendPipeline({
        type: 'set_node_const',
        node_id: nodeId,
        port: portKey,
        value: parsed.value.value
      });
      if (!sent) {
        deps.scheduleTuneGlobalAutoSave();
      }
      return;
    }
    deps.setNodeConstantValue(nodeId, portKey, parsed.value);
    const sent = deps.pipelineUpdates.sendPipeline({
      type: 'set_node_const',
      node_id: nodeId,
      port: portKey,
      value: parsed.value.value
    });
    if (!sent) {
      deps.scheduleTuneGlobalAutoSave();
    }
  };

  const updateStreamNodeValue = (
    streamId: string,
    nodeId: string,
    portKey: string,
    dataType: PipelineDataType | null,
    raw: string
  ): void => {
    const normalizedPort = deps.normalizePortKey(portKey);
    deps.setTuneStreamNodeDraft(streamId, nodeId, normalizedPort, raw);
    if (!raw.trim()) {
      const prev = deps.getTuneStreamNodeOverridesById()[streamId] ?? {};
      const nodeOverrides = { ...(prev[nodeId] ?? {}) };
      delete nodeOverrides[normalizedPort];
      deps.setTuneStreamNodeOverridesById({
        ...deps.getTuneStreamNodeOverridesById(),
        [streamId]: { ...prev, [nodeId]: nodeOverrides }
      });
      deps.clearTuneStreamNodeDraft(streamId, nodeId, normalizedPort);
      deps.setTuneStreamNodeError(streamId, nodeId, normalizedPort, null);
      deps.scheduleTuneStreamAutoApply(streamId);
      return;
    }
    const typeKey = deps.resolveDataTypeKey(dataType ?? undefined) ?? 'string';
    const variants = deps.getDataTypeVariants(dataType ?? undefined);
    const parsed = deps.buildNodeValueFromInput(raw, typeKey, variants);
    if (!parsed.success) {
      deps.setTuneStreamNodeError(streamId, nodeId, normalizedPort, parsed.error ?? 'Invalid value');
      return;
    }
    deps.setTuneStreamNodeError(streamId, nodeId, normalizedPort, null);
    const prev = deps.getTuneStreamNodeOverridesById()[streamId] ?? {};
    const nodeOverrides = prev[nodeId] ?? {};
    deps.setTuneStreamNodeOverridesById({
      ...deps.getTuneStreamNodeOverridesById(),
      [streamId]: { ...prev, [nodeId]: { ...nodeOverrides, [normalizedPort]: parsed.value } }
    });
    deps.scheduleTuneStreamAutoApply(streamId);
  };

  return { updateGlobalNodeValue, updateStreamNodeValue };
};
