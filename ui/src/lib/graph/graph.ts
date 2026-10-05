// Graph editor plumbing: GraphDocument <-> Svelte Flow nodes/edges, typed
// connection rules, category presentation and new-pipeline templates. Pure
// functions; the editor owns the reactive state.

import type { Connection, Edge, Node } from "@xyflow/svelte";
import { CATALOG_BY_ID, PORT_COLORS, defaults, type NodeType, type Param, type PortType } from "$lib/api/catalog";
import type { GraphDocument, GraphEdge, GraphNode } from "$lib/api/model";
import { tagGraph } from "$lib/api/mock";
import type { IconName } from "$lib/ui/icons";

export type ParamValue = number | string | boolean;

export interface PipelineNodeData extends Record<string, unknown> {
  type: string;
  label: string;
  params: Record<string, ParamValue>;
}

export type FlowNode = Node<PipelineNodeData, "pipeline">;
export type FlowEdge = Edge<{ portType: PortType }>;

/** dataTransfer type for dragging a catalog entry onto the canvas. */
export const DRAG_MIME = "application/x-helios-node";

export const CATEGORY_ORDER: NodeType["category"][] = ["Source", "Fiducials", "Filter", "Mask", "Geometry", "Color", "Motion", "Output"];

// Category hints reuse the port palette (same family of data) or status tokens.
export const CATEGORY_COLORS: Record<NodeType["category"], string> = {
  Source: PORT_COLORS["styx:framelease"],
  Fiducials: PORT_COLORS["eidos:detections"],
  Filter: PORT_COLORS["eidos:image"],
  Mask: PORT_COLORS["eidos:mask"],
  Geometry: PORT_COLORS["eidos:refined_corners"],
  Color: PORT_COLORS["eidos:quads"],
  Motion: "var(--warn)",
  Output: PORT_COLORS["eidos:tag_poses"],
};

export const CATEGORY_ICONS: Record<NodeType["category"], IconName> = {
  Source: "camera",
  Fiducials: "target",
  Filter: "adjustments-horizontal",
  Mask: "contrast-2",
  Geometry: "arrows-maximize",
  Color: "sun",
  Motion: "activity",
  Output: "broadcast",
};

export function portColor(type: PortType | undefined): string {
  return type ? PORT_COLORS[type] : PORT_COLORS.any;
}

/** Short display name of a port type: "styx:framelease" -> "framelease". */
export function portTypeLabel(type: PortType): string {
  return type === "any" ? "any" : type.split(":")[1];
}

// --- Conversion ------------------------------------------------------------

function edgeStyle(type: PortType | undefined): string {
  return `--edge-color: ${portColor(type)};`;
}

export function outputType(nodes: FlowNode[], nodeId: string, port: string | null | undefined): PortType | undefined {
  const n = nodes.find((x) => x.id === nodeId);
  return n ? CATALOG_BY_ID[n.data.type]?.outputs.find((p) => p.name === port)?.type : undefined;
}

export function inputType(nodes: FlowNode[], nodeId: string, port: string | null | undefined): PortType | undefined {
  const n = nodes.find((x) => x.id === nodeId);
  return n ? CATALOG_BY_ID[n.data.type]?.inputs.find((p) => p.name === port)?.type : undefined;
}

export function makeEdge(nodes: FlowNode[], id: string, source: string, sourceHandle: string, target: string, targetHandle: string): FlowEdge {
  const portType = outputType(nodes, source, sourceHandle) ?? "any";
  return { id, source, sourceHandle, target, targetHandle, data: { portType }, style: edgeStyle(portType), class: "typed-edge" };
}

export function toFlow(graph: GraphDocument): { nodes: FlowNode[]; edges: FlowEdge[] } {
  const nodes: FlowNode[] = graph.nodes.map((n) => ({
    id: n.id,
    type: "pipeline",
    position: { ...n.position },
    data: { type: n.type, label: n.label, params: { ...n.params } },
  }));
  const edges = graph.edges.map((e) => makeEdge(nodes, e.id, e.from.node, e.from.port, e.to.node, e.to.port));
  return { nodes, edges };
}

/** Plugins a graph needs, in catalog order of first use. */
export function requiresFor(nodes: { type: string }[]): { id: string }[] {
  const seen: string[] = [];
  for (const n of nodes) {
    const plugin = CATALOG_BY_ID[n.type]?.plugin;
    if (plugin && !seen.includes(plugin)) seen.push(plugin);
  }
  return seen.map((id) => ({ id }));
}

export function toGraphDocument(nodes: FlowNode[], edges: FlowEdge[]): GraphDocument {
  const graphNodes: GraphNode[] = nodes.map((n) => ({
    id: n.id,
    type: n.data.type,
    label: n.data.label,
    params: { ...n.data.params },
    position: { x: Math.round(n.position.x), y: Math.round(n.position.y) },
  }));
  const graphEdges: GraphEdge[] = edges.map((e) => ({
    id: e.id,
    from: { node: e.source, port: e.sourceHandle ?? "" },
    to: { node: e.target, port: e.targetHandle ?? "" },
  }));
  return { format: "daedalus.graph", schema_version: 1, requires: requiresFor(graphNodes), nodes: graphNodes, edges: graphEdges };
}

/** Comparable fingerprint (requires is derived, so it is left out). */
export function fingerprint(graph: GraphDocument): string {
  return JSON.stringify({
    nodes: graph.nodes.map((n) => [n.id, n.type, n.label, n.params, Math.round(n.position.x), Math.round(n.position.y)]),
    edges: graph.edges.map((e) => [e.id, e.from.node, e.from.port, e.to.node, e.to.port]),
  });
}

// --- Editing helpers -------------------------------------------------------

export function newNode(type: NodeType, existing: { id: string }[], position: { x: number; y: number }): FlowNode {
  const base = type.id.split(/[:.]/).at(-1)!.replace(/[^a-z0-9_]/gi, "_");
  let id = base;
  for (let i = 2; existing.some((n) => n.id === id); i++) id = `${base}_${i}`;
  return { id, type: "pipeline", position, data: { type: type.id, label: type.title, params: defaults(type) }, selected: true };
}

export function edgeId(): string {
  return `e-${Math.random().toString(36).slice(2, 9)}`;
}

export type ConnectionVerdict = { ok: true; type: PortType } | { ok: false; reason: string };

/** Same type, or a target that accepts "any"; one edge per input; no self loops. */
export function checkConnection(nodes: FlowNode[], edges: FlowEdge[], c: Connection | FlowEdge): ConnectionVerdict {
  if (c.source === c.target) return { ok: false, reason: "A node cannot feed itself" };
  const from = outputType(nodes, c.source, c.sourceHandle);
  const to = inputType(nodes, c.target, c.targetHandle);
  if (!from || !to) return { ok: false, reason: "Unknown port" };
  if (to !== "any" && from !== to) return { ok: false, reason: `${portTypeLabel(from)} cannot connect to ${portTypeLabel(to)}` };
  if (edges.some((e) => e.target === c.target && e.targetHandle === c.targetHandle)) return { ok: false, reason: "That input is already connected" };
  return { ok: true, type: from };
}

/** The params worth showing on the node face: changed ones first, up to `max`. */
export function keyParams(type: NodeType | undefined, params: Record<string, ParamValue>, max = 3): { param: Param; value: ParamValue; changed: boolean }[] {
  if (!type) return [];
  const rows = type.params.map((p) => ({ param: p, value: params[p.name] ?? p.default, changed: (params[p.name] ?? p.default) !== p.default }));
  return [...rows.filter((r) => r.changed), ...rows.filter((r) => !r.changed)].slice(0, max);
}

export function formatParam(value: ParamValue): string {
  if (typeof value === "boolean") return value ? "on" : "off";
  if (typeof value === "number") return Number.isInteger(value) ? String(value) : String(+value.toFixed(4));
  return value;
}

/** Graphs replaced by deploys from this browser session, newest last, per
 *  workload: what "Restore previous revision" goes back to. */

// --- Templates -------------------------------------------------------------

export type TemplateId = "apriltag" | "aruco" | "empty";

export const TEMPLATES: { id: TemplateId; title: string; summary: string; icon: IconName }[] = [
  { id: "apriltag", title: "AprilTag 36h11", summary: "Runs mask, quads, decode, validate, refine and pose, then streams and publishes to NT4.", icon: "target" },
  { id: "aruco", title: "ArUco 4x4", summary: "The same chain decoding the aruco_4x4_50 dictionary.", icon: "tag" },
  { id: "empty", title: "Empty", summary: "Only a camera source. Add nodes from the catalog.", icon: "file-plus" },
];

export function templateGraph(id: TemplateId, table: string): GraphDocument {
  if (id === "apriltag") return tagGraph("apriltag_36h11", table);
  if (id === "aruco") return tagGraph("aruco_4x4_50", table);
  const camera = CATALOG_BY_ID["styx:camera"];
  return {
    format: "daedalus.graph",
    schema_version: 1,
    requires: [{ id: camera.plugin }],
    nodes: [{ id: "camera", type: camera.id, label: camera.title, params: defaults(camera), position: { x: 0, y: 120 } }],
    edges: [],
  };
}

/** NT4 table for a node, when the catalog offers one matching its name. */
export function tableFor(nodeId: string): string {
  const options = CATALOG_BY_ID["helios:nt4"].params.find((p) => p.kind === "enum")!;
  const opts = options.kind === "enum" ? options.options : [];
  return opts.find((o) => nodeId.endsWith(o.split("/").at(-1)!)) ?? opts[0];
}
