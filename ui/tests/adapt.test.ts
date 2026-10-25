import { test, expect } from "bun:test";
import { fromDaedalus, toDaedalus } from "../src/lib/api/adapt";
import doc from "../../backend/src/helios/engine/graphs/apriltag-36h11.graph.json";
const norm = (d: any) => { const c = structuredClone(d); delete c.metadata?.["helios.editor"]; for (const n of c.graph.nodes) delete n.metadata?.["helios.title"]; return c; };
test("round trip keeps the document", () => {
  const back = toDaedalus(fromDaedalus(doc as any));
  expect(norm(back)).toEqual(norm(doc));
});
test("editing the dictionary keeps it an enum", () => {
  const g = fromDaedalus(doc as any);
  const det = g.nodes.find((n) => n.params.dictionary !== undefined)!;
  expect(det.params.dictionary).toBe("apriltag_36h11");
  det.params.dictionary = "apriltag_16h5";
  const back: any = toDaedalus(g);
  const node = back.graph.nodes.find((n: any) => n.label === det.id);
  expect(node.const_inputs.find((c: any) => c[0] === "dictionary")[1]).toEqual({ type: "Enum", value: { name: "apriltag_16h5", value: null } });
  expect(node.const_inputs.find((c: any) => c[0] === "change_threshold")[1]).toEqual({ type: "Float", value: 24.0 });
});

test("the graph editor's round trip keeps the document too", async () => {
  const { toFlow, toGraphDocument } = await import("../src/lib/graph/graph");
  const g = fromDaedalus(doc as any);
  const f = toFlow(g);
  // An edit in the editor, as the inspector makes it.
  f.nodes = f.nodes.map((n) => (n.data.params.dictionary !== undefined ? { ...n, data: { ...n.data, params: { ...n.data.params, dictionary: "apriltag_16h5" } } } : n));
  const back: any = toDaedalus(toGraphDocument(f.nodes, f.edges, g));
  const original: any = norm(doc);
  const edited = norm(back);
  expect(edited.requires).toEqual(original.requires);
  expect(edited.metadata).toEqual(original.metadata);
  for (const [i, node] of edited.graph.nodes.entries()) {
    const was = original.graph.nodes[i];
    expect(node.id).toBe(was.id);
    expect(node.inputs).toEqual(was.inputs);
    // Every const input survives with its type (the list-valued known_tags, the enums, the floats).
    expect(node.const_inputs.map((c: any) => [c[0], c[1].type])).toEqual(was.const_inputs.map((c: any) => [c[0], c[1].type]));
  }
  const det = edited.graph.nodes.find((n: any) => n.const_inputs.some((c: any) => c[0] === "dictionary"));
  expect(det.const_inputs.find((c: any) => c[0] === "dictionary")[1]).toEqual({ type: "Enum", value: { name: "apriltag_16h5", value: null } });
});
