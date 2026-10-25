// The node catalog: every node type a graph can use, with typed ports and
// parameters. The list below is what the UI knows by itself (mock mode, and the
// titles and help it shows for well-known nodes); on a device, `mergeDeviceCatalog`
// replaces it with what helios-engine's loaded plugins actually declare
// (`GET /v1/catalog`), so a new plugin node appears with no UI change.

import type { DeviceCatalog, DeviceCatalogNode, DeviceCatalogPort } from "./types.js";

export type KnownPortType =
  | "styx:framelease"
  | "eidos:mask"
  | "eidos:runs"
  | "eidos:quads"
  | "eidos:detections"
  | "eidos:rejected_markers"
  | "eidos:refined_corners"
  | "eidos:tag_poses"
  | "eidos:image"
  | "eidos:camera_calibration"
  | "eidos:camera_extrinsics"
  | "eidos:pose_solutions"
  | "eidos:camera_pose"
  | "eidos:feature_tracks"
  | "any";

/** A port's Daedalus type key; the known ones have their own colour. */
export type PortType = KnownPortType | (string & {});

export interface Port {
  name: string;
  type: PortType;
}

/** How a settings screen shows a parameter, when its node declares it: its name for people, its
 * unit, the group it belongs to within the node, and whether it is an expert setting. */
export interface ParamMeta {
  label?: string;
  unit?: string;
  group?: string;
  advanced?: boolean;
}

export type Param = (
  | { name: string; kind: "int"; default: number; min: number; max: number; help: string }
  | { name: string; kind: "float"; default: number; min: number; max: number; step: number; help: string }
  | { name: string; kind: "bool"; default: boolean; help: string }
  | { name: string; kind: "enum"; default: string; options: string[]; help: string }
) &
  ParamMeta;

export interface NodeType {
  id: string;
  plugin: string;
  category: "Source" | "Fiducials" | "Filter" | "Mask" | "Geometry" | "Color" | "Motion" | "Output";
  title: string;
  summary: string;
  inputs: Port[];
  outputs: Port[];
  params: Param[];
  /** False when the device's catalog does not list it: kept to draw old graphs, not offered. */
  available?: boolean;
  /** A node-group: its stages are nodes of their own. */
  group?: boolean;
  /** The node-groups it is a stage of (listed under them in the palette). */
  stageOf?: string[];
}

const dictionaries = [
  "apriltag_36h11",
  "apriltag_16h5",
  "apriltag_25h9",
  "apriltag_36h10",
  ...["4x4", "5x5", "6x6", "7x7"].flatMap((size) => [50, 100, 250, 1000].map((n) => `aruco_${size}_${n}`)),
];

const maskParams: Param[] = [
  { name: "radius", kind: "int", default: 3, min: 1, max: 15, help: "Box-mean window radius (window = 2r + 1)." },
  { name: "offset", kind: "int", default: 7, min: -255, max: 255, help: "Darker than the local mean by more than this is foreground." },
  { name: "invert", kind: "bool", default: true, help: "Markers are dark on light." },
  { name: "pyramid_level", kind: "int", default: 1, min: 0, max: 3, help: "0 = full frame; 1 = the ISP's half-size plane." },
  { name: "downscale_missing", kind: "bool", default: true, help: "Downscale on the CPU when the frame has no pyramid level." },
];

const quadParams: Param[] = [
  { name: "min_width", kind: "int", default: 4, min: 1, max: 400, help: "Smallest candidate width, mask pixels." },
  { name: "min_height", kind: "int", default: 4, min: 1, max: 400, help: "Smallest candidate height, mask pixels." },
  { name: "dp_fraction_x1000", kind: "int", default: 50, min: 1, max: 500, help: "Polygon approximation tolerance (per mille of perimeter)." },
];

export const CATALOG: NodeType[] = [
  {
    id: "styx:camera",
    plugin: "styx.frames",
    category: "Source",
    title: "Camera",
    summary: "Frames from a camera resource anywhere in the cluster.",
    inputs: [],
    outputs: [{ name: "frame", type: "styx:framelease" }],
    params: [
      { name: "pyramid", kind: "bool", default: true, help: "Ask the ISP for a half-size companion plane." },
      { name: "fps", kind: "int", default: 60, min: 1, max: 120, help: "Requested frame rate." },
    ],
  },
  {
    id: "eidos:aruco.mask_prep_runs",
    plugin: "eidos",
    category: "Fiducials",
    title: "Mask prep (runs)",
    summary: "Adaptive threshold straight to compact runs; no byte mask.",
    inputs: [{ name: "frame", type: "styx:framelease" }],
    outputs: [{ name: "runs", type: "eidos:runs" }],
    params: maskParams,
  },
  {
    id: "eidos:aruco.mask_prep",
    plugin: "eidos",
    category: "Fiducials",
    title: "Mask prep",
    summary: "Adaptive box-mean threshold into a byte mask.",
    inputs: [{ name: "frame", type: "styx:framelease" }],
    outputs: [{ name: "mask", type: "eidos:mask" }],
    params: maskParams,
  },
  {
    id: "eidos:aruco.quads_from_runs",
    plugin: "eidos",
    category: "Fiducials",
    title: "Quads (runs)",
    summary: "Candidate quadrilaterals from compact runs.",
    inputs: [{ name: "runs", type: "eidos:runs" }],
    outputs: [{ name: "quads", type: "eidos:quads" }],
    params: quadParams,
  },
  {
    id: "eidos:aruco.quads",
    plugin: "eidos",
    category: "Fiducials",
    title: "Quads",
    summary: "Candidate quadrilaterals traced on a mask.",
    inputs: [{ name: "mask", type: "eidos:mask" }],
    outputs: [{ name: "quads", type: "eidos:quads" }],
    params: quadParams,
  },
  {
    id: "eidos:aruco.decode",
    plugin: "eidos",
    category: "Fiducials",
    title: "Decode",
    summary: "Reads marker bits on the full frame.",
    inputs: [
      { name: "frame", type: "styx:framelease" },
      { name: "quads", type: "eidos:quads" },
    ],
    outputs: [{ name: "detections", type: "eidos:detections" }],
    params: [
      { name: "dictionary", kind: "enum", default: "apriltag_36h11", options: dictionaries, help: "Marker family." },
      { name: "sampler", kind: "enum", default: "mean3x3_otsu", options: ["point", "mean3x3", "mean3x3_otsu", "mean3x3_best_hamming"], help: "How bit cells are sampled." },
      { name: "max_correction_bits", kind: "int", default: -1, min: -1, max: 8, help: "-1 uses the dictionary default." },
      { name: "validate", kind: "bool", default: false, help: "Run validation inside decode (off when a Validate node follows)." },
    ],
  },
  {
    id: "eidos:aruco.validate",
    plugin: "eidos",
    category: "Fiducials",
    title: "Validate",
    summary: "OpenCV-style gates; rejected markers come out with a reason.",
    inputs: [
      { name: "frame", type: "styx:framelease" },
      { name: "detections", type: "eidos:detections" },
    ],
    outputs: [
      { name: "detections", type: "eidos:detections" },
      { name: "rejected", type: "eidos:rejected_markers" },
    ],
    params: [
      { name: "require_convex", kind: "bool", default: true, help: "Reject non-convex quads." },
      { name: "min_perimeter_permille", kind: "int", default: 30, min: 0, max: 1000, help: "Smallest perimeter, per mille of the frame's larger side." },
      { name: "min_corner_distance_permille", kind: "int", default: 50, min: 0, max: 1000, help: "Smallest corner spacing, per mille of the perimeter." },
      { name: "min_distance_to_border", kind: "int", default: 3, min: 0, max: 100, help: "Pixels from the frame edge." },
      { name: "error_correction_percent", kind: "int", default: 60, min: 0, max: 100, help: "Share of the dictionary's correction bits allowed." },
      { name: "max_border_error_percent", kind: "int", default: 35, min: 0, max: 100, help: "Largest share of bright border cells." },
      { name: "min_contrast", kind: "int", default: 20, min: 0, max: 255, help: "Bright vs dark cell difference." },
    ],
  },
  {
    id: "eidos:aruco.refine",
    plugin: "eidos",
    category: "Fiducials",
    title: "Refine corners",
    summary: "Moves corners onto the sub-pixel marker edges.",
    inputs: [
      { name: "frame", type: "styx:framelease" },
      { name: "detections", type: "eidos:detections" },
    ],
    outputs: [{ name: "refined_corners", type: "eidos:refined_corners" }],
    params: [
      { name: "search_rate", kind: "float", default: 0.06, min: 0.01, max: 0.3, step: 0.01, help: "First-pass search radius, share of side length." },
      { name: "min_radius", kind: "float", default: 3, min: 0.5, max: 20, step: 0.5, help: "Smallest search radius, px." },
      { name: "max_radius", kind: "float", default: 12, min: 1, max: 40, step: 0.5, help: "Largest search radius, px." },
      { name: "polish_radius", kind: "float", default: 1.5, min: 0.25, max: 5, step: 0.25, help: "Second-pass radius, px." },
    ],
  },
  {
    id: "io.host_bridge",
    plugin: "helios",
    category: "Source",
    title: "Camera and results",
    summary: "The bound camera's frames, calibration and mount in; the pipeline's results out to the API and NetworkTables.",
    inputs: [
      { name: "detections", type: "eidos:detections" },
      { name: "rejected", type: "eidos:detections" },
      { name: "refined_corners", type: "eidos:refined_corners" },
      { name: "poses", type: "eidos:tag_poses" },
      { name: "pose_solutions", type: "eidos:pose_solutions" },
      { name: "multi_tag_pose", type: "eidos:camera_pose" },
    ],
    outputs: [
      { name: "frame", type: "styx:framelease" },
      { name: "camera", type: "eidos:camera_calibration" },
      { name: "extrinsics", type: "eidos:camera_extrinsics" },
    ],
    params: [],
  },
  ...(["apriltag", "aruco"] as const).flatMap((family) =>
    (["", "_tracked"] as const).map(
      (mode): NodeType => ({
        id: `eidos:detectors.${family}${mode}`,
        plugin: "eidos",
        category: "Fiducials",
        title: `${family === "apriltag" ? "AprilTag" : "ArUco"} detector${mode ? "" : " (full search)"}`,
        summary: mode
          ? "Finds tags: a full search every few frames, windows around tracked tags in between. A node group: its stages show in the profiler."
          : "Finds tags with a full search on every frame. A node group: its stages show in the profiler.",
        inputs: [{ name: "frame", type: "styx:framelease" }],
        outputs: [
          { name: "detections", type: "eidos:detections" },
          { name: "refined_corners", type: "eidos:refined_corners" },
          { name: "rejected", type: "eidos:detections" },
        ],
        params: [
          { name: "dictionary", kind: "enum", default: family === "apriltag" ? "apriltag_36h11" : "aruco_4x4_50", options: dictionaries.filter((d) => d.startsWith(family)), help: "Tag family. A family other than the tags' own finds nothing." },
          ...(mode ? [{ name: "full_search_every", kind: "int", default: 8, min: 1, max: 100000, help: "Full search at least every this many frames; new tags appear within that many." } as Param] : []),
        ],
      }),
    ),
  ),
  {
    id: "eidos:aruco.multi_tag_pose",
    plugin: "eidos",
    category: "Fiducials",
    title: "Field pose",
    summary: "The camera's (and robot's) pose on the field from every visible tag with a known position.",
    inputs: [
      { name: "camera", type: "eidos:camera_calibration" },
      { name: "detections", type: "eidos:detections" },
      { name: "extrinsics", type: "eidos:camera_extrinsics" },
      { name: "refined_corners", type: "eidos:refined_corners" },
    ],
    outputs: [{ name: "camera_pose", type: "eidos:camera_pose" }],
    params: [],
  },
  {
    id: "eidos:aruco.pose",
    plugin: "eidos",
    category: "Fiducials",
    title: "Tag pose",
    summary: "Camera-frame pose of each tag from refined corners.",
    inputs: [
      { name: "camera", type: "eidos:camera_calibration" },
      { name: "detections", type: "eidos:detections" },
      { name: "refined_corners", type: "eidos:refined_corners" },
    ],
    outputs: [
      { name: "poses", type: "eidos:tag_poses" },
      { name: "solutions", type: "eidos:pose_solutions" },
    ],
    params: [
      { name: "tag_size_m", kind: "float", default: 0.1651, min: 0.01, max: 1, step: 0.0001, help: "Black-border edge length." },
      { name: "calibration", kind: "enum", default: "camera default", options: ["camera default", "none"], help: "Intrinsics from the camera resource." },
    ],
  },
  {
    id: "eidos:filter.gaussian_blur",
    plugin: "eidos",
    category: "Filter",
    title: "Gaussian blur",
    summary: "Separable Gaussian.",
    inputs: [{ name: "image", type: "styx:framelease" }],
    outputs: [{ name: "image", type: "eidos:image" }],
    params: [{ name: "sigma", kind: "float", default: 1, min: 0.1, max: 10, step: 0.1, help: "Standard deviation, px." }],
  },
  {
    id: "eidos:filter.clahe",
    plugin: "eidos",
    category: "Filter",
    title: "CLAHE",
    summary: "Contrast-limited adaptive histogram equalisation.",
    inputs: [{ name: "image", type: "styx:framelease" }],
    outputs: [{ name: "image", type: "eidos:image" }],
    params: [
      { name: "clip_limit", kind: "float", default: 2, min: 0.5, max: 10, step: 0.5, help: "Contrast limit." },
      { name: "tiles", kind: "int", default: 8, min: 2, max: 32, help: "Tiles per side." },
    ],
  },
  {
    id: "eidos:mask.otsu",
    plugin: "eidos",
    category: "Mask",
    title: "Otsu threshold",
    summary: "Global threshold chosen by Otsu's method.",
    inputs: [{ name: "image", type: "eidos:image" }],
    outputs: [{ name: "mask", type: "eidos:mask" }],
    params: [{ name: "invert", kind: "bool", default: false, help: "Dark is foreground." }],
  },
  {
    id: "eidos:mask.morphology",
    plugin: "eidos",
    category: "Mask",
    title: "Morphology",
    summary: "Erode, dilate, open or close.",
    inputs: [{ name: "mask", type: "eidos:mask" }],
    outputs: [{ name: "mask", type: "eidos:mask" }],
    params: [
      { name: "operation", kind: "enum", default: "open", options: ["erode", "dilate", "open", "close"], help: "Operation." },
      { name: "radius", kind: "int", default: 1, min: 1, max: 15, help: "Structuring element radius." },
    ],
  },
  {
    id: "eidos:geometry.resize",
    plugin: "eidos",
    category: "Geometry",
    title: "Resize",
    summary: "Area, bilinear or nearest resize.",
    inputs: [{ name: "image", type: "styx:framelease" }],
    outputs: [{ name: "image", type: "eidos:image" }],
    params: [
      { name: "scale", kind: "float", default: 0.5, min: 0.1, max: 2, step: 0.05, help: "Scale factor." },
      { name: "interpolation", kind: "enum", default: "area", options: ["area", "bilinear", "nearest"], help: "Method." },
    ],
  },
  {
    id: "eidos:motion.frame_difference",
    plugin: "eidos",
    category: "Motion",
    title: "Frame difference",
    summary: "Absolute difference to the previous frame.",
    inputs: [{ name: "image", type: "styx:framelease" }],
    outputs: [{ name: "image", type: "eidos:image" }],
    params: [],
  },
  {
    id: "eidos:features.track",
    plugin: "eidos",
    category: "Motion",
    title: "Feature tracks",
    summary: "Corners followed across frames with stable ids; empty grid cells are refilled.",
    inputs: [
      { name: "frame", type: "styx:framelease" },
      { name: "mask", type: "eidos:mask" },
    ],
    outputs: [{ name: "tracks", type: "eidos:feature_tracks" }],
    params: [
      { name: "detector", kind: "enum", default: "shi_tomasi", options: ["shi_tomasi", "harris", "fast"], help: "Corner detector for new tracks." },
      { name: "max_points", kind: "int", default: 200, min: 1, max: 2000, help: "Most live tracks." },
      { name: "min_distance", kind: "float", default: 8, min: 0, max: 64, step: 0.5, help: "Smallest spacing between corners, in pixels." },
      { name: "cell_size", kind: "int", default: 32, min: 4, max: 256, help: "Grid cell size, in pixels." },
      { name: "max_per_cell", kind: "int", default: 2, min: 1, max: 16, help: "Most tracks per grid cell." },
      { name: "window_radius", kind: "int", default: 7, min: 1, max: 31, help: "Tracker window half-size, in pixels." },
      { name: "pyramid_levels", kind: "int", default: 3, min: 0, max: 6, help: "Tracker pyramid levels." },
    ],
  },
  {
    id: "eidos:detectors.tracked_detect",
    plugin: "eidos",
    category: "Fiducials",
    title: "AprilTag detector (tracked, one node)",
    summary: "The tracked AprilTag detector as one node: a full search every few frames, windows around tracked tags in between.",
    inputs: [{ name: "frame", type: "styx:framelease" }],
    outputs: [
      { name: "detections", type: "eidos:detections" },
      { name: "rejected", type: "eidos:detections" },
      { name: "refined_corners", type: "eidos:refined_corners" },
    ],
    params: [
      { name: "dictionary", kind: "enum", default: "apriltag_36h11", options: dictionaries.filter((d) => d.startsWith("apriltag")), help: "Tag family. A family other than the tags' own finds nothing." },
      { name: "full_search_every", kind: "int", default: 8, min: 1, max: 100000, help: "Full search at least every this many frames." },
    ],
  },
  {
    id: "eidos:mapping.live_map",
    plugin: "eidos",
    category: "Motion",
    title: "Live map",
    summary: "Live mapper: camera pose and a growing map from tags and features, with per-tick deltas and rate-limited snapshots.",
    inputs: [
      { name: "frame", type: "styx:framelease" },
      { name: "calibration", type: "eidos:camera_calibration" },
      { name: "detections", type: "eidos:detections" },
      { name: "rejected", type: "eidos:detections" },
      { name: "refined_corners", type: "eidos:refined_corners" },
      { name: "extrinsics", type: "eidos:camera_extrinsics" },
      { name: "layout", type: "eidos:known_tag_poses" },
    ],
    outputs: [
      { name: "pose", type: "eidos:map_pose" },
      { name: "delta", type: "eidos:map_delta" },
      { name: "snapshot", type: "eidos:map_snapshot" },
      { name: "timing", type: "eidos:map_timing" },
    ],
    params: [
      { name: "tag_size_m", kind: "float", default: 0.1651, min: 0.001, max: 10, step: 0.0001, help: "Side of the tags, in metres (the outer black border)." },
      { name: "snapshot_hz", kind: "float", default: 2, min: 0, max: 1000, step: 0.5, help: "Snapshots per second at most, on the frame timestamps (0 for none)." },
      { name: "calibration_frames", kind: "int", default: 150, min: 1, max: 100000, help: "Frames fed to the self-calibration when no calibration input is connected." },
      { name: "lk_levels", kind: "int", default: 4, min: 1, max: 8, help: "Pyramid levels of the Lucas-Kanade tracker." },
    ],
  },
  {
    id: "eidos:dense.map",
    plugin: "eidos",
    category: "Motion",
    title: "Dense map",
    summary: "Dense mapper: keyframe depth fused into a TSDF and a mesh of the surroundings, on the live map's pose.",
    inputs: [
      { name: "frame", type: "styx:framelease" },
      { name: "pose", type: "eidos:map_pose" },
      { name: "calibration", type: "eidos:camera_calibration" },
    ],
    outputs: [
      { name: "mesh", type: "eidos:dense_mesh" },
      { name: "timing", type: "eidos:dense_timing" },
    ],
    params: [
      { name: "budget_ms", kind: "float", default: 2, min: 0, max: 100, step: 0.1, help: "Time per tick the dense mapper may take (0: whole keyframes each tick)." },
      { name: "key_every", kind: "int", default: 8, min: 1, max: 1000, help: "A keyframe every this many posed frames." },
      { name: "voxel_m", kind: "float", default: 0.04, min: 0.005, max: 1, step: 0.005, help: "Side of a TSDF voxel, in metres." },
    ],
  },
  {
    id: "helios:stream",
    plugin: "helios",
    category: "Output",
    title: "Stream",
    summary: "Publishes an output as a cluster stream resource.",
    inputs: [{ name: "value", type: "any" }],
    outputs: [],
    params: [
      { name: "name", kind: "enum", default: "detections", options: ["detections", "poses", "rejected", "tracks", "telemetry"], help: "Stream name." },
      { name: "max_rate_hz", kind: "int", default: 60, min: 1, max: 240, help: "Publish at most this often." },
    ],
  },
  {
    id: "helios:nt4",
    plugin: "helios",
    category: "Output",
    title: "NetworkTables",
    summary: "Bridges a stream to robot code over NetworkTables 4.",
    inputs: [{ name: "value", type: "any" }],
    outputs: [],
    params: [{ name: "table", kind: "enum", default: "/helios/front", options: ["/helios/front", "/helios/back", "/helios/left", "/helios/right"], help: "Table path." }],
  },
];

export const CATALOG_BY_ID: Record<string, NodeType> = Object.fromEntries(CATALOG.map((n) => [n.id, n]));

export const PORT_COLORS: Record<KnownPortType, string> = {
  "styx:framelease": "#60a5fa",
  "eidos:mask": "#a78bfa",
  "eidos:runs": "#c084fc",
  "eidos:quads": "#f472b6",
  "eidos:detections": "#4ade80",
  "eidos:rejected_markers": "#fb7185",
  "eidos:refined_corners": "#2dd4bf",
  "eidos:tag_poses": "#fbbf24",
  "eidos:image": "#93c5fd",
  "eidos:camera_calibration": "#e2e8f0",
  "eidos:camera_extrinsics": "#cbd5e1",
  "eidos:pose_solutions": "#f59e0b",
  "eidos:camera_pose": "#fb923c",
  "eidos:feature_tracks": "#22d3ee",
  any: "#9aa0ae",
};

export function defaults(type: NodeType): Record<string, number | string | boolean> {
  return Object.fromEntries(type.params.map((p) => [p.name, p.default]));
}

// --- The device's catalog ----------------------------------------------------

/** The plugin's own category (Eidos `category` metadata) as the palette's. */
const CATEGORY_BY_PLUGIN: Record<string, NodeType["category"]> = {
  fiducials: "Fiducials",
  calibration: "Geometry",
  color: "Color",
  contour: "Mask",
  draw: "Output",
  filter: "Filter",
  geometry: "Geometry",
  io: "Source",
  mask: "Mask",
  motion: "Motion",
  sampling: "Geometry",
};

const CATEGORY_BY_SECTION: Record<string, NodeType["category"]> = {
  aruco: "Fiducials",
  detectors: "Fiducials",
  calibration: "Geometry",
  filter: "Filter",
  mask: "Mask",
  contour: "Mask",
  geometry: "Geometry",
  sampling: "Geometry",
  color: "Color",
  motion: "Motion",
  draw: "Output",
  io: "Source",
};

/** UI-level nodes the engine has no declaration for (camera bindings, host outputs). */
const UI_PLUGINS = new Set(["styx.frames", "helios"]);

function humanize(id: string): string {
  const last = id.split(/[:.]/).at(-1) ?? id;
  const words = last.replace(/_/g, " ").trim();
  return words.charAt(0).toUpperCase() + words.slice(1);
}

function scalarOf(port: DeviceCatalogPort): string | undefined {
  return /"Scalar":"(\w+)"/.exec(port.type_key)?.[1];
}

function toParam(port: DeviceCatalogPort, known: Param | undefined): Param {
  const meta: ParamMeta = {};
  if (port.label) meta.label = port.label;
  if (port.unit) meta.unit = port.unit;
  if (port.group) meta.group = port.group;
  if (port.advanced) meta.advanced = true;
  return { ...typedParam(port, known), ...meta };
}

function typedParam(port: DeviceCatalogPort, known: Param | undefined): Param {
  const help = port.description ?? known?.help ?? "";
  const scalar = scalarOf(port);
  if (port.options?.length) return { name: port.name, kind: "enum", default: String(port.default ?? port.options[0]), options: port.options.map(String), help };
  if (typeof port.default === "boolean" || scalar === "Bool") return { name: port.name, kind: "bool", default: Boolean(port.default), help };
  const value = Number(port.default ?? 0);
  if (scalar === "Float" || scalar === "F32" || scalar === "F64" || (scalar === undefined && !Number.isInteger(value))) {
    const min = port.min ?? (known?.kind === "float" ? known.min : Math.min(0, value));
    const max = port.max ?? (known?.kind === "float" ? known.max : Math.max(1, value * 4));
    return { name: port.name, kind: "float", default: value, min, max, step: port.step ?? (known?.kind === "float" ? known.step : (max - min) / 100 || 0.01), help };
  }
  if (typeof port.default === "string") return { name: port.name, kind: "enum", default: port.default, options: [port.default], help };
  const unsigned = scalar?.startsWith("U") ?? false;
  const min = port.min ?? (known?.kind === "int" ? known.min : unsigned ? 0 : Math.min(0, value));
  const max = port.max ?? (known?.kind === "int" ? known.max : Math.max(16, value * 4));
  return { name: port.name, kind: "int", default: value, min, max, help };
}

function fromDevice(node: DeviceCatalogNode, known: NodeType | undefined): NodeType {
  const section = node.id.split(":")[1]?.split(".")[0] ?? "";
  const ports = (list: DeviceCatalogPort[]) => list.map((p) => ({ name: p.name, type: p.type_key }));
  const summary = node.summary ?? known?.summary ?? node.label ?? "";
  return {
    id: node.id,
    plugin: node.plugin ?? known?.plugin ?? node.id.split(":")[0],
    category: known?.category ?? (node.category ? CATEGORY_BY_PLUGIN[node.category] : undefined) ?? CATEGORY_BY_SECTION[section] ?? "Filter",
    // A short label is the plugin's title; a sentence (older plugins) is not.
    title: known?.title ?? (node.label && node.label.length <= 40 && !node.label.endsWith(".") ? node.label : humanize(node.id)),
    summary,
    inputs: ports(node.inputs.filter((p) => p.default === undefined)),
    outputs: ports(node.outputs),
    params: node.inputs.filter((p) => p.default !== undefined).map((p) => toParam(p, known?.params.find((k) => k.name === p.name))),
    available: true,
    group: node.group,
    stageOf: node.stage_of?.length ? node.stage_of : undefined,
  };
}

/**
 * Make `CATALOG` (and `CATALOG_BY_ID`) what the device declares: its nodes, typed as it
 * types them, with the UI's titles and categories for the ones it knows. Nodes the UI
 * lists that the device does not are kept (old graphs still draw) but no longer offered.
 */
export function mergeDeviceCatalog(device: DeviceCatalog) {
  const merged = new Map<string, NodeType>();
  for (const node of device.nodes) {
    const known = CATALOG_BY_ID[node.id];
    // A declaration with no ports (the host bridge) says less than the UI's entry does.
    const bare = !node.inputs.length && !node.outputs.length;
    merged.set(node.id, known && bare ? { ...known, available: true } : fromDevice(node, known));
  }
  for (const known of CATALOG) {
    if (!merged.has(known.id)) merged.set(known.id, { ...known, available: UI_PLUGINS.has(known.plugin) });
  }
  CATALOG.splice(0, CATALOG.length, ...merged.values());
  for (const id of Object.keys(CATALOG_BY_ID)) delete CATALOG_BY_ID[id];
  Object.assign(CATALOG_BY_ID, Object.fromEntries(CATALOG.map((n) => [n.id, n])));
}
