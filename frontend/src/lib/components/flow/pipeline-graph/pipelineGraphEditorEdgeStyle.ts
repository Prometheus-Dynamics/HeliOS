import type {
  PipelineConnectionStyle,
  PipelineConnectionRoute,
  PipelineGraphPlan
} from '$lib/types/pipeline';
import type { EdgeSelection } from './types';
import { findConnectionForSelection } from './selectionHelpers';
import { DEFAULT_CONNECTION_STYLE, normalizeConnectionStyle } from './edgeStyle';

export const EDGE_STYLE_OPTIONS: Array<{
  id: PipelineConnectionRoute;
  label: string;
  icon: string;
}> = [
  { id: 'bezier', label: 'Curve', icon: '∿' },
  { id: 'straight', label: 'Line', icon: '—' },
  { id: 'step', label: 'Step', icon: '┐' },
  { id: 'teleport', label: 'Portal', icon: '⟳' }
];

export const buildSelectedEdgeStyle = (
  selectedEdge: EdgeSelection | null,
  plan: PipelineGraphPlan
): PipelineConnectionStyle => {
  if (!selectedEdge) {
    return normalizeConnectionStyle(DEFAULT_CONNECTION_STYLE);
  }
  const connection = findConnectionForSelection(selectedEdge, plan);
  return normalizeConnectionStyle(connection?.style ?? DEFAULT_CONNECTION_STYLE);
};

export const applyEdgeStyleSelection = (
  selectedEdge: EdgeSelection | null,
  plan: PipelineGraphPlan,
  style: PipelineConnectionStyle,
  applyConnectionStyle: (
    selection: EdgeSelection,
    nextStyle: PipelineConnectionStyle,
    options?: { commit?: boolean }
  ) => void
) => {
  if (!selectedEdge) return;
  const connection = findConnectionForSelection(selectedEdge, plan);
  if (!connection) return;
  const base = connection.style ? { ...connection.style } : { ...DEFAULT_CONNECTION_STYLE };
  const next: PipelineConnectionStyle = { ...base, ...style };
  if (next.route === 'bezier' && typeof next.curvature !== 'number') {
    next.curvature = 0.32;
  }
  applyConnectionStyle(selectedEdge, next, { commit: true });
};
