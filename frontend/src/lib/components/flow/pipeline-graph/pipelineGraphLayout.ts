import type { Node } from '@xyflow/svelte';
import type { PipelineNodeDimensions, PipelineNodeLayout } from '$lib/types/pipeline';

export function buildLayoutSnapshot(nodes: Node[]): { hash: string; layout: PipelineNodeLayout } {
  const layoutEntries = nodes
    .map<[string, PipelineNodeDimensions]>((node) => {
      const measuredWidth = node.measured?.width;
      const measuredHeight = node.measured?.height;
      const width =
        typeof measuredWidth === 'number' && Number.isFinite(measuredWidth)
          ? measuredWidth
          : typeof node.width === 'number' && Number.isFinite(node.width)
            ? node.width
            : undefined;
      const height =
        typeof measuredHeight === 'number' && Number.isFinite(measuredHeight)
          ? measuredHeight
          : typeof node.height === 'number' && Number.isFinite(node.height)
            ? node.height
            : undefined;
      return [node.id, { width, height }];
    })
    .sort((a, b) => a[0].localeCompare(b[0]));

  const hash = layoutEntries.map(([id, dims]) => `${id}:${dims.width ?? -1},${dims.height ?? -1}`).join('|');
  const layout = Object.fromEntries(layoutEntries) as PipelineNodeLayout;
  return { hash, layout };
}
