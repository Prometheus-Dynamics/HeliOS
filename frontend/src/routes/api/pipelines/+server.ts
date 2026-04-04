import { json, type RequestHandler } from '@sveltejs/kit';

import { PipelinesApi } from '$lib/api/pipelinesApi';
import { DEFAULT_REQUEST_TIMEOUT_MS } from '$lib/api/requestUtils';
import { buildPipelinePayloadFromOverview } from '$lib/api/pipelinesNormalize';
import type { PipelinePagePayload } from '$lib/types/pipeline';
import type { PipelineLifecycleStatus, PipelineOverviewEntry, PipelineOverviewResponse } from '$lib/types/pipeline-api';

const REQUEST_TIMEOUT_MS = DEFAULT_REQUEST_TIMEOUT_MS;

export const GET: RequestHandler = async ({ url }) => {
  try {
    const payload = await buildPipelinePayload(url.origin);
    return json(payload);
  } catch (error) {
    console.error('Failed to load pipeline payload', error);
    return json({ message: 'Pipeline data unavailable' }, { status: 502 });
  }
};

async function buildPipelinePayload(baseUrl: string): Promise<PipelinePagePayload> {
  const apiOptions = { baseUrl, timeoutMs: REQUEST_TIMEOUT_MS };
  const [summaries, templates] = await Promise.all([
    PipelinesApi.listGraphs(apiOptions),
    PipelinesApi.listTemplates(apiOptions).catch((error) => {
      console.warn('Failed to load pipeline templates', error);
      return [];
    })
  ]);
  const sortedSummaries = Array.isArray(summaries)
    ? [...summaries].sort((a, b) => String(a?.name ?? a?.id ?? '').localeCompare(String(b?.name ?? b?.id ?? '')))
    : [];
  const bootstrapPipelineId =
    typeof sortedSummaries[0]?.id === 'string' && sortedSummaries[0].id.trim().length
      ? sortedSummaries[0].id
      : null;
  const bootstrapGraph =
    bootstrapPipelineId
      ? await PipelinesApi.fetchGraph({ id: bootstrapPipelineId }, apiOptions).catch((error) => {
          console.warn(`Failed to load bootstrap graph for ${bootstrapPipelineId}`, error);
          return null;
        })
      : null;
  const issueCountById = new Map<string, number>();
  for (const entry of summaries ?? []) {
    const id = typeof entry?.id === 'string' ? entry.id : null;
    if (!id) continue;
    const rawIssueCount = (entry as { issue_count?: number; issueCount?: number }).issue_count ?? (entry as { issue_count?: number; issueCount?: number }).issueCount ?? 0;
    const parsed = Number(rawIssueCount);
    const count = Number.isFinite(parsed) ? Math.max(0, Math.floor(parsed)) : 0;
    issueCountById.set(id, count);
  }
  const pipelines = (summaries ?? [])
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
        graph:
          pipelineId === bootstrapPipelineId && bootstrapGraph?.graph
            ? bootstrapGraph.graph
            : ({} satisfies PipelineOverviewEntry['graph']),
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
    .filter((entry): entry is PipelineOverviewEntry => Boolean(entry));

  const overview: PipelineOverviewResponse = {
    pipelines,
    summary: { total: pipelines.length, live: 0, degraded: 0, drafts: pipelines.length },
    templates: Array.isArray(templates) ? templates : [],
    registry: [],
    dataTypes: [],
    generatedAt: Date.now()
  };

  const payload = buildPipelinePayloadFromOverview(overview);
  payload.registry = [];
  return payload;
}
