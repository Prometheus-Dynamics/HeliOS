import { expect, test } from "bun:test";
import { mockDevice } from "../src/lib/api/mock-device";
import type { MapEntry, MapList } from "../src/lib/api/types";

// The simulated device keeps its maps in localStorage, as it keeps its other settings: a stand-in here.
const stored = new Map<string, string>();
Object.defineProperty(globalThis, "localStorage", {
  configurable: true,
  value: {
    getItem: (key: string) => stored.get(key) ?? null,
    setItem: (key: string, value: string) => void stored.set(key, value),
    removeItem: (key: string) => void stored.delete(key),
  },
});

/** The value the simulated device answers a request with (throws as the device refuses it). */
async function call(method: string, path: string, body?: unknown): Promise<unknown> {
  return (await mockDevice(method, path, { body }))?.value;
}

test("the simulated device lists its saved maps, newest first, with the device's limits", async () => {
  const list = (await call("GET", "/v1/maps")) as MapList;
  expect(list.maps.length).toBeGreaterThan(0);
  const times = list.maps.map((m) => m.saved_at_ms);
  expect([...times].sort((a, b) => b - a)).toEqual(times);
  expect(list.total_bytes).toBe(list.maps.reduce((sum, m) => sum + m.size_bytes, 0));
  expect(list.max_total_bytes).toBe(128 * 1024 * 1024);
  expect(list.max_map_bytes).toBe(16 * 1024 * 1024);
});

test("a live-map pipeline saves its map under the name given, and one without the mapper does not", async () => {
  const saved = (await call("POST", "/v1/pipelines/live-map-front/map/save", { name: "Lab" })) as MapEntry;
  expect(saved.name).toBe("Lab");
  expect(saved.pipeline).toBe("live-map-front");
  const list = (await call("GET", "/v1/maps")) as MapList;
  expect(list.maps.some((m) => m.id === saved.id)).toBe(true);
  await expect(call("POST", "/v1/pipelines/tags-front/map/save", {})).rejects.toThrow("has no live map to save");
});

test("a map a pipeline starts from cannot be deleted until the pipeline chooses another", async () => {
  const saved = (await call("POST", "/v1/pipelines/live-map-front/map/save", {})) as MapEntry;
  await call("PUT", "/v1/pipelines/live-map-front", { name: "Live map", bindings: {}, map: saved.id });
  await expect(call("DELETE", `/v1/maps/${saved.id}`)).rejects.toThrow(`start from map ${saved.id}`);
  await call("PUT", "/v1/pipelines/live-map-front", { name: "Live map", bindings: {}, map: null });
  await call("DELETE", `/v1/maps/${saved.id}`);
  const list = (await call("GET", "/v1/maps")) as MapList;
  expect(list.maps.some((m) => m.id === saved.id)).toBe(false);
});

test("a pipeline cannot start from a map the device does not keep", async () => {
  await expect(call("PUT", "/v1/pipelines/live-map-front", { name: "Live map", bindings: {}, map: "map-missing" })).rejects.toThrow("no map map-missing");
});

/** A map file's bytes: the header as the device reads it (version 1 unless said), no map after it. */
function mapFile(creator = "eidos-mapping", version = 1): Blob {
  const header = new Uint8Array(22 + creator.length);
  header.set(new TextEncoder().encode("EIDMAP\0\0"), 0);
  const view = new DataView(header.buffer);
  view.setUint16(8, version, true);
  view.setUint16(10, 0, true);
  view.setBigUint64(12, 42n, true);
  view.setUint16(20, creator.length, true);
  header.set(new TextEncoder().encode(creator), 22);
  return new Blob([header]);
}

test("a map file is kept as a map when its header is a version 1 map, and refused with the reason when not", async () => {
  const kept = (await mockDevice("POST", "/v1/maps", { query: { name: "Lab upload" }, raw: mapFile() }))?.value as MapEntry;
  expect(kept.name).toBe("Lab upload");
  expect(kept.format_version).toBe(1);
  expect(kept.creator).toBe("eidos-mapping");
  expect(kept.pipeline).toBeNull();
  await expect(mockDevice("POST", "/v1/maps", { raw: new Blob([new TextEncoder().encode("PNG, not a map")]) })).rejects.toThrow("not a saved map");
  await expect(mockDevice("POST", "/v1/maps", { raw: mapFile("eidos-mapping", 2) })).rejects.toThrow("version 2");
});
