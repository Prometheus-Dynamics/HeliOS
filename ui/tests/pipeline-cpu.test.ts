import { test, expect } from "bun:test";
import { toWorkload } from "../src/lib/api/adapt";
import { ENGINE_CPU_CORES } from "../src/lib/api/model";
import doc from "../../backend/src/helios/engine/graphs/apriltag-36h11.graph.json";

// A pipeline as helios-api returns it, with only the fields the adapter reads.
function pipeline(overrides: Record<string, unknown>): any {
  return {
    id: "tags-front",
    name: "AprilTags · front",
    node_id: "raze-front",
    revision: 4,
    enabled: true,
    state: "running",
    graph: doc,
    bindings: { camera: { resource_id: "camera.front" } },
    outputs: [],
    telemetry: { fps: 120, last_tick_ms: 0.4 },
    metrics: null,
    cpu: null,
    ...overrides,
  };
}

test("the engine's cores are the CM5's application cores", () => {
  expect([...ENGINE_CPU_CORES]).toEqual([1, 2, 3]);
});

test("a pinned pipeline shows the core the engine applied", () => {
  const w = toWorkload(pipeline({ cpu: 2, telemetry: { fps: 120, cpu: 2, cpu_error: null } }), "raze-front");
  expect(w.cpu).toBe(2);
  expect(w.cpuApplied).toBe(2);
  expect(w.cpuError).toBeNull();
});

test("a core the engine cannot use is reported, not applied", () => {
  const error = "core 0 is not one of the engine's cores (1, 2, 3)";
  const w = toWorkload(pipeline({ cpu: 0, telemetry: { fps: 0, cpu: null, cpu_error: error } }), "raze-front");
  expect(w.cpu).toBe(0);
  expect(w.cpuApplied).toBeNull();
  expect(w.cpuError).toBe(error);
});

test("a pipeline stored without a core is placed by the scheduler", () => {
  const w = toWorkload(pipeline({ cpu: null }), "raze-front");
  expect(w.cpu).toBeNull();
  expect(w.cpuApplied).toBeNull();
  expect(w.cpuError).toBeNull();
});
