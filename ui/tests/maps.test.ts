import { afterEach, expect, test } from "bun:test";
import { api, mapDownloadUrl } from "../src/lib/api/client";
import type { MapEntry, MapList } from "../src/lib/api/types";
import { hasLiveMap, mapChoices, mapOrigin, mapsSummary, noMapToSave } from "../src/lib/maps/mapping";
import { parseMapPose } from "../src/lib/vision/map";

const entry = (over: Partial<MapEntry> = {}): MapEntry => ({
  id: "map-a1",
  name: "Practice field",
  saved_at_ms: 1_700_000_000_000,
  size_bytes: 1_310_720,
  pipeline: "live-map-front",
  camera: "camera.raze-front.cam0",
  format_version: 1,
  created_ns: 1_700_000_000_000_000_000,
  creator: "eidos-mapping",
  ...over,
});

test("a graph offers saved maps when it runs the live mapper", () => {
  const withMapper = { nodes: [{ type: "styx:camera" }, { type: "eidos:mapping.live_map" }] };
  const withoutMapper = { nodes: [{ type: "styx:camera" }, { type: "eidos:dense.map" }] };
  expect(hasLiveMap(withMapper)).toBe(true);
  expect(hasLiveMap(withoutMapper)).toBe(false);
  expect(hasLiveMap(undefined)).toBe(false);
});

test("the map choices start with no map, then the stored maps in their order", () => {
  const choices = mapChoices([entry({ id: "map-b", name: "Newer" }), entry({ id: "map-a", name: "Older" })], null);
  expect(choices).toEqual([
    { value: "", label: "No map" },
    { value: "map-b", label: "Newer" },
    { value: "map-a", label: "Older" },
  ]);
});

test("a map the pipeline starts from but that is no longer stored stays choosable", () => {
  const choices = mapChoices([entry()], "map-gone");
  expect(choices.at(-1)).toEqual({ value: "map-gone", label: "map-gone (not stored)" });
  expect(mapChoices([entry()], "map-a1")).toHaveLength(2);
});

test("Save now waits for a map: none while the pipeline is stopped or the mapper is still calibrating", () => {
  const pose = (state: string) =>
    parseMapPose({ state, reason: "", valid: state === "tracking", calibration: { estimated: true, rms_px: 0.4, views: 9, frames: 90 }, alignment: { aligned: false, matched_tags: 0, residual_m: 0 } });
  expect(noMapToSave(true, null)).toContain("not reported");
  expect(noMapToSave(true, pose("bootstrapping"))).toContain("still calibrating");
  expect(noMapToSave(true, pose("tracking"))).toBeNull();
  expect(noMapToSave(true, pose("coasting"))).toBeNull();
  expect(noMapToSave(true, pose("lost"))).toBeNull();
  expect(noMapToSave(false, pose("tracking"))).toContain("Start the pipeline");
});

test("a map's origin names its pipeline and camera, or says it was uploaded", () => {
  const names = { pipeline: (id: string) => `pipeline ${id}`, camera: (id: string) => `camera ${id}` };
  expect(mapOrigin(entry(), names)).toBe("from pipeline live-map-front · camera camera.raze-front.cam0");
  expect(mapOrigin(entry({ camera: null }), names)).toBe("from pipeline live-map-front");
  expect(mapOrigin(entry({ pipeline: null, camera: null }), names)).toBe("uploaded");
});

test("the stored maps' line counts them and the store's size", () => {
  const list: MapList = { maps: [entry(), entry({ id: "map-b" })], total_bytes: 2 * 1_310_720, max_total_bytes: 128 * 1024 * 1024, max_map_bytes: 16 * 1024 * 1024 };
  expect(mapsSummary(list)).toBe("2 maps · 2.5 MiB of 128 MiB");
  expect(mapsSummary({ ...list, maps: [entry()], total_bytes: 0 })).toBe("1 map · 0 B of 128 MiB");
});

test("a map's download link is the device's file, or a peer's through the gateway", () => {
  expect(mapDownloadUrl("map-a1")).toBe("/v1/maps/map-a1");
  expect(mapDownloadUrl("map a/1")).toBe("/v1/maps/map%20a%2F1");
  expect(mapDownloadUrl("map-a1", "systemcore-1")).toBe("/v1/peers/systemcore-1/maps/map-a1");
});

const realFetch = globalThis.fetch;
afterEach(() => {
  globalThis.fetch = realFetch;
});

/** Stubs fetch with one JSON answer, and returns the requests it was asked. */
function answerWith(status: number, body: unknown) {
  const requests: { url: string; method: string; body: unknown }[] = [];
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    requests.push({ url: String(input), method: init?.method ?? "GET", body: init?.body ? JSON.parse(String(init.body)) : undefined });
    return new Response(body === undefined ? null : JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
  }) as typeof fetch;
  return requests;
}

test("the client lists the maps, and reads the list it gets", async () => {
  const list: MapList = { maps: [entry()], total_bytes: 1_310_720, max_total_bytes: 134_217_728, max_map_bytes: 16_777_216 };
  const requests = answerWith(200, list);
  expect(await api.maps()).toEqual(list);
  expect(requests).toEqual([{ url: "/v1/maps", method: "GET", body: undefined }]);
});

test("saving a pipeline's map posts its optional name, and returns the stored entry", async () => {
  const requests = answerWith(201, entry({ name: "Lab" }));
  const saved = await api.saveMap("live-map-front", "Lab");
  expect(saved.name).toBe("Lab");
  expect(requests).toEqual([{ url: "/v1/pipelines/live-map-front/map/save", method: "POST", body: { name: "Lab" } }]);
  answerWith(201, entry());
  const unnamed = answerWith(201, entry());
  await api.saveMap("live-map-front");
  expect(unnamed[0]?.body).toEqual({});
});

test("a refused delete carries the device's message", async () => {
  answerWith(409, { error: { code: "conflict", message: "pipelines live-map-front start from map map-a1; choose another map for them first" } });
  const error = await api.deleteMap("map-a1").catch((e: Error) => e);
  expect(error).toBeInstanceOf(Error);
  expect((error as Error).message).toContain("start from map map-a1");
});
