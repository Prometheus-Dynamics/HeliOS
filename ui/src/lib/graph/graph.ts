// Graph editor plumbing: GraphDocument <-> Svelte Flow nodes/edges, typed
// connection rules, category presentation and new-pipeline templates. Pure
// functions; the editor owns the reactive state.

import type { Connection, Edge, Node } from "@xyflow/svelte";
import { CATALOG_BY_ID, PORT_COLORS, UI_PLUGINS, defaults, type NodeType, type Param, type PortType } from "#lib/api/catalog.js";
import type { GraphDocument, GraphEdge, GraphNode } from "#lib/api/model.js";
import { tagGraph } from "#lib/api/mock.js";
import type { IconName } from "#lib/ui/icons.js";

export type ParamValue = number | string | boolean;

export interface PipelineNodeData extends Record<string, unknown> {
  type: string;
  label: string;
  params: Record<string, ParamValue>;
  /** The Daedalus node this came from (see `GraphNode.raw`): carried through the editor so a
   * deploy re-encodes only the edited parameters, in their original types. */
  raw?: unknown;
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

// Node geometry, shared by the node face and the auto-layout.
// Keep in step with the sizes in PipelineNode.svelte.
export const NODE_WIDTH = 216;
export const NODE_HEADER_HEIGHT = 44;
export const NODE_ROW_HEIGHT = 22;
export const NODE_FOOTER_HEIGHT = 28;

export function portColor(type: PortType | undefined): string {
  if (!type) return PORT_COLORS.any;
  const known = (PORT_COLORS as Record<string, string>)[type];
  if (known) return known;
  // A type the UI has no colour for gets a stable one of its own.
  let hash = 0;
  for (const c of type) hash = (hash * 31 + c.charCodeAt(0)) | 0;
  return `hsl(${Math.abs(hash) % 360} 70% 68%)`;
}

/** Short display name of a port type: "styx:framelease" -> "framelease", a scalar -> "float". */
export function portTypeLabel(type: PortType): string {
  if (type === "any") return "any";
  const scalar = /"Scalar":"(\w+)"/.exec(type)?.[1];
  if (scalar) return scalar.toLowerCase();
  if (type.startsWith("typeexpr:")) return type.includes('"Enum"') ? "enum" : "value";
  return type.split(":").at(-1) ?? type;
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
    data: { type: n.type, label: n.label, params: { ...n.params }, ...(n.raw !== undefined ? { raw: n.raw } : {}) },
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

/**
 * The editor's nodes and edges as a GraphDocument. `base` is the document the draft was loaded
 * from: its metadata, graph metadata and pinned plugin versions carry over, and each node's
 * Daedalus original rides along (`raw`), so deploying from the editor changes only what was edited.
 */
export function toGraphDocument(nodes: FlowNode[], edges: FlowEdge[], base?: GraphDocument): GraphDocument {
  const graphNodes: GraphNode[] = nodes.map((n) => ({
    id: n.id,
    type: n.data.type,
    label: n.data.label,
    params: { ...n.data.params },
    position: { x: Math.round(n.position.x), y: Math.round(n.position.y) },
    ...(n.data.raw !== undefined ? { raw: n.data.raw } : {}),
  }));
  const graphEdges: GraphEdge[] = edges.map((e) => ({
    id: e.id,
    from: { node: e.source, port: e.sourceHandle ?? "" },
    to: { node: e.target, port: e.targetHandle ?? "" },
  }));
  // The loaded document's requires as they were (a node the catalog does not list still needs
  // its plugin, and versions stay pinned), plus the plugins of nodes added since. The UI's own
  // pseudo-plugins (camera bindings, host outputs) are never a device requirement.
  const versions = new Map((base?.requires ?? []).map((r) => [r.id, r.version ?? null]));
  const added = requiresFor(graphNodes).map((r) => r.id).filter((id) => !versions.has(id) && !(base && UI_PLUGINS.has(id)));
  const ids = [...versions.keys(), ...added];
  const requires = ids.map((id) => (versions.get(id) ? { id, version: versions.get(id) } : { id }));
  return {
    format: "daedalus.graph",
    schema_version: 1,
    requires,
    nodes: graphNodes,
    edges: graphEdges,
    ...(base?.metadata ? { metadata: base.metadata } : {}),
    ...(base?.graphMetadata ? { graphMetadata: base.graphMetadata } : {}),
  };
}

/** Comparable fingerprint of what a deploy would change on the device: nodes, their parameters
 * and labels, and edges. Positions are editor metadata and ride along with the next deploy, so
 * moving or arranging nodes does not by itself ask for one (requires is derived, so left out). */
export function fingerprint(graph: GraphDocument): string {
  return JSON.stringify({
    nodes: graph.nodes.map((n) => [n.id, n.type, n.label, n.params]),
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

/** Parameters whose value differs from the catalog default. Shown as a count on the node; values live in the inspector. */
export function changedParams(type: NodeType | undefined, params: Record<string, ParamValue>): Param[] {
  if (!type) return [];
  return type.params.filter((p) => (params[p.name] ?? p.default) !== p.default);
}

/** Nodes whose label, type id or catalog title contains `query`, left to right then top to bottom. */
export function matchNodes(nodes: FlowNode[], query: string): FlowNode[] {
  const q = query.trim().toLowerCase();
  if (!q) return [];
  return nodes
    .filter((n) => {
      const t = CATALOG_BY_ID[n.data.type];
      return [n.data.label, n.data.type, t?.id, t?.title].some((s) => s?.toLowerCase().includes(q));
    })
    .sort((a, b) => a.position.x - b.position.x || a.position.y - b.position.y);
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
