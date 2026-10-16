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
