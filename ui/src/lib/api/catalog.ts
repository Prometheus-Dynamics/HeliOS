// The node catalog the device reports: every node type a graph can use, with
// typed ports and parameters. Mirrors what the installed plugins register
// (styx.frames, eidos, helios bridges); the UI renders whatever is listed, so
// a new plugin node appears here with no UI change.

export type PortType =
  | "styx:framelease"
  | "eidos:mask"
  | "eidos:runs"
  | "eidos:quads"
  | "eidos:detections"
  | "eidos:rejected_markers"
  | "eidos:refined_corners"
  | "eidos:tag_poses"
  | "eidos:image"
  | "any";

export interface Port {
  name: string;
  type: PortType;
}

export type Param =
  | { name: string; kind: "int"; default: number; min: number; max: number; help: string }
  | { name: string; kind: "float"; default: number; min: number; max: number; step: number; help: string }
  | { name: string; kind: "bool"; default: boolean; help: string }
  | { name: string; kind: "enum"; default: string; options: string[]; help: string };

export interface NodeType {
  id: string;
  plugin: string;
  category: "Source" | "Fiducials" | "Filter" | "Mask" | "Geometry" | "Color" | "Motion" | "Output";
  title: string;
  summary: string;
  inputs: Port[];
  outputs: Port[];
  params: Param[];
}

const dictionaries = [
  "aruco_4x4_50",
  "aruco_4x4_100",
  "aruco_5x5_100",
  "aruco_6x6_250",
  "apriltag_16h5",
  "apriltag_25h9",
  "apriltag_36h10",
  "apriltag_36h11",
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
    id: "eidos:aruco.pose",
    plugin: "eidos",
    category: "Fiducials",
    title: "Tag pose",
    summary: "Camera-frame pose of each tag from refined corners.",
    inputs: [
      { name: "detections", type: "eidos:detections" },
      { name: "refined_corners", type: "eidos:refined_corners" },
    ],
    outputs: [{ name: "poses", type: "eidos:tag_poses" }],
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
    id: "helios:stream",
    plugin: "helios",
    category: "Output",
    title: "Stream",
    summary: "Publishes an output as a cluster stream resource.",
    inputs: [{ name: "value", type: "any" }],
    outputs: [],
    params: [
      { name: "name", kind: "enum", default: "detections", options: ["detections", "poses", "rejected", "telemetry"], help: "Stream name." },
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

export const PORT_COLORS: Record<PortType, string> = {
  "styx:framelease": "#60a5fa",
  "eidos:mask": "#a78bfa",
  "eidos:runs": "#c084fc",
  "eidos:quads": "#f472b6",
  "eidos:detections": "#4ade80",
  "eidos:rejected_markers": "#fb7185",
  "eidos:refined_corners": "#2dd4bf",
  "eidos:tag_poses": "#fbbf24",
  "eidos:image": "#93c5fd",
  any: "#9aa0ae",
};

export function defaults(type: NodeType): Record<string, number | string | boolean> {
  return Object.fromEntries(type.params.map((p) => [p.name, p.default]));
}
