import type {
  PipelineDataType,
  PipelineMissingDataPolicy,
  PipelineNodeStyle,
  PipelineReadinessPolicy,
  PipelineStalenessPolicy,
  PipelineSyncDropPolicy,
  PipelineTickMode,
  PipelineTickPolicy,
  PipelineTickSource,
  PipelineTypeDescriptor,
  PipelineWorkKey
} from '$lib/types/pipeline';

type Palette = Record<string, PipelineTypeDescriptor>;

export function cloneUnknown<T>(value: T): T {
  if (value === null || typeof value !== 'object') {
    return value;
  }
  if (Array.isArray(value)) {
    return value.map((item) => cloneUnknown(item)) as unknown as T;
  }
  return Object.fromEntries(
    Object.entries(value as Record<string, unknown>).map(([key, item]) => [key, cloneUnknown(item)])
  ) as T;
}

export const trimmedString = (value: unknown): string | undefined => {
  if (typeof value !== 'string') return undefined;
  const trimmed = value.trim();
  return trimmed ? trimmed : undefined;
};

export const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);

export const parseStringArray = (value: unknown): string[] | undefined => {
  if (!Array.isArray(value)) return undefined;
  const entries = value
    .map((entry) => trimmedString(entry))
    .filter((entry): entry is string => Boolean(entry));
  return entries.length > 0 ? entries : [];
};

export const parseWorkKey = (value: unknown): PipelineWorkKey | undefined => {
  if (value !== 'workId' && value !== 'timestamp' && value !== 'localId') {
    return undefined;
  }
  return value;
};

export const parseReadinessPolicy = (value: unknown): PipelineReadinessPolicy | undefined => {
  if (value === 'allSameKey' || value === 'any') {
    return value;
  }
  return undefined;
};

export const parseDropPolicy = (value: unknown): PipelineSyncDropPolicy | undefined => {
  if (value === 'dropOldest' || value === 'dropNewest' || value === 'keepLatest' || value === 'block') {
    return value;
  }
  return undefined;
};

export const parseStalenessPolicy = (value: unknown): PipelineStalenessPolicy | undefined => {
  if (value === 'allowAny' || value === 'requireExact') {
    return { kind: value };
  }
  if (isRecord(value) && isRecord(value.maxLagCount)) {
    const maxDistance = Number(value.maxLagCount.maxDistance);
    if (Number.isFinite(maxDistance)) {
      return { kind: 'maxLagCount', maxDistance };
    }
  }
  if (isRecord(value) && isRecord(value.maxLagDuration)) {
    const maxLagMs = Number(value.maxLagDuration.maxLagMs);
    if (Number.isFinite(maxLagMs)) {
      return { kind: 'maxLagDuration', maxLagMs };
    }
  }
  return undefined;
};

export const parseMissingPolicy = (value: unknown): PipelineMissingDataPolicy | undefined => {
  if (value === 'allowNone' || value === 'skipTick') {
    return { kind: value };
  }
  if (isRecord(value) && isRecord(value.wait)) {
    const timeoutMsRaw = value.wait.timeoutMs;
    const timeoutMs = typeof timeoutMsRaw === 'number' ? timeoutMsRaw : undefined;
    return { kind: 'wait', timeoutMs: timeoutMs ?? undefined };
  }
  return undefined;
};

export const parseTickMode = (value: unknown): PipelineTickMode | undefined => {
  if (value === 'allGroups' || value === 'anyGroup') {
    return value;
  }
  if (isRecord(value) && typeof value.primaryGroup === 'string' && value.primaryGroup.trim()) {
    return { primaryGroup: value.primaryGroup.trim() };
  }
  return undefined;
};

export const parseTickPolicy = (value: unknown): PipelineTickPolicy | undefined => {
  if (!isRecord(value)) return undefined;
  const rawMode = parseTickMode(value.mode);
  if (!rawMode) return undefined;
  const requiredGroups = parseStringArray(value.requiredGroups) ?? [];
  return {
    requiredGroups,
    mode: rawMode
  };
};

export const parseTickSource = (value: unknown): PipelineTickSource | undefined => {
  if (value === undefined || value === null) {
    return { kind: 'ports' };
  }
  if (value === 'ports') {
    return { kind: 'ports' };
  }
  if (isRecord(value)) {
    if (value.ports !== undefined) {
      return { kind: 'ports' };
    }
    const timerValue = value.timer ?? value.Timer ?? value.timerSource;
    if (isRecord(timerValue)) {
      const rawInterval = timerValue.intervalMs ?? timerValue.interval_ms ?? timerValue.interval;
      const interval = typeof rawInterval === 'number' ? rawInterval : Number(rawInterval);
      if (Number.isFinite(interval) && interval > 0) {
        return { kind: 'timer', intervalMs: interval };
      }
    }
  }
  return undefined;
};

export function normalizeNodeStyle(value: unknown): PipelineNodeStyle | undefined {
  if (!value || typeof value !== 'object') return undefined;
  const record = value as Record<string, unknown>;
  const rawBg =
    trimmedString(record.bg_color) ??
    trimmedString((record as { bgColor?: unknown }).bgColor);
  const rawBorder =
    trimmedString(record.border_color) ??
    trimmedString((record as { borderColor?: unknown }).borderColor);
  if (!rawBg && !rawBorder) {
    return undefined;
  }
  const bg = rawBg ?? rawBorder ?? '#0f172a';
  const border = rawBorder ?? rawBg ?? '#1f2937';
  return { bg_color: bg, border_color: border };
}

const attachDescriptor = (target: Exclude<PipelineDataType, string>, descriptor: PipelineTypeDescriptor) => {
  Object.defineProperty(target, 'descriptor', {
    value: descriptor,
    enumerable: false,
    configurable: true,
    writable: true
  });
};

const normalizeTypeKeyCandidates = (value: string): string[] => {
  const trimmed = value.trim();
  if (!trimmed) return [];
  const candidates = new Set<string>();
  const add = (candidate: string | null | undefined) => {
    if (!candidate) return;
    const cleaned = candidate.trim();
    if (!cleaned) return;
    candidates.add(cleaned);
  };

  add(trimmed);
  add(trimmed.toLowerCase());

  if (trimmed.toLowerCase().startsWith('rust:')) {
    const withoutPrefix = trimmed.slice(5);
    add(withoutPrefix);
    add(withoutPrefix.toLowerCase());
  }

  const suffixMatch = trimmed.match(/\(([^)]+)\)\s*$/);
  if (suffixMatch?.[1]) {
    add(suffixMatch[1]);
  }

  const lt = trimmed.indexOf('<');
  if (lt >= 0) {
    const base = trimmed.slice(0, lt).trim();
    add(base);
    const end = trimmed.lastIndexOf('>');
    const inner = trimmed.slice(lt + 1, end > lt ? end : trimmed.length).trim();
    if (inner) {
      add(inner);
      inner.split(',').forEach((part) => add(part));
    }
  }

  Array.from(candidates).forEach((candidate) => {
    const nsParts = candidate.split('::').filter(Boolean);
    if (nsParts.length > 0) {
      add(nsParts[nsParts.length - 1]);
    }
    const colonIndex = candidate.lastIndexOf(':');
    if (colonIndex >= 0) {
      add(candidate.slice(colonIndex + 1));
    }
  });

  Array.from(candidates).forEach((candidate) => {
    const lowered = candidate.toLowerCase();
    if (lowered.includes('image')) add('image');
    if (lowered.includes('enum')) add('enum');
    if (lowered.includes('json')) add('json');
  });

  return Array.from(candidates);
};

export const resolveDescriptorFromPalette = (
  value: string,
  palette: Palette
): PipelineTypeDescriptor | undefined => {
  const candidates = normalizeTypeKeyCandidates(value);
  const lowerMap = new Map<string, PipelineTypeDescriptor>();
  Object.entries(palette).forEach(([key, descriptor]) => {
    lowerMap.set(key.toLowerCase(), descriptor);
  });

  for (const candidate of candidates) {
    const direct = palette[candidate];
    if (direct) return direct;
    const lower = lowerMap.get(candidate.toLowerCase());
    if (lower) return lower;
  }

  const loweredCandidates = new Set(candidates.map((candidate) => candidate.toLowerCase()));
  return Object.values(palette).find((descriptor) => {
    const label = descriptor.label?.toLowerCase();
    return label ? loweredCandidates.has(label) : false;
  });
};

export function normalizeOverrides(
  value: Exclude<PipelineDataType, string>,
  descriptor: PipelineTypeDescriptor | undefined
): Exclude<PipelineDataType, string> {
  const normalized = value;

  if (descriptor) {
    attachDescriptor(normalized, descriptor);
    if (normalized.label && descriptor.label && normalized.label === descriptor.label) {
      delete normalized.label;
    }
    if (normalized.summary && descriptor.summary && normalized.summary === descriptor.summary) {
      delete normalized.summary;
    }
    if (typeof normalized.settable === 'boolean' && normalized.settable === descriptor.settable) {
      delete normalized.settable;
    }
  }

  if (!normalized.label) delete normalized.label;
  const descriptorColor = descriptor?.color?.trim();
  if (!normalized.color && descriptorColor) {
    normalized.color = descriptorColor;
  } else if (!normalized.color) {
    delete normalized.color;
  }
  if (!normalized.summary) delete normalized.summary;
  if (!normalized.format) delete normalized.format;
  if (Array.isArray(normalized.variants) && normalized.variants.length === 0) {
    delete normalized.variants;
  }

  return normalized;
}
