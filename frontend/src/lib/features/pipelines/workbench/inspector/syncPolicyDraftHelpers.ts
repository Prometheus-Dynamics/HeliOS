import type { PipelineNodeSyncConfig, PipelineSyncGroupConfig, PipelineTickMode } from '$lib/types/pipeline';
import { canonicalPortName } from './syncPolicyUtils';

export const GROUP_ACCENTS = [
  'var(--color-primary-400)',
  'var(--color-secondary-300)',
  'var(--color-tertiary-300)',
  'var(--color-success-300)',
  'var(--color-warning-300)',
  'var(--color-error-300)'
];

export const groupAccentColor = (index: number) => GROUP_ACCENTS[index % GROUP_ACCENTS.length];

export const generateGroupId = (existingIds: string[], hint?: string): string => {
  const existing = new Set(existingIds);
  const base = hint?.trim() || `group-${existing.size + 1}`;
  if (!existing.has(base)) {
    return base;
  }
  let counter = existing.size + 1;
  let candidate = `${base}-${counter}`;
  while (existing.has(candidate)) {
    counter += 1;
    candidate = `${base}-${counter}`;
  }
  return candidate;
};

export function updateTickMode(draft: PipelineNodeSyncConfig, mode: PipelineTickMode) {
  if (mode === 'allGroups' || mode === 'anyGroup') {
    draft.tickPolicy.mode = mode;
  } else {
    draft.tickPolicy.mode = { primaryGroup: mode.primaryGroup };
  }
}

export function setPrimaryTickGroup(draft: PipelineNodeSyncConfig, groupId: string) {
  draft.tickPolicy.mode = { primaryGroup: groupId };
}

export function toggleRequiredGroup(draft: PipelineNodeSyncConfig, groupId: string, enabled: boolean) {
  const set = new Set(draft.tickPolicy.requiredGroups);
  if (enabled) {
    set.add(groupId);
  } else {
    set.delete(groupId);
  }
  draft.tickPolicy.requiredGroups = Array.from(set);
}

export function removeGroup(draft: PipelineNodeSyncConfig, index: number) {
  const target = draft.groups?.[index];
  if (!target) return;
  draft.groups.splice(index, 1);
  draft.tickPolicy.requiredGroups = draft.tickPolicy.requiredGroups.filter((id) => id !== target.id);
  if (typeof draft.tickPolicy.mode === 'object' && 'primaryGroup' in draft.tickPolicy.mode) {
    if (draft.tickPolicy.mode.primaryGroup === target.id) {
      draft.tickPolicy.mode = draft.groups.length > 0 ? { primaryGroup: draft.groups[0].id } : 'allGroups';
    }
  }
}

export function duplicateGroup(draft: PipelineNodeSyncConfig, index: number, nextId: string) {
  const template = draft.groups?.[index];
  if (!template) return;
  draft.groups.splice(index + 1, 0, {
    id: nextId,
    ports: [...(template.ports ?? [])],
    matchKey: template.matchKey,
    readiness: template.readiness,
    staleness: template.staleness ? { ...template.staleness } : { kind: 'allowAny' },
    drop: template.drop,
    missing: template.missing ? { ...template.missing } : { kind: 'allowNone' }
  });
}

export function updateGroup(draft: PipelineNodeSyncConfig, index: number, updater: (group: PipelineSyncGroupConfig) => void) {
  const group = draft.groups?.[index];
  if (!group) return;
  updater(group);
}

export function renameGroup(draft: PipelineNodeSyncConfig, index: number, nextId: string) {
  const group = draft.groups?.[index];
  if (!group) return;
  const previous = group.id;
  group.id = nextId;
  draft.tickPolicy.requiredGroups = draft.tickPolicy.requiredGroups.map((id) => (id === previous ? nextId : id));
  if (typeof draft.tickPolicy.mode === 'object' && 'primaryGroup' in draft.tickPolicy.mode) {
    if (draft.tickPolicy.mode.primaryGroup === previous) {
      draft.tickPolicy.mode = { primaryGroup: nextId };
    }
  }
}

export function applyGroupWithPorts(
  draft: PipelineNodeSyncConfig,
  ports: string[],
  idHint: string | undefined,
  overrides: Partial<PipelineSyncGroupConfig> | undefined,
  existingIds: string[]
) {
  if (!ports.length) return;
  const normalized = ports.map((port) => canonicalPortName(port));
  const id = generateGroupId(existingIds, idHint);
  draft.groups.push({
    id,
    ports: normalized,
    matchKey: overrides?.matchKey ?? 'workId',
    readiness: overrides?.readiness ?? 'allSameKey',
    staleness: overrides?.staleness ?? { kind: 'requireExact' },
    drop: overrides?.drop ?? 'dropOldest',
    missing: overrides?.missing ?? { kind: 'wait', timeoutMs: 10 }
  });
  if (!draft.tickPolicy.requiredGroups.includes(id)) {
    draft.tickPolicy.requiredGroups.push(id);
  }
}

export function movePortToGroup(draft: PipelineNodeSyncConfig, port: string, targetGroupId: string | null) {
  const canonical = canonicalPortName(port);
  draft.groups.forEach((group) => {
    group.ports = (group.ports ?? []).filter((entry) => canonicalPortName(entry) !== canonical);
  });
  if (!targetGroupId) return;
  const group = draft.groups.find((entry) => entry.id === targetGroupId);
  if (!group) return;
  const existing = new Set((group.ports ?? []).map((entry) => canonicalPortName(entry)));
  existing.add(canonical);
  group.ports = Array.from(existing);
}

export function syncAllInputs(
  draft: PipelineNodeSyncConfig,
  selectedNodePorts: string[]
) {
  const id = generateGroupId(draft.groups.map((group) => group.id), 'all');
  draft.groups = [
    {
      id,
      ports: selectedNodePorts.map((port) => canonicalPortName(port)),
      matchKey: 'workId',
      readiness: 'allSameKey',
      staleness: { kind: 'requireExact' },
      drop: 'dropOldest',
      missing: { kind: 'wait', timeoutMs: 10 }
    }
  ];
  draft.tickPolicy.requiredGroups = [id];
  draft.tickPolicy.mode = 'allGroups';
}

export function splitPortsIntoGroups(
  draft: PipelineNodeSyncConfig,
  selectedNodePorts: string[]
) {
  draft.groups = selectedNodePorts.map((port) => ({
    id: generateGroupId(draft.groups.map((group) => group.id), port),
    ports: [canonicalPortName(port)],
    matchKey: 'workId',
    readiness: 'any',
    staleness: { kind: 'allowAny' },
    drop: 'keepLatest',
    missing: { kind: 'allowNone' }
  }));
  draft.tickPolicy.requiredGroups = draft.groups.map((group) => group.id);
  draft.tickPolicy.mode = draft.groups.length > 1 ? 'allGroups' : 'anyGroup';
}
