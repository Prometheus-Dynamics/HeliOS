import type { DaedalusEdge, DaedalusValue } from '../daedalusTypes';
import { UI_EDGE_STYLE_KEY, UI_EDGE_STYLES_KEY } from '../daedalusTypes';
import { decodeMetadataString } from './normalizeNodes';

export const decodeEdgeStyle = (edge: DaedalusEdge): Record<string, unknown> | null => {
  const raw = decodeMetadataString(edge.metadata?.[UI_EDGE_STYLE_KEY]);
  if (!raw) return null;
  try {
    const parsed = JSON.parse(raw);
    return parsed && typeof parsed === 'object' ? (parsed as Record<string, unknown>) : null;
  } catch {
    return null;
  }
};

export const encodeEdgeStyle = (style: Record<string, unknown> | null | undefined): DaedalusValue | null => {
  if (!style || typeof style !== 'object') return null;
  return { type: 'String', value: JSON.stringify(style) };
};

export const decodeEdgeStyles = (
  metadata: Record<string, string> | null | undefined
): Record<string, unknown> | null => {
  const raw = metadata?.[UI_EDGE_STYLES_KEY];
  if (typeof raw !== 'string' || !raw.trim()) return null;
  try {
    const parsed = JSON.parse(raw);
    return parsed && typeof parsed === 'object' ? (parsed as Record<string, unknown>) : null;
  } catch {
    return null;
  }
};

export function decodeEdgeStyleForConnection(edge: DaedalusEdge, styles: Record<string, unknown> | null): Record<string, unknown> | null {
  const direct = decodeEdgeStyle(edge);
  if (direct) return direct;
  if (!styles) return null;
  const fromNode = edge?.from?.node;
  const toNode = edge?.to?.node;
  const fromPort = edge?.from?.port;
  const toPort = edge?.to?.port;
  if (typeof fromNode !== 'number' || typeof toNode !== 'number') return null;
  if (typeof fromPort !== 'string' || typeof toPort !== 'string') return null;
  const signature = `${String(fromNode)}:${fromPort}->${String(toNode)}:${toPort}`;
  const fallbackStyle = styles[signature];
  return fallbackStyle && typeof fallbackStyle === 'object' ? { ...(fallbackStyle as Record<string, unknown>) } : null;
}
