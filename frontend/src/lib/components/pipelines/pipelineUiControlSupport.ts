import type { PipelineUiControlBind, PipelineUiNodeDescriptor } from '$lib/features/pipelines/pipelineUiTypes';

export type RangeBind = { min: string; max: string };
export type HsvBind = { h: string; s: string; v: string };
export type HsvRangeBind = { h: RangeBind; s: RangeBind; v: RangeBind };

export const normalizeNodePortKey = (value: string): string => value.trim().toLowerCase();

export const parseBind = (bind: string): { nodeId: string; portKey: string } | null => {
  const trimmed = bind.trim();
  const dotIndex = trimmed.lastIndexOf('.');
  if (dotIndex <= 0 || dotIndex === trimmed.length - 1) return null;
  const nodeId = trimmed.slice(0, dotIndex).trim();
  const portKey = trimmed.slice(dotIndex + 1).trim();
  if (!nodeId.length || !portKey.length) return null;
  return { nodeId, portKey };
};

const parseNumericNodeId = (value: string): number | null => {
  const trimmed = value.trim();
  if (!/^-?\d+$/u.test(trimmed)) return null;
  const parsed = Number.parseInt(trimmed, 10);
  return Number.isInteger(parsed) ? parsed : null;
};

const descriptorAliases = (entry: PipelineUiNodeDescriptor): string[] => {
  const aliases: string[] = [];
  const push = (value: unknown) => {
    if (typeof value !== 'string') return;
    const trimmed = value.trim();
    if (!trimmed.length) return;
    aliases.push(trimmed);
  };
  push(entry.nodeId);
  push(entry.backendId);
  push(entry.sourceId);
  return aliases;
};

const descriptorNumericNodeId = (entry: PipelineUiNodeDescriptor): number | null => {
  for (const alias of descriptorAliases(entry)) {
    const direct = parseNumericNodeId(alias);
    if (direct != null) return direct;
    const atIndex = alias.lastIndexOf('@');
    if (atIndex > 0) {
      const suffix = parseNumericNodeId(alias.slice(atIndex + 1));
      if (suffix != null) return suffix;
    }
  }
  return null;
};

const pickFallbackDescriptor = (
  parsed: { nodeId: string; portKey: string },
  candidates: PipelineUiNodeDescriptor[]
): PipelineUiNodeDescriptor | null => {
  if (!candidates.length) return null;
  if (candidates.length === 1) return candidates[0] ?? null;

  const requestedNodeId = parsed.nodeId.trim().toLowerCase();
  if (requestedNodeId.length) {
    const exact = candidates.find((entry) =>
      descriptorAliases(entry).some((alias) => alias.toLowerCase() === requestedNodeId)
    );
    if (exact) return exact;
  }

  const requestedNumericId = parseNumericNodeId(parsed.nodeId);
  if (requestedNumericId != null) {
    const scored = candidates
      .map((entry) => ({
        entry,
        distance: (() => {
          const candidateId = descriptorNumericNodeId(entry);
          return candidateId == null ? Number.POSITIVE_INFINITY : Math.abs(candidateId - requestedNumericId);
        })()
      }))
      .sort((a, b) => {
        if (a.distance !== b.distance) return a.distance - b.distance;
        return String(a.entry.nodeId ?? '').localeCompare(String(b.entry.nodeId ?? ''));
      });
    const best = scored[0];
    if (best && Number.isFinite(best.distance)) return best.entry;
  }

  if (requestedNodeId.length) {
    const fuzzy = candidates
      .map((entry) => ({
        entry,
        score: Math.max(
          ...descriptorAliases(entry).map((alias) => {
            const normalized = alias.toLowerCase();
            if (normalized.includes(requestedNodeId)) return requestedNodeId.length;
            if (requestedNodeId.includes(normalized)) return normalized.length;
            return 0;
          })
        )
      }))
      .sort((a, b) => {
        if (a.score !== b.score) return b.score - a.score;
        return String(a.entry.nodeId ?? '').localeCompare(String(b.entry.nodeId ?? ''));
      });
    if (fuzzy[0]?.score > 0) return fuzzy[0].entry;
  }

  return (
    candidates
      .slice()
      .sort((a, b) => String(a.nodeId ?? '').localeCompare(String(b.nodeId ?? '')))[0] ?? null
  );
};

export function resolveDescriptor(
  bind: PipelineUiControlBind | undefined,
  nodeDescriptors: PipelineUiNodeDescriptor[]
): PipelineUiNodeDescriptor | null {
  if (!bind || typeof bind !== 'string') return null;
  const parsed = parseBind(bind);
  if (!parsed) return null;
  const normalized = normalizeNodePortKey(parsed.portKey);
  const candidates = nodeDescriptors.filter((entry) => normalizeNodePortKey(entry.portKey) === normalized);
  if (!candidates.length) return null;

  const direct = candidates.find((entry) => {
    if (entry.nodeId === parsed.nodeId) return true;
    if (typeof entry.backendId === 'string' && entry.backendId === parsed.nodeId) return true;
    return typeof entry.sourceId === 'string' && entry.sourceId === parsed.nodeId;
  });
  if (direct) return direct;

  return pickFallbackDescriptor(parsed, candidates);
}

export function isRangeBind(bind: PipelineUiControlBind | undefined): bind is RangeBind {
  if (!bind || typeof bind !== 'object') return false;
  if ('min' in bind || 'max' in bind) {
    const min = (bind as RangeBind).min;
    const max = (bind as RangeBind).max;
    return typeof min === 'string' && typeof max === 'string';
  }
  return false;
}

export function isHsvBind(bind: PipelineUiControlBind | undefined): bind is HsvBind {
  if (!bind || typeof bind !== 'object') return false;
  if ('h' in bind && 's' in bind && 'v' in bind) {
    const candidate = bind as HsvBind;
    return typeof candidate.h === 'string' && typeof candidate.s === 'string' && typeof candidate.v === 'string';
  }
  return false;
}

export function isHsvRangeBind(bind: PipelineUiControlBind | undefined): bind is HsvRangeBind {
  if (!bind || typeof bind !== 'object') return false;
  if ('h' in bind && 's' in bind && 'v' in bind) {
    const candidate = bind as HsvRangeBind;
    return isRangeBind(candidate.h) && isRangeBind(candidate.s) && isRangeBind(candidate.v);
  }
  return false;
}

export function clampNumber(value: number, min: number, max: number): number {
  if (!Number.isFinite(value)) return min;
  return Math.min(max, Math.max(min, value));
}

export function normalizeHex(value: string, fallback: string, hexColorPattern: RegExp): string {
  return hexColorPattern.test(value) ? value : fallback;
}

export function hsvToHex(h: number, s: number, v: number): string {
  const hue = ((h % 360) + 360) % 360;
  const sat = clampNumber(s, 0, 1);
  const val = clampNumber(v, 0, 1);
  const c = val * sat;
  const x = c * (1 - Math.abs(((hue / 60) % 2) - 1));
  const m = val - c;
  let r = 0;
  let g = 0;
  let b = 0;

  if (hue < 60) {
    r = c;
    g = x;
  } else if (hue < 120) {
    r = x;
    g = c;
  } else if (hue < 180) {
    g = c;
    b = x;
  } else if (hue < 240) {
    g = x;
    b = c;
  } else if (hue < 300) {
    r = x;
    b = c;
  } else {
    r = c;
    b = x;
  }

  const toHex = (value: number) =>
    Math.round(clampNumber((value + m) * 255, 0, 255))
      .toString(16)
      .padStart(2, '0');

  return `#${toHex(r)}${toHex(g)}${toHex(b)}`;
}
