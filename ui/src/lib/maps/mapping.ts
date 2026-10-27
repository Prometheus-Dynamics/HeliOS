// Saved maps as pure data: when a pipeline offers them (its graph has the live mapper), the choices
// of the map a pipeline starts from, and the lines that describe the stored maps. Nothing here reads
// the device; the maps store and the panel do.

import { bytes } from "#lib/format.js";
import type { MapEntry, MapList } from "#lib/api/types.js";
import type { MapPose } from "#lib/vision/map.js";

/** The live mapper's node type, as a graph names it (`eidos:mapping.live_map`). */
export const LIVE_MAP_NODE = "eidos:mapping.live_map";

/** Whether a graph runs the live mapper, so its pipeline can start from a saved map. */
export function hasLiveMap(graph: { nodes: { type: string }[] } | undefined): boolean {
  return graph?.nodes.some((node) => node.type === LIVE_MAP_NODE) ?? false;
}

/**
 * The choices of the map a pipeline starts from: "No map" (the empty value) and the stored maps,
 * newest first. A map the pipeline names that is no longer stored stays listed, so the select shows it.
 */
export function mapChoices(maps: MapEntry[] | undefined, current: string | null): { value: string; label: string }[] {
  const choices = [{ value: "", label: "No map" }, ...(maps ?? []).map((map) => ({ value: map.id, label: map.name }))];
  if (current && !choices.some((choice) => choice.value === current)) choices.push({ value: current, label: `${current} (not stored)` });
  return choices;
}

/** Where a stored map came from: its pipeline and camera, as the names given; "uploaded" when none. */
export function mapOrigin(map: MapEntry, names: { pipeline: (id: string) => string; camera: (id: string) => string }): string {
  if (!map.pipeline) return "uploaded";
  const from = `from ${names.pipeline(map.pipeline)}`;
  return map.camera ? `${from} · ${names.camera(map.camera)}` : from;
}

/**
 * Why the live mapper has no map to save yet, as a line for the control that would save it (null when
 * it has one). A pipeline that is not running has none, and neither does a mapper still bootstrapping
 * (calibrating, no tags seen yet, the state the device reports as `bootstrapping`). The device refuses
 * a save then, after a wait; the control says so instead.
 */
export function noMapToSave(running: boolean, pose: MapPose | null): string | null {
  if (!running) return "Start the pipeline to save its map.";
  if (!pose) return "The live mapper has not reported yet. Save once it has a map.";
  if (pose.state === "bootstrapping") return "The live mapper is still calibrating, so there is no map to save yet.";
  return null;
}

/** The stored maps' line: how many, and how much of the store they take. */
export function mapsSummary(list: MapList): string {
  const count = list.maps.length;
  return `${count} ${count === 1 ? "map" : "maps"} · ${bytes(list.total_bytes)} of ${bytes(list.max_total_bytes)}`;
}
