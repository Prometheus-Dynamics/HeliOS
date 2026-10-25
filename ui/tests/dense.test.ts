import { describe, expect, test } from "bun:test";
import { DEPTH_COLOURS, bufferOf, decodeBase64, depthRgba, encodeBase64, greyColours, meshBounds, parseDenseDepth, parseDenseMesh, parseDenseTiming } from "../src/lib/vision/dense";

/** Base64 of little-endian f32 values, as the device sends a buffer of positions or normals. */
const f32 = (values: number[]) => Buffer.from(new Float32Array(values).buffer).toString("base64");
/** Base64 of little-endian u32 values (triangle indices). */
const u32 = (values: number[]) => Buffer.from(new Uint32Array(values).buffer).toString("base64");
/** Base64 of raw bytes. */
const bytes = (values: number[]) => Buffer.from(Uint8Array.from(values)).toString("base64");

/** A minimal `dense_mesh` value; override the fields a test needs. */
function mesh(extra: Record<string, unknown> = {}) {
  return {
    seq: 3,
    frame: 42,
    t_ns: 1_000,
    keyframes: 5,
    submaps: 2,
    voxel_m: 0.05,
    vertices: 3,
    triangles: 1,
    positions: f32([0, 0, 0, 1, 0, 0, 0, 1, 0]),
    normals: f32([0, 0, 1, 0, 0, 1, 0, 0, 1]),
    grey: bytes([10, 128, 250]),
    indices: u32([0, 1, 2]),
    ...extra,
  };
}

describe("dense mesh", () => {
  test("decodes positions, normals, grey and indices from base64 little-endian buffers", () => {
    const view = parseDenseMesh(mesh());
    expect(view).not.toBeNull();
    expect(view!.seq).toBe(3);
    expect(view!.frame).toBe(42);
    expect(view!.vertices).toBe(3);
    expect(view!.triangles).toBe(1);
    expect(Array.from(view!.positions)).toEqual([0, 0, 0, 1, 0, 0, 0, 1, 0]);
    expect(Array.from(view!.normals!)).toEqual([0, 0, 1, 0, 0, 1, 0, 0, 1]);
    expect(Array.from(view!.grey!)).toEqual([10, 128, 250]);
    expect(Array.from(view!.indices)).toEqual([0, 1, 2]);
    expect(view!.droppedTriangles).toBe(0);
  });

  test("accepts a JSON array of byte values as well as base64", () => {
    const positions = Array.from(new Uint8Array(new Float32Array([2, 3, 4]).buffer));
    const view = parseDenseMesh(mesh({ positions, normals: null, grey: [9] }));
    expect(Array.from(view!.positions)).toEqual([2, 3, 4]);
    expect(Array.from(view!.grey!)).toEqual([9]);
  });

  test("a positions buffer of a partial vertex keeps the whole vertices only", () => {
    const short = Buffer.from(new Float32Array([0, 0, 0, 1, 0, 0, 9]).buffer).toString("base64"); // 28 bytes: 2 vertices and a stray float
    const view = parseDenseMesh(mesh({ positions: short, normals: null, grey: null, indices: u32([0, 1, 0]) }));
    expect(view!.vertices).toBe(2);
    expect(view!.positions.length).toBe(6);
    expect(Array.from(view!.positions)).toEqual([0, 0, 0, 1, 0, 0]);
  });

  test("normals of the wrong length are dropped (the scene derives them)", () => {
    const view = parseDenseMesh(mesh({ normals: f32([0, 0, 1]) }));
    expect(view!.normals).toBeNull();
    expect(view!.grey).not.toBeNull();
  });

  test("an empty grey is no grey, and a grey of the wrong length is dropped", () => {
    expect(parseDenseMesh(mesh({ grey: "" }))!.grey).toBeNull();
    expect(parseDenseMesh(mesh({ grey: bytes([1, 2]) }))!.grey).toBeNull();
    expect(parseDenseMesh(mesh({ grey: bytes([1, 2, 3, 4]) }))!.grey).toBeNull();
  });

  test("triangles past the vertices, or on a non-finite vertex, are dropped and counted", () => {
    const positions = f32([0, 0, 0, 1, 0, 0, Number.NaN, 1, 0]);
    const view = parseDenseMesh(
      mesh({
        positions,
        normals: null,
        grey: null,
        indices: u32([0, 1, 2, 0, 1, 7, 0, 1, 0]),
      }),
    );
    // Triangle 0 uses the NaN vertex (2); triangle 1 indexes past the vertices; triangle 2 is fine.
    expect(view!.droppedTriangles).toBe(2);
    expect(view!.triangles).toBe(1);
    expect(Array.from(view!.indices)).toEqual([0, 1, 0]);
  });

  test("an empty mesh is a valid, empty view; a value that is not a mesh is null", () => {
    const empty = parseDenseMesh(mesh({ vertices: 0, triangles: 0, positions: "", normals: "", grey: "", indices: "" }));
    expect(empty).not.toBeNull();
    expect(empty!.vertices).toBe(0);
    expect(empty!.indices.length).toBe(0);
    expect(meshBounds(empty!)).toBeNull();
    expect(parseDenseMesh(null)).toBeNull();
    expect(parseDenseMesh({ seq: 1 })).toBeNull();
  });

  test("the bounds are the min and max of the finite vertices", () => {
    const view = parseDenseMesh(mesh({ positions: f32([-1, 2, 0, 3, -4, 5, Number.NaN, 0, 0]), normals: null, grey: null, indices: "" }));
    expect(meshBounds(view!)).toEqual({ min: [-1, -4, 0], max: [3, 2, 5] });
  });

  test("the grey shading lifts each byte off black", () => {
    const colours = greyColours(Uint8Array.from([0, 255]));
    expect(colours.length).toBe(6);
    expect(colours[0]).toBeGreaterThan(0.2);
    expect(colours[3]).toBeGreaterThan(colours[0]);
    expect(colours[3]).toBeLessThanOrEqual(1);
  });
});

describe("dense depth", () => {
  test("keeps a depth of the sweep's pixel count and reads its frame", () => {
    const depth = parseDenseDepth({ frame: 9, width: 2, height: 2, inverse_depth: bytes([0, 64, 128, 255]) });
    expect(depth).not.toBeNull();
    expect(depth!.frame).toBe(9);
    expect(Array.from(depth!.inverseDepth)).toEqual([0, 64, 128, 255]);
  });

  test("a depth whose bytes do not match its size is not a depth", () => {
    expect(parseDenseDepth({ frame: 1, width: 2, height: 2, inverse_depth: bytes([1, 2, 3]) })).toBeNull();
    expect(parseDenseDepth({ frame: 1, width: 2, height: 2, inverse_depth: bytes([1, 2, 3, 4, 5]) })).toBeNull();
    expect(parseDenseDepth({ frame: 1, width: 0, height: 2, inverse_depth: "" })).toBeNull();
    expect(parseDenseDepth({ frame: 1, width: 2, height: 2 })).toBeNull();
  });

  test("unknown pixels are transparent, known ones opaque, and near is brighter than far", () => {
    const rgba = depthRgba({ width: 3, height: 1, inverseDepth: Uint8Array.from([0, 1, 255]) });
    expect(rgba.length).toBe(12);
    expect(rgba[3]).toBe(0); // unknown
    expect(rgba[7]).toBe(255); // far but known
    expect(rgba[11]).toBe(255); // near
    const luminance = (i: number) => 0.3 * rgba[i * 4] + 0.59 * rgba[i * 4 + 1] + 0.11 * rgba[i * 4 + 2];
    expect(luminance(2)).toBeGreaterThan(luminance(1));
  });

  test("the colour map is 256 entries, perceptual from dark to bright", () => {
    expect(DEPTH_COLOURS.length).toBe(256 * 3);
    // Far (1) is dark purple, near (255) is yellow: red and green both rise towards near.
    expect(DEPTH_COLOURS[255 * 3]).toBeGreaterThan(DEPTH_COLOURS[1 * 3]);
    expect(DEPTH_COLOURS[255 * 3 + 1]).toBeGreaterThan(DEPTH_COLOURS[1 * 3 + 1]);
  });
});

describe("dense timing and bytes", () => {
  test("the timing reads the mapper's state and work", () => {
    const timing = parseDenseTiming({ frame: 4, t_ns: 5, state: "running", processed: 1, keyframes: 2, dropped_keyframes: 0, pending: 3, keyframe_rate: 0.5, sweep_ms: 3, fill_ms: 1, integrate_ms: 2, mesh_ms: 0.5, budget_ms: 2, spacing: 16, cost_ms: 4, arrived: 6, dropped_blocks: 0, submaps: 1, blocks: 10, volume_bytes: 99 });
    expect(timing?.state).toBe("running");
    expect(timing?.pending).toBe(3);
    expect(timing?.volumeBytes).toBe(99);
    expect(parseDenseTiming({ frame: 1 })).toBeNull();
  });

  test("base64 round-trips, and a bad string is not bytes", () => {
    const data = Uint8Array.from([0, 1, 2, 254, 255]);
    expect(Array.from(decodeBase64(encodeBase64(data))!)).toEqual([0, 1, 2, 254, 255]);
    expect(decodeBase64("%%% not base64 %%%")).toBeNull();
    expect(bufferOf(42)).toBeNull();
  });
});
