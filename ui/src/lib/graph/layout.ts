// Auto-layout for pipeline graphs: layers by longest path from the sources,
// ordered within a layer by the average position of their inputs (two sweeps
// of the barycentre heuristic), stacked by estimated node height.

import { CATALOG_BY_ID } from "#lib/api/catalog.js";
import { NODE_FOOTER_HEIGHT, NODE_HEADER_HEIGHT, NODE_ROW_HEIGHT, NODE_WIDTH, type FlowEdge, type FlowNode } from "./graph";

const COL = NODE_WIDTH + 72;
const GAP = 24;

/** Height of a node face: header, one row per port pair, footer. */
export function estimateHeight(n: FlowNode): number {
  const t = CATALOG_BY_ID[n.data.type];
  if (!t) return NODE_HEADER_HEIGHT + NODE_FOOTER_HEIGHT;
  const rows = Math.max(t.inputs.length, t.outputs.length);
  return NODE_HEADER_HEIGHT + (rows ? rows * NODE_ROW_HEIGHT + 10 : 0) + NODE_FOOTER_HEIGHT;
}

export function autoLayout(nodes: FlowNode[], edges: FlowEdge[]): FlowNode[] {
  const ids = nodes.map((n) => n.id);
  const preds = new Map(ids.map((id) => [id, [] as string[]]));
  for (const e of edges) if (preds.has(e.target) && preds.has(e.source)) preds.get(e.target)!.push(e.source);

  const layer = new Map<string, number>();
  const visiting = new Set<string>();
  const depth = (id: string): number => {
    if (layer.has(id)) return layer.get(id)!;
    if (visiting.has(id)) return 0; // cycle: break it
    visiting.add(id);
    const d = Math.max(-1, ...preds.get(id)!.map(depth)) + 1;
    visiting.delete(id);
    layer.set(id, d);
    return d;
  };
  ids.forEach(depth);

  const layers: string[][] = [];
  for (const id of ids) (layers[layer.get(id)!] ??= []).push(id);

  const order = new Map<string, number>();
  layers.forEach((l) => l.forEach((id, i) => order.set(id, i)));
  for (let sweep = 0; sweep < 2; sweep++) {
    for (const l of layers.slice(1)) {
      const bary = (id: string) => {
        const p = preds.get(id)!;
        return p.length ? p.reduce((s, x) => s + order.get(x)!, 0) / p.length : order.get(id)!;
      };
      l.sort((a, b) => bary(a) - bary(b));
      l.forEach((id, i) => order.set(id, i));
    }
  }

  const byId = new Map(nodes.map((n) => [n.id, n]));
  const heights = layers.map((l) => l.reduce((s, id) => s + estimateHeight(byId.get(id)!) + GAP, -GAP));
  const tallest = Math.max(...heights);
  const pos = new Map<string, { x: number; y: number }>();
  layers.forEach((l, li) => {
    let y = (tallest - heights[li]) / 2;
    for (const id of l) {
      pos.set(id, { x: li * COL, y: Math.round(y) });
      y += estimateHeight(byId.get(id)!) + GAP;
    }
  });
  return nodes.map((n) => ({ ...n, position: pos.get(n.id) ?? n.position }));
}
