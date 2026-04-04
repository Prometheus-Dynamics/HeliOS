import { REQUESTED_BY } from '../../../../routes/settings/api';
import type {
  LightingAnimationTemplateDocument,
  LightingAnimationTemplateSummary,
  LightingColorPayload,
  LightingFramePayload,
  LightingRuntimeState,
  SavedLightingAnimation
} from './lightingModalUtils';

export type LoadAnimationRef = {
  kind: 'saved' | 'template';
  value: string;
};

export type LightingAnimationImportBundle = {
  version?: number;
  exported_at_ms?: number;
  animations?: SavedLightingAnimation[];
};

export function encodeLoadAnimationRef(
  savedPrefix: string,
  templatePrefix: string,
  kind: 'saved' | 'template',
  value: string
): string {
  return `${kind === 'saved' ? savedPrefix : templatePrefix}${value}`;
}

export function decodeLoadAnimationRef(
  savedPrefix: string,
  templatePrefix: string,
  value: string
): LoadAnimationRef | null {
  const trimmed = value.trim();
  if (trimmed.startsWith(savedPrefix)) {
    return { kind: 'saved', value: trimmed.slice(savedPrefix.length) };
  }
  if (trimmed.startsWith(templatePrefix)) {
    return { kind: 'template', value: trimmed.slice(templatePrefix.length) };
  }
  return null;
}

export function normalizedAnimationKey(value: string): string {
  return value.trim().toLowerCase();
}

export function hasLoadAnimationRef(args: {
  savedPrefix: string;
  templatePrefix: string;
  ref: string;
  visibleSavedAnimations: SavedLightingAnimation[];
  lightingTemplates: LightingAnimationTemplateSummary[];
  fallbackTemplateAnimations: SavedLightingAnimation[];
}): boolean {
  const decoded = decodeLoadAnimationRef(
    args.savedPrefix,
    args.templatePrefix,
    args.ref
  );
  if (!decoded) return false;
  if (decoded.kind === 'saved') {
    return args.visibleSavedAnimations.some((entry) => entry.name === decoded.value);
  }
  const selectedKey = normalizedAnimationKey(decoded.value);
  return (
    args.lightingTemplates.some(
      (entry) =>
        entry.template_id === decoded.value ||
        normalizedAnimationKey(entry.name) === selectedKey
    ) ||
    args.fallbackTemplateAnimations.some(
      (entry) => normalizedAnimationKey(entry.name) === selectedKey
    )
  );
}

export function defaultLoadAnimationRef(args: {
  savedPrefix: string;
  templatePrefix: string;
  visibleSavedAnimations: SavedLightingAnimation[];
  lightingTemplates: LightingAnimationTemplateSummary[];
  fallbackTemplateAnimations: SavedLightingAnimation[];
}): string {
  if (args.visibleSavedAnimations.length > 0) {
    return encodeLoadAnimationRef(
      args.savedPrefix,
      args.templatePrefix,
      'saved',
      args.visibleSavedAnimations[0].name
    );
  }
  if (args.lightingTemplates.length > 0) {
    return encodeLoadAnimationRef(
      args.savedPrefix,
      args.templatePrefix,
      'template',
      args.lightingTemplates[0].template_id
    );
  }
  if (args.fallbackTemplateAnimations.length > 0) {
    return encodeLoadAnimationRef(
      args.savedPrefix,
      args.templatePrefix,
      'template',
      args.fallbackTemplateAnimations[0].name
    );
  }
  return '';
}

export function runtimeAnimationKind(
  state: LightingRuntimeState
):
  | 'off'
  | 'chase'
  | 'pulse'
  | 'rainbow'
  | 'breathing_rainbow'
  | null {
  const kind = state.animation?.kind;
  if (
    kind === 'off' ||
    kind === 'chase' ||
    kind === 'pulse' ||
    kind === 'rainbow' ||
    kind === 'breathing_rainbow'
  ) {
    return kind;
  }
  return null;
}

export function describeRuntimeState(state: LightingRuntimeState | null): string {
  if (!state) return 'Unknown';
  const kind = runtimeAnimationKind(state);
  if (kind && kind !== 'off') {
    return `Animation: ${kind.replace('_', ' ')}`;
  }
  if (Array.isArray(state.frame)) {
    return `Frame output (${state.frame.length} LEDs)`;
  }
  return 'Idle';
}

export function formatRuntimeAnimationLabel(
  kind: Exclude<ReturnType<typeof runtimeAnimationKind>, null>
): string {
  return kind.replace('_', ' ');
}

export function templateToSavedAnimation(
  template: LightingAnimationTemplateDocument
): SavedLightingAnimation {
  return {
    name: template.name,
    frame: template.frame ?? null,
    frames: template.frames ?? null,
    timeline: template.timeline ?? null,
    brightness: template.brightness ?? null,
    animation: template.animation ?? null,
    duration_ms: template.duration_ms ?? null
  };
}

export function currentAnimationExportName(animationName: string): string {
  const base = animationName.trim() || 'lighting-animations';
  const safe = base
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '');
  const stamp = new Date()
    .toISOString()
    .replace(/[-:]/g, '')
    .replace(/\..+/, '')
    .replace('T', '-');
  return `${safe || 'lighting-animations'}-${stamp}.json`;
}

export function buildAnimationImportBundle(
  savedAnimations: SavedLightingAnimation[]
): LightingAnimationImportBundle {
  return {
    version: 1,
    exported_at_ms: Date.now(),
    animations: savedAnimations.map((entry) => ({
      name: entry.name,
      frame: entry.frame ?? null,
      frames: entry.frames ?? null,
      timeline: entry.timeline ?? null,
      brightness: entry.brightness ?? null,
      animation: entry.animation ?? null,
      duration_ms: entry.duration_ms ?? null
    }))
  };
}

export function buildSaveBodyFromImportedEntry(
  entry: SavedLightingAnimation,
  fallbackName: string
): Record<string, unknown> | null {
  const name = entry.name?.trim() || fallbackName;
  const body: Record<string, unknown> = {
    name,
    requested_by: REQUESTED_BY
  };
  if (typeof entry.brightness === 'number' && Number.isFinite(entry.brightness)) {
    body.brightness = Math.min(255, Math.max(0, Math.trunc(entry.brightness)));
  }
  if (entry.timeline?.keyframes?.length) {
    body.timeline = entry.timeline;
    if (typeof entry.duration_ms === 'number' && Number.isFinite(entry.duration_ms)) {
      body.duration_ms = Math.max(0, Math.trunc(entry.duration_ms));
    }
    return body;
  }
  if (entry.frames?.length) {
    body.frames = entry.frames;
    if (typeof entry.duration_ms === 'number' && Number.isFinite(entry.duration_ms)) {
      body.duration_ms = Math.max(0, Math.trunc(entry.duration_ms));
    }
    return body;
  }
  if (entry.frame?.length) {
    body.frame = entry.frame;
    if (typeof entry.duration_ms === 'number' && Number.isFinite(entry.duration_ms)) {
      body.duration_ms = Math.max(0, Math.trunc(entry.duration_ms));
    }
    return body;
  }
  if (entry.animation) {
    body.animation = entry.animation;
    if (typeof entry.duration_ms === 'number' && Number.isFinite(entry.duration_ms)) {
      body.duration_ms = Math.max(0, Math.trunc(entry.duration_ms));
    }
    return body;
  }
  return null;
}

export function normalizeImportedEntries(payload: unknown): SavedLightingAnimation[] {
  if (payload == null) return [];
  if (Array.isArray(payload)) return payload as SavedLightingAnimation[];
  if (typeof payload !== 'object') return [];
  const record = payload as Record<string, unknown>;
  if (Array.isArray(record.animations)) {
    return record.animations as SavedLightingAnimation[];
  }
  if (
    typeof record.name === 'string' &&
    (Array.isArray(record.frame) ||
      Array.isArray(record.frames) ||
      typeof record.animation === 'object' ||
      typeof record.timeline === 'object')
  ) {
    return [record as SavedLightingAnimation];
  }
  return [];
}
