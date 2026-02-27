import type { PipelineAppearance, PipelineStatus } from '$lib/types/pipeline';
import { formatStatus, statusBadgeClass } from '$lib/features/pipelines/controller/uiAdapters';

type PipelineListSource = {
  id: string;
  name: string;
  alias: string;
  appearance: PipelineAppearance | null;
  status: PipelineStatus;
  attachments: number;
  issueCount: number;
  revision: string | null;
};

type PipelineListItem = {
  id: string;
  name: string;
  alias: string;
  appearance: PipelineAppearance | null;
  status: PipelineStatus;
  statusLabel: string;
  statusClass: string;
  attachments: number;
  issueCount: number;
  revision: string | null;
};

type PipelineListWorkerPayload = {
  requestId: number;
  pipelines: PipelineListSource[];
  search: string;
};

type PipelineListWorkerResponse = {
  requestId: number;
  summary: { total: number; live: number; degraded: number; drafts: number };
  listItems: PipelineListItem[];
};

function normalizeString(value: string): string {
  return value.trim().toLowerCase();
}

self.onmessage = (event: MessageEvent<PipelineListWorkerPayload>) => {
  const { requestId, pipelines, search } = event.data;
  const items = Array.isArray(pipelines) ? pipelines : [];
  const summary = {
    total: items.length,
    live: items.filter((pipeline) => pipeline.status === 'live').length,
    degraded: items.filter((pipeline) => pipeline.status === 'degraded').length,
    drafts: items.filter((pipeline) => pipeline.status === 'draft').length
  };
  const term = normalizeString(search ?? '');
  const filtered = term.length
    ? items.filter((pipeline) => `${pipeline.name} ${pipeline.alias}`.toLowerCase().includes(term))
    : items.slice();
  const listItems: PipelineListItem[] = filtered.map((pipeline) => ({
    id: pipeline.id,
    name: pipeline.name,
    alias: pipeline.alias,
    appearance: pipeline.appearance ?? null,
    status: pipeline.status,
    statusLabel: formatStatus(pipeline.status),
    statusClass: statusBadgeClass(pipeline.status),
    attachments: pipeline.attachments,
    issueCount: pipeline.issueCount,
    revision: pipeline.revision ?? null
  }));
  const response: PipelineListWorkerResponse = { requestId, summary, listItems };
  self.postMessage(response);
};
