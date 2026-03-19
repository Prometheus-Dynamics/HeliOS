import type { PipelineOverviewPipeline, PipelinePagePayload } from '$lib/types/pipeline';

export function dedupePipelineOverview(pipelines: PipelineOverviewPipeline[]): PipelineOverviewPipeline[] {
  const seen = new Set<string>();
  const deduped: PipelineOverviewPipeline[] = [];
  for (const pipeline of pipelines) {
    const id = typeof pipeline?.id === 'string' ? pipeline.id.trim() : '';
    if (!id || seen.has(id)) continue;
    seen.add(id);
    deduped.push(pipeline);
  }
  return deduped;
}

export function summarizePipelineOverview(pipelines: PipelineOverviewPipeline[]): PipelinePagePayload['summary'] {
  let live = 0;
  let degraded = 0;
  let drafts = 0;
  for (const pipeline of pipelines) {
    if (pipeline.status === 'live') {
      live += 1;
    } else if (pipeline.status === 'degraded') {
      degraded += 1;
    } else {
      drafts += 1;
    }
  }
  return {
    total: pipelines.length,
    live,
    degraded,
    drafts
  };
}
