import { PIPELINE_INPUT_BACKEND_ID } from '$lib/features/pipelines/boundary';
import type {
  PipelineGraphNode,
  PipelineNodeSyncConfig,
  PipelineSyncGroupConfig
} from '$lib/types/pipeline';

export const canonicalPortName = (value: string): string => value.trim().toLowerCase();

export const nodeSyncPorts = (node: PipelineGraphNode | null): string[] => {
  if (!node) return [];
  const backendId = node.backendId?.toLowerCase() ?? '';
  const record = backendId === PIPELINE_INPUT_BACKEND_ID ? node.outputs ?? {} : node.inputs ?? {};
  return Object.keys(record).map((port) => canonicalPortName(port));
};

export const metadataAlignedPortHints = (node: PipelineGraphNode | null): string[] => {
  if (!node) return [];
  const available = nodeSyncPorts(node);
  if (available.length === 0) return [];
  const sourceMetadata = (
    node.source as { metadata?: { alignedInputs?: string[] | null; aligned_inputs?: string[] | null } } | null
  )?.metadata;
  const rawHints =
    sourceMetadata?.alignedInputs ?? sourceMetadata?.aligned_inputs ?? node.metadata?.alignedInputs;
  if (!Array.isArray(rawHints) || rawHints.length === 0) {
    return [];
  }
  const normalized = rawHints
    .map((entry) => (typeof entry === 'string' ? canonicalPortName(entry) : ''))
    .filter((entry, index, array) => entry.length > 0 && array.indexOf(entry) === index);
  if (normalized.includes('*')) {
    return available;
  }
  return available.filter((port) => normalized.includes(port));
};

export const validateSyncConfig = (
  config: PipelineNodeSyncConfig | null,
  ports: string[]
): string[] => {
  if (!config) return [];
  const errors: string[] = [];
  const seenIds = new Set<string>();
  const portAssignments = new Map<string, string>();
  config.groups?.forEach((group) => {
    const id = group.id?.trim();
    if (!id || seenIds.has(id)) {
      errors.push(`Group ID '${group?.id}' is invalid or duplicated.`);
    } else {
      seenIds.add(id);
    }
    const groupPorts = (group.ports ?? []).map((port) => canonicalPortName(port));
    if (groupPorts.length === 0) {
      errors.push(`Group '${group.id ?? '?'}' has no ports assigned.`);
    }
    groupPorts.forEach((port) => {
      if (!ports.includes(port)) {
        errors.push(`Port '${port}' in group '${group.id ?? '?'}' is not available on this node.`);
      }
      const owner = portAssignments.get(port);
      if (owner && owner !== id) {
        errors.push(`Port '${port}' is assigned to groups '${owner}' and '${id}'.`);
      } else if (id) {
        portAssignments.set(port, id);
      }
    });
  });
  const required = config.tickPolicy.requiredGroups ?? [];
  const missingIds = required.filter((id) => !seenIds.has(id));
  if (missingIds.length) {
    errors.push(`Tick policy references missing groups: ${missingIds.join(', ')}.`);
  }
  if (
    typeof config.tickPolicy.mode === 'object' &&
    'primaryGroup' in config.tickPolicy.mode &&
    config.tickPolicy.mode.primaryGroup &&
    !seenIds.has(config.tickPolicy.mode.primaryGroup)
  ) {
    errors.push(`Primary group '${config.tickPolicy.mode.primaryGroup}' does not exist.`);
  }
  return errors;
};

export const listUnassignedPorts = (
  config: PipelineNodeSyncConfig | null,
  validPorts: string[]
): string[] => {
  if (!config) return [...validPorts];
  const assigned = new Set<string>();
  config.groups?.forEach((group) => {
    group.ports?.forEach((port) => assigned.add(canonicalPortName(port)));
  });
  return validPorts.filter((port) => !assigned.has(port));
};

export const defaultSyncConfigForNode = (node: PipelineGraphNode): PipelineNodeSyncConfig => {
  const ports = nodeSyncPorts(node);
  const groupId = 'group-1';
  return {
    groups: ports.length
      ? [
          {
            id: groupId,
            ports,
            matchKey: 'workId',
            readiness: 'allSameKey',
            staleness: { kind: 'requireExact' },
            drop: 'dropOldest',
            missing: { kind: 'wait', timeoutMs: 10 }
          }
        ]
      : [],
    tickPolicy: {
      requiredGroups: ports.length ? [groupId] : [],
      mode: 'allGroups'
    },
    tickSource: { kind: 'ports' }
  };
};

export const parsePortsInput = (value: string): string[] =>
  value
    .split(',')
    .map((entry) => canonicalPortName(entry))
    .filter((entry, index, array) => entry.length > 0 && array.indexOf(entry) === index);

export const cloneGroup = (group: PipelineSyncGroupConfig): PipelineSyncGroupConfig => ({
  id: group.id,
  ports: [...(group.ports ?? [])],
  matchKey: group.matchKey,
  readiness: group.readiness,
  staleness: group.staleness ? { ...group.staleness } : { kind: 'allowAny' },
  drop: group.drop,
  missing: group.missing ? { ...group.missing } : { kind: 'allowNone' }
});
