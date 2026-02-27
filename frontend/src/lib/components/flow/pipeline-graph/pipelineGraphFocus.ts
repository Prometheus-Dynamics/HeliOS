import type { Node } from '@xyflow/svelte';
import type { Viewport } from '@xyflow/system';
import type { PipelineGraphPlan } from '$lib/types/pipeline';
import { findNodeEntryForId } from './editorUtils';

export type FocusRequest = { nodeId: string; port: string | null };

export type FocusRequestDeps = {
  plan: PipelineGraphPlan;
  nodes: Node[];
  flowApi: { getZoom?: () => number; setCenter: (x: number, y: number, options: { zoom: number; duration: number }) => void } | null;
  flowViewport: Viewport | null;
  notifySelection: (nodeId: string | null, edge: unknown, nodes?: string[]) => void;
  applyFocusHighlight: (nodeId: string | null, port: string | null) => void;
  setPendingFocusRequest: (request: FocusRequest | null) => void;
};

export function applyFocusRequest(request: FocusRequest, deps: FocusRequestDeps): void {
  const match = findNodeEntryForId(deps.plan, request.nodeId);
  if (!match) return;
  const nodeId = match.key;
  const port = request.port ?? null;
  deps.notifySelection(nodeId, null);
  deps.applyFocusHighlight(nodeId, port);
  const flowNode = deps.nodes.find((node) => node.id === nodeId);
  const nodePosition = flowNode?.position;
  if (!nodePosition || !deps.flowApi) {
    deps.setPendingFocusRequest(request);
    return;
  }
  const measuredWidth = flowNode?.measured?.width ?? flowNode?.width;
  const measuredHeight = flowNode?.measured?.height ?? flowNode?.height;
  const width = typeof measuredWidth === 'number' && Number.isFinite(measuredWidth) ? measuredWidth : 260;
  const height = typeof measuredHeight === 'number' && Number.isFinite(measuredHeight) ? measuredHeight : 160;
  const centerX = nodePosition.x + width / 2;
  const centerY = nodePosition.y + height / 2;
  const currentZoom = deps.flowApi.getZoom?.() ?? deps.flowViewport?.zoom ?? 1;
  const targetZoom = Math.min(Math.max(currentZoom, 0.85), 1.35);
  deps.setPendingFocusRequest(null);
  void deps.flowApi.setCenter(centerX, centerY, { zoom: targetZoom, duration: 300 });
}

export function focusOnGraphCenter(options: {
  nodes: Node[];
  flowApi: { getZoom?: () => number; setCenter: (x: number, y: number, options: { zoom: number; duration: number }) => void } | null;
  flowViewport: Viewport | null;
}): void {
  if (!options.nodes.length || !options.flowApi?.setCenter) return;
  let minX = Number.POSITIVE_INFINITY;
  let minY = Number.POSITIVE_INFINITY;
  let maxX = Number.NEGATIVE_INFINITY;
  let maxY = Number.NEGATIVE_INFINITY;
  options.nodes.forEach((node) => {
    const pos = node.position ?? { x: 0, y: 0 };
    const width = node.measured?.width ?? node.width ?? 240;
    const height = node.measured?.height ?? node.height ?? 140;
    minX = Math.min(minX, pos.x);
    minY = Math.min(minY, pos.y);
    maxX = Math.max(maxX, pos.x + width);
    maxY = Math.max(maxY, pos.y + height);
  });
  if (!Number.isFinite(minX) || !Number.isFinite(maxX) || !Number.isFinite(minY) || !Number.isFinite(maxY)) return;
  const centerX = (minX + maxX) / 2;
  const centerY = (minY + maxY) / 2;
  const currentZoom = options.flowApi.getZoom?.() ?? options.flowViewport?.zoom ?? 1;
  const targetZoom = Math.min(Math.max(currentZoom, 0.7), 1.2);
  void options.flowApi.setCenter(centerX, centerY, { zoom: targetZoom, duration: 300 });
}
