// A pipeline PUT replaces the spec, so every change sends the pipeline's whole spec with its own
// field changed. These check that no setting of the pipeline is left out.

import { describe, expect, test } from "bun:test";
import { specOf } from "#lib/api/spec.js";
import type { Pipeline } from "#lib/api/types.js";

function pipeline(overrides: Partial<Pipeline> = {}): Pipeline {
  return {
    id: "live",
    workload_id: "pipeline.live",
    name: "Live map",
    node_id: "node-local",
    revision: 3,
    enabled: true,
    state: "running",
    graph: { format: "daedalus.graph", schema_version: 1, requires: [], graph: { nodes: [], edges: [] } } as unknown as Pipeline["graph"],
    graph_error: null,
    bindings: { frame: { resource_id: "camera.front" } },
    plugins: [],
    session: null,
    telemetry: null,
    outputs: [],
    search_mode: "tracked",
    full_search_every: 8,
    loss_full_search_after: 2,
    margin_growth_misses: 6,
    field_layout: "frc-2026",
    dictionary: "apriltag_36h11",
    cpu: 3,
    nice: 5,
    map: "map-abc",
    pose: null,
    managed: true,
    status: { stage: "running" } as unknown as Pipeline["status"],
    ...overrides,
  } as Pipeline;
}

describe("specOf", () => {
  test("carries every setting the pipeline has", () => {
    const spec = specOf(pipeline());
    expect(spec).toEqual({
      name: "Live map",
      graph: pipeline().graph as never,
      bindings: { frame: { resource_id: "camera.front" } },
      enabled: true,
      search_mode: "tracked",
      full_search_every: 8,
      loss_full_search_after: 2,
      margin_growth_misses: 6,
      field_layout: "frc-2026",
      dictionary: "apriltag_36h11",
      cpu: 3,
      nice: 5,
      map: "map-abc",
    });
  });

  test("a change of one field keeps all the others", () => {
    const changed = { ...specOf(pipeline()), cpu: undefined };
    expect(changed.dictionary).toBe("apriltag_36h11");
    expect(changed.search_mode).toBe("tracked");
    expect(changed.map).toBe("map-abc");
    expect(changed.nice).toBe(5);
  });

  test("an unset setting is absent, and no map is an explicit null", () => {
    const spec = specOf(pipeline({ search_mode: null, cpu: null, nice: null, dictionary: null, field_layout: null, map: null }));
    expect("search_mode" in spec && spec.search_mode).toBeFalsy();
    expect(spec.cpu).toBeUndefined();
    expect(spec.nice).toBeUndefined();
    expect(spec.dictionary).toBeUndefined();
    expect(spec.map).toBeNull();
    expect(JSON.parse(JSON.stringify(spec))).not.toHaveProperty("cpu");
  });
});
