import { PipelinesApi } from '$lib/api/pipelinesApi';
import { buildPipelinePayloadFromOverview, emptyPipelinePayload } from '$lib/api/pipelinesNormalize';
import type { PipelinePagePayload, PipelineTypeDescriptor } from '$lib/types/pipeline';
import type { PipelineLifecycleStatus, PipelineOverviewEntry, PipelineOverviewResponse } from '$lib/types/pipeline-api';

export async function fetchPipelinePagePayload(_: (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>): Promise<PipelinePagePayload> {
  try {
    const [summaries, templates] = await Promise.all([
      PipelinesApi.listGraphs(),
      PipelinesApi.listTemplates().catch((error) => {
        console.warn('Failed to load pipeline templates', error);
        return [];
      })
    ]);
    const issueCountById = new Map<string, number>();
    for (const entry of summaries ?? []) {
      const id = typeof entry?.id === 'string' ? entry.id : null;
      if (!id) continue;
      const rawIssueCount = (entry as { issue_count?: number; issueCount?: number }).issue_count ?? (entry as { issue_count?: number; issueCount?: number }).issueCount ?? 0;
      const parsed = Number(rawIssueCount);
      const count = Number.isFinite(parsed) ? Math.max(0, Math.floor(parsed)) : 0;
      issueCountById.set(id, count);
    }

    const pipelines: PipelineOverviewEntry[] = (summaries ?? [])
      .map((entry): PipelineOverviewEntry | null => {
        const pipelineId = typeof entry?.id === 'string' ? entry.id : null;
        if (!pipelineId) return null;
        const rawName = typeof entry?.name === 'string' ? entry.name.trim() : '';
        const name = rawName.length > 0 ? rawName : pipelineId;
        const updatedAtMs = typeof entry?.updated_at_ms === 'number' ? entry.updated_at_ms : null;
        return {
          pipelineId,
          name,
          alias: name,
          issueCount: issueCountById.get(pipelineId) ?? 0,
          graph: { nodes: {}, connections: [] } satisfies PipelineOverviewEntry['graph'],
          attachments: [],
          diagnostics: null,
          appearance: null,
          status: 'draft' satisfies PipelineLifecycleStatus,
          revision: updatedAtMs ? String(updatedAtMs) : null,
          planHash: null,
          createdAt: updatedAtMs ?? null,
          updatedAt: updatedAtMs ?? null
        };
      })
      .filter((entry): entry is PipelineOverviewEntry => Boolean(entry))
      .sort((a, b) => a.name.localeCompare(b.name));

    const overview: PipelineOverviewResponse = {
      pipelines,
      summary: { total: pipelines.length, live: 0, degraded: 0, drafts: pipelines.length },
      templates: Array.isArray(templates) ? templates : [],
      registry: [],
      dataTypes: [],
      generatedAt: Date.now()
    };

    return buildPipelinePayloadFromOverview(overview);
  } catch (error) {
    const message = error instanceof Error ? error.message : 'Unexpected error loading pipeline overview';
    console.warn(message, error);
    return emptyPipelinePayload(message);
  }
}

export function validatePipelinePayload(payload: unknown): PipelinePagePayload | null {
  if (!payload || typeof payload !== 'object') return null;
  const record = payload as Record<string, unknown>;
  if (!Array.isArray(record.pipelines)) return null;
  if (!record.summary || typeof record.summary !== 'object') return null;
  if (record.templates && !Array.isArray(record.templates)) return null;
  if (record.registry && !Array.isArray(record.registry)) return null;
  if (record.dataTypes && typeof record.dataTypes !== 'object') return null;
  const generatedAt = normalizeGeneratedAt(record.generatedAt);
  if (generatedAt == null) return null;
  const normalized: PipelinePagePayload = {
    pipelines: Array.isArray(record.pipelines) ? (record.pipelines as PipelinePagePayload['pipelines']) : [],
    summary: record.summary as PipelinePagePayload['summary'],
    templates: Array.isArray(record.templates) ? (record.templates as PipelinePagePayload['templates']) : [],
    registry: Array.isArray(record.registry) ? (record.registry as PipelinePagePayload['registry']) : [],
    dataTypes:
      typeof record.dataTypes === 'object' && record.dataTypes !== null ? (record.dataTypes as Record<string, PipelineTypeDescriptor>) : {},
    generatedAt,
    errorMessage: typeof record.errorMessage === 'string' ? record.errorMessage : undefined
  };
  return normalized;
}

export { emptyPipelinePayload } from '$lib/api/pipelinesNormalize';

function normalizeGeneratedAt(value: unknown): number | null {
  if (typeof value === 'number' && Number.isFinite(value)) {
    return value;
  }
  if (typeof value === 'string') {
    const parsed = Date.parse(value);
    if (Number.isFinite(parsed)) {
      return Math.floor(parsed / 1000);
    }
  }
  return null;
}
