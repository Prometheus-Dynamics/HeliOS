import { expect, test } from "bun:test";
import type { FlowEdge, FlowNode } from "#lib/graph/graph.js";
import { autoLayout } from "#lib/graph/layout.js";

// A device graph's host bridge takes what the nodes it feeds make, so the graph has a cycle.
const node = (id: string) => ({ id, type: "pipeline", position: { x: 0, y: 0 }, data: { type: "unknown:node", label: id, params: {} } }) as unknown as FlowNode;
const edge = (id: string, source: string, target: string) => ({ id, source, target, sourceHandle: "out", targetHandle: "in" }) as unknown as FlowEdge;

test("auto-arrange on a graph with a cycle gives every node finite coordinates", () => {
  const nodes = [node("host"), node("detector"), node("pose")];
  const edges = [edge("e1", "host", "detector"), edge("e2", "detector", "host"), edge("e3", "detector", "pose")];
  const laid = autoLayout(nodes, edges);
  expect(laid.map((n) => n.id).sort()).toEqual(["detector", "host", "pose"]);
  for (const n of laid) {
    expect(Number.isFinite(n.position.x)).toBe(true);
    expect(Number.isFinite(n.position.y)).toBe(true);
  }
});

test("auto-arrange puts a chain in columns, left to right", () => {
  const laid = autoLayout([node("a"), node("b"), node("c")], [edge("e1", "a", "b"), edge("e2", "b", "c")]);
  const x = Object.fromEntries(laid.map((n) => [n.id, n.position.x]));
  expect(x.a).toBeLessThan(x.b);
  expect(x.b).toBeLessThan(x.c);
});
