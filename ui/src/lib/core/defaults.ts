// The default pipelines: what setup offers a camera. All of a camera's Eidos work shares one
// core, so a camera runs one pipeline and the choices combine into one template: AprilTag
// localization, with the live 3D map and its dense depth on the same detector, or ArUco markers,
// or feature tracks, or nothing but video.

import type { GraphDocument } from "#lib/api/model.js";
import type { Capability, PipelineTemplate } from "#lib/api/types.js";

export type TagKind = "apriltag" | "aruco" | null;

/** What a camera should do. `map` and `depth` go with AprilTag; `features` with no tags. */
export interface Choice {
  tags: TagKind;
  map: boolean;
  depth: boolean;
  features: boolean;
}

export const NO_CHOICE: Choice = { tags: null, map: false, depth: false, features: false };

/** A template as setup sees it: the device's own, or the mock's stand-in. */
export interface DefaultTemplate {
  id: string;
  title: string;
  summary: string;
  capabilities: Capability[];
}

/** The device's default templates, as `GET /v1/pipelines/templates` describes them (the staged
 * variants are for profiling, not offered). The mock and an older device without capabilities
 * use the same list. */
export const BUILT_IN: DefaultTemplate[] = [
  { id: "apriltag-36h11", title: "AprilTag localization", summary: "Field tags give the robot's position on the field.", capabilities: ["tags", "tag_pose", "field_pose"] },
  { id: "apriltag-36h11-map", title: "Localization and 3D map", summary: "Field position from the tags and a live 3D map, on one detector.", capabilities: ["tags", "tag_pose", "field_pose", "map"] },
  { id: "apriltag-36h11-map-dense", title: "Localization, 3D map and depth", summary: "Field position, the live 3D map and a dense depth mesh, on one detector.", capabilities: ["tags", "tag_pose", "field_pose", "map", "depth"] },
  { id: "aruco-4x4_50", title: "ArUco markers", summary: "Small printed markers, for game pieces or practice targets.", capabilities: ["tags", "tag_pose"] },
  { id: "features-track", title: "Feature tracks", summary: "Tracks image features from frame to frame, without tags.", capabilities: ["features"] },
];

/** The default templates a device offers (its own descriptions when it gives them). */
export function defaultsFrom(templates: PipelineTemplate[] | undefined): DefaultTemplate[] {
  return BUILT_IN.map((builtIn) => {
    const own = templates?.find((t) => t.id === builtIn.id);
    return own?.capabilities ? { id: own.id, title: own.title ?? builtIn.title, summary: own.summary ?? builtIn.summary, capabilities: own.capabilities } : builtIn;
  }).filter((d) => !templates || templates.some((t) => t.id === d.id));
}

/** What a graph does, by its node types (the device's rule for templates). */
export function capabilitiesOf(graph: GraphDocument): Capability[] {
  const has = (prefix: string) => graph.nodes.some((n) => n.type.startsWith(prefix));
  const all: [Capability, boolean][] = [
    ["tags", has("eidos:detectors.") || has("eidos:aruco.decode")],
    ["tag_pose", has("eidos:aruco.pose")],
    ["field_pose", has("eidos:aruco.multi_tag_pose")],
    ["map", has("eidos:mapping.live_map")],
    ["depth", has("eidos:dense.map")],
    ["features", has("eidos:features.track")],
  ];
  return all.filter(([, present]) => present).map(([capability]) => capability);
}

/** The capabilities a choice asks for (empty: video only). */
export function wanted(choice: Choice): Capability[] {
  if (choice.tags === "apriltag") return ["tags", "tag_pose", "field_pose", ...(choice.map ? (["map"] as const) : []), ...(choice.map && choice.depth ? (["depth"] as const) : [])];
  if (choice.tags === "aruco") return ["tags", "tag_pose"];
  return choice.features ? ["features"] : [];
}

const same = (a: Capability[], b: Capability[]) => a.length === b.length && a.every((c) => b.includes(c));

/** The one template that does what `choice` asks; null for video only. */
export function templateFor(choice: Choice, offered: DefaultTemplate[]): DefaultTemplate | null {
  const caps = wanted(choice);
  return caps.length === 0 ? null : (offered.find((t) => same(t.capabilities, caps)) ?? null);
}

/** The choice a running graph amounts to (`dictionary` tells AprilTag from ArUco). */
export function choiceOf(graph: GraphDocument | undefined, dictionary: string): Choice {
  if (!graph) return NO_CHOICE;
  const caps = capabilitiesOf(graph);
  const tags: TagKind = caps.includes("tags") ? (dictionary.startsWith("aruco") ? "aruco" : "apriltag") : null;
  return { tags, map: caps.includes("map"), depth: caps.includes("depth"), features: !tags && caps.includes("features") };
}

/** The choice with the rules applied: map and depth need AprilTag, depth needs the map, feature
 * tracks need no tags. */
export function normalized(choice: Choice): Choice {
  const april = choice.tags === "apriltag";
  const map = april && choice.map;
  return { tags: choice.tags, map, depth: map && choice.depth, features: !choice.tags && choice.features };
}
