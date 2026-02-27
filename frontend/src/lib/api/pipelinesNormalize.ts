import { applyPaletteToGraphPlan } from '$lib/features/pipelines/graph';
import { normalizeDiagnostics } from '$lib/features/pipelines/diagnostics';
import { fromApiGraphPlan, fromApiPortDescriptor, normalizeNodeStyle } from '$lib/features/pipelines/model';
import { hydratePipelinesWithRegistry } from '$lib/features/pipelines/styleHydration';
import type {
  PipelineAppearance,
  PipelineAttachmentSummary,
  PipelineDataType,
  PipelinePagePayload,
  PipelineRegistryEntry,
  PipelineTemplateSummary,
  PipelineTypeDescriptor,
  PipelineOverviewPipeline
} from '$lib/types/pipeline';
import type { ApiPortDescriptor, PipelineOverviewEntry, PipelineOverviewResponse } from '$lib/types/pipeline-api';

export const PIPELINE_OVERVIEW_INCLUDES = ['graph', 'attachments', 'templates', 'registry', 'dataTypes'] as const;

export function emptyPipelinePayload(errorMessage?: string): PipelinePagePayload {
  return {
    pipelines: [],
    summary: { total: 0, live: 0, degraded: 0, drafts: 0 },
    templates: [],
    registry: [],
    dataTypes: {},
    generatedAt: Math.floor(Date.now() / 1000),
    errorMessage
  };
}

export function buildPipelinePayloadFromOverview(overview: PipelineOverviewResponse | null | undefined): PipelinePagePayload {
  if (!overview) {
    return emptyPipelinePayload('Pipeline overview unavailable');
  }

  const dataTypes = normalizeDataTypeCatalog(overview.dataTypes);
  const pipelines = normalizePipelines(overview.pipelines, dataTypes);
  const summary = normalizeSummary(overview);
  const templates = normalizeTemplates(overview.templates);
  const registry = normalizeRegistry(overview.registry, dataTypes);
  hydratePipelinesWithRegistry(pipelines, registry);
  const generatedAt = normalizeTimestamp(overview.generatedAt ?? Date.now().toString());

  return {
    pipelines,
    summary,
    templates,
    registry,
    dataTypes,
    generatedAt
  };
}

function normalizeDataTypeCatalog(value: PipelineOverviewResponse['dataTypes']): Record<string, PipelineTypeDescriptor> {
  if (!value || !Array.isArray(value)) {
    return {};
  }
  const catalog: Record<string, PipelineTypeDescriptor> = {};
  for (const entry of value) {
    if (!entry || typeof entry !== 'object') continue;
    const descriptor = entry as PipelineTypeDescriptor & { id?: string };
    const id = typeof descriptor.id === 'string' && descriptor.id.trim() ? descriptor.id.trim() : null;
    if (!id) continue;
    catalog[id] = descriptor;
  }
  return catalog;
}

function normalizePipelines(entries: PipelineOverviewEntry[] | undefined, palette: Record<string, PipelineTypeDescriptor>): PipelineOverviewPipeline[] {
  if (!entries || !Array.isArray(entries)) {
    return [];
  }
  return entries
    .map((entry) => normalizePipeline(entry, palette))
    .filter((item): item is PipelineOverviewPipeline => Boolean(item))
    .sort((a, b) => a.name.localeCompare(b.name));
}

function normalizePipeline(entry: PipelineOverviewEntry, palette: Record<string, PipelineTypeDescriptor>): PipelineOverviewPipeline | null {
  const pipelineId = typeof entry.pipelineId === 'string' ? entry.pipelineId : null;
  const nameRaw = typeof entry.name === 'string' && entry.name.trim() ? entry.name.trim() : null;
  const aliasRaw = typeof entry.alias === 'string' && entry.alias.trim() ? entry.alias.trim() : null;
  if (!pipelineId) {
    return null;
  }
  // Backend name can be absent (e.g. blank graphs); fall back to alias/id so the pipeline stays visible.
  const name = nameRaw ?? aliasRaw ?? pipelineId;
  const alias = aliasRaw ?? nameRaw ?? pipelineId;

  const graphPlan = fromApiGraphPlan(entry.graph);
  const graph = applyPaletteToGraphPlan(graphPlan, palette);
  const attachments = Array.isArray(entry.attachments)
    ? entry.attachments.map(normalizeAttachment).filter((attachment): attachment is PipelineAttachmentSummary => attachment !== null)
    : [];

  const diagnostics = normalizeDiagnostics((entry as Record<string, unknown>).diagnostics);
  const appearance = normalizeAppearance((entry as Record<string, unknown>).appearance);
  const issueCount = normalizeNumber((entry as Record<string, unknown>).issueCount ?? (entry as Record<string, unknown>).issue_count) ?? 0;
  return {
    id: pipelineId,
    name,
    alias,
    status: parseStatus(entry.status),
    revision: typeof entry.revision === 'string' ? entry.revision : null,
    planHash: normalizeOptionalPlanHash(entry.planHash),
    createdAt: normalizeTimestamp(entry.createdAt),
    updatedAt: normalizeTimestamp(entry.updatedAt),
    issueCount,
    graph,
    attachments,
    diagnostics,
    appearance
  };
}

function normalizeAttachment(entry: unknown): PipelineAttachmentSummary | null {
  if (!entry || typeof entry !== 'object') {
    return null;
  }
  const record = entry as Record<string, unknown>;
  // Backend payloads may use snake_case; accept both.
  const captureSessionId =
    typeof record.captureSessionId === 'string'
      ? record.captureSessionId
      : typeof record.capture_session_id === 'string'
        ? (record.capture_session_id as string)
        : null;
  const cameraUid =
    typeof record.cameraUid === 'string'
      ? record.cameraUid
      : typeof record.camera_uid === 'string'
        ? (record.camera_uid as string)
        : null;
  const cameraPath =
    typeof record.cameraPath === 'string'
      ? record.cameraPath
      : typeof record.camera_path === 'string'
        ? (record.camera_path as string)
        : null;
  const planHash = normalizePlanHash((record.planHash ?? (record as any).plan_hash) as any);
  const priority = normalizeNumber(record.priority);
  if (!captureSessionId || !cameraUid || !cameraPath || planHash === null || priority === null) {
    return null;
  }
  return { captureSessionId, cameraUid, cameraPath, planHash, priority };
}

function normalizeAppearance(value: unknown): PipelineAppearance | null {
  if (!value || typeof value !== 'object') {
    return null;
  }
  const record = value as Record<string, unknown>;
  const rawIcon = typeof record.icon === 'string' ? record.icon.trim() : '';
  const rawColor = typeof record.color === 'string' ? record.color.trim() : '';
  const icon = rawIcon.length ? rawIcon : null;
  const color = rawColor.length ? rawColor : null;
  if (!icon && !color) {
    return null;
  }
  const appearance: PipelineAppearance = {};
  if (icon) appearance.icon = icon;
  if (color) appearance.color = color;
  return appearance;
}

function parseStatus(value: unknown): PipelineOverviewPipeline['status'] {
  if (value === 'live' || value === 'degraded' || value === 'draft') return value;
  return 'draft';
}

function normalizeSummary(response: PipelineOverviewResponse): PipelinePagePayload['summary'] {
  const summary = response.summary;
  if (!summary || typeof summary !== 'object') {
    return { total: 0, live: 0, degraded: 0, drafts: 0 };
  }
  return {
    total: normalizeNumber(summary.total) ?? 0,
    live: normalizeNumber(summary.live) ?? 0,
    degraded: normalizeNumber(summary.degraded) ?? 0,
    drafts: normalizeNumber(summary.drafts) ?? 0
  };
}

function normalizeTemplates(entries: PipelineOverviewResponse['templates']): PipelineTemplateSummary[] {
  if (!entries || !Array.isArray(entries)) {
    return [];
  }
  const results: PipelineTemplateSummary[] = [];
  for (const entry of entries) {
    if (!entry || typeof entry !== 'object') continue;
    const record = entry as Record<string, unknown>;
    const templateId =
      typeof record.templateId === 'string'
        ? record.templateId
        : typeof record.template_id === 'string'
          ? record.template_id
          : typeof record.id === 'string'
            ? record.id
            : null;
    const name = typeof record.name === 'string' ? record.name : null;
    if (!templateId || !name) continue;
    const tags = Array.isArray(record.tags)
      ? record.tags.filter((tag): tag is string => typeof tag === 'string' && tag.trim().length > 0)
      : [];
    const summary = optionalString(record.summary) ?? '';
    results.push({
      templateId,
      name,
      summary,
      tags
    });
  }
  return results.sort((a, b) => a.name.localeCompare(b.name));
}

function normalizeRegistry(entries: PipelineOverviewResponse['registry'], palette: Record<string, PipelineTypeDescriptor>): PipelineRegistryEntry[] {
  if (!entries || !Array.isArray(entries)) {
    return [];
  }
  const registry: PipelineRegistryEntry[] = [];
  for (const entry of entries) {
    if (!entry || typeof entry !== 'object') continue;
    const record = entry as {
      id?: string;
      inputs?: Record<string, ApiPortDescriptor>;
      outputs?: Record<string, ApiPortDescriptor>;
      metadata?: {
        name?: string | null;
        summary?: string | null;
        categories?: string[][];
        tags?: string[];
        provider?: string | null;
        style?: unknown;
      };
      style?: unknown;
    };
    const id = typeof record.id === 'string' && record.id.trim() ? record.id.trim() : null;
    if (!id) continue;

    const meta = record.metadata ?? {};
    const name = typeof meta.name === 'string' && meta.name.trim() ? meta.name.trim() : id;
    const summary = optionalString(meta.summary);
    const categories = Array.isArray(meta.categories) ? meta.categories : undefined;
    const tags = Array.isArray(meta.tags) ? meta.tags : undefined;
    const provider = optionalString(meta.provider);
    const style = normalizeNodeStyle(meta.style ?? record.style);
    const inputs = normalizePortMap(record.inputs, palette);
    const outputs = normalizePortMap(record.outputs, palette);

    registry.push({
      id,
      metadata: { name, summary: summary ?? undefined, categories, tags, provider, style },
      inputs,
      outputs
    });
  }
  return registry;
}

function normalizePortMap(
  value: Record<string, ApiPortDescriptor> | undefined,
  palette: Record<string, PipelineTypeDescriptor>
): Record<string, PipelineDataType> {
  const result: Record<string, PipelineDataType> = {};
  if (!value || typeof value !== 'object') return result;
  for (const [key, descriptor] of Object.entries(value)) {
    const dataType = fromApiPortDescriptor(descriptor);
    if (!dataType) continue;
    result[key] = applyPaletteToDataType(dataType, palette);
  }
  return result;
}

function applyPaletteToDataType(dataType: PipelineDataType, palette: Record<string, PipelineTypeDescriptor>): PipelineDataType {
  if (typeof dataType === 'string') {
    const descriptor = palette[dataType];
    return descriptor ? { kind: dataType, descriptor } : dataType;
  }
  const descriptor = palette[dataType.kind];
  const enriched: PipelineDataType = {
    ...dataType,
    descriptor: descriptor ?? dataType.descriptor
  };
  if (dataType.element) {
    (enriched as { element?: PipelineDataType }).element = applyPaletteToDataType(dataType.element, palette);
  }
  return enriched;
}

function normalizeTimestamp(value: unknown): number {
  if (typeof value === 'number' && Number.isFinite(value)) {
    return Math.floor(value);
  }
  if (typeof value === 'string') {
    const parsed = Date.parse(value);
    if (Number.isFinite(parsed)) {
      return Math.floor(parsed / 1000);
    }
  }
  return Math.floor(Date.now() / 1000);
}

function normalizeNumber(value: unknown): number | null {
  if (typeof value === 'number' && Number.isFinite(value)) return value;
  if (typeof value === 'string') {
    const parsed = Number.parseFloat(value);
    if (Number.isFinite(parsed)) {
      return parsed;
    }
  }
  return null;
}

function normalizePlanHash(value: unknown): string | null {
  if (typeof value === 'string' && value.trim()) {
    return value.trim();
  }
  return null;
}

function normalizeOptionalPlanHash(value: unknown): string | null {
  if (value == null) return null;
  return normalizePlanHash(value);
}

function optionalString(value: unknown): string | undefined {
  if (typeof value !== 'string') return undefined;
  const trimmed = value.trim();
  return trimmed.length ? trimmed : undefined;
}
