// The dense mapper's outputs as the UI reads them (Eidos's `dense.map`): `dense_mesh` (at most
// 1 Hz), `dense_depth` (a keyframe's inverse depth, half the sweep size) and `dense_timing` (the
// mapper's work per tick). Byte buffers arrive as base64 text (a JSON array of bytes is accepted
// too). Each message is decoded once into typed arrays: positions, normals and indices ready for
// a three.js BufferGeometry, and the depth as RGBA pixels with a colour map. Malformed input
// (wrong lengths, a missing grey, indices past the vertices) is dropped, never drawn.

export interface DenseMeshView {
  seq: number;
  frame: number;
  tNs: number;
  keyframes: number;
  submaps: number;
  voxelM: number;
  /** Vertices and triangles as decoded (the buffers' whole rows). */
  vertices: number;
  triangles: number;
  /** x, y, z per vertex, in the live map's reference frame. */
  positions: Float32Array;
  /** Unit normals per vertex; null when absent or of the wrong length (the scene derives them). */
  normals: Float32Array | null;
  /** One grey byte per vertex; null when the mesh keeps none (empty or of the wrong length). */
  grey: Uint8Array | null;
  /** Three vertex indices per triangle, all in range and on finite vertices. */
  indices: Uint32Array;
  /** Triangles dropped for an index past the vertices or a non-finite corner. */
  droppedTriangles: number;
}

export interface DenseDepthView {
  frame: number;
  width: number;
  height: number;
  /** One byte per pixel, row-major: bigger is nearer, 0 is unknown. */
  inverseDepth: Uint8Array;
}

export interface DenseTimingView {
  frame: number;
  tNs: number;
  /** `waiting` (no calibration), `running`, or `no_pose` (a frame with no pose yet). */
  state: string;
  processed: number;
  keyframes: number;
  droppedKeyframes: number;
  pending: number;
  keyframeRate: number;
  sweepMs: number;
  fillMs: number;
  integrateMs: number;
  meshMs: number;
  budgetMs: number;
  spacing: number;
  costMs: number;
  arrived: number;
  droppedBlocks: number;
  submaps: number;
  blocks: number;
  volumeBytes: number;
}

type Json = Record<string, unknown>;
const isObject = (value: unknown): value is Json => typeof value === "object" && value !== null;
const num = (value: unknown, fallback = 0): number => (typeof value === "number" && Number.isFinite(value) ? value : fallback);

/** Base64 text as bytes, or null when it is not base64. */
export function decodeBase64(text: string): Uint8Array | null {
  let binary: string;
  try {
    binary = atob(text);
  } catch {
    return null;
  }
  const out = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) out[i] = binary.charCodeAt(i);
  return out;
}

/** Base64 text (or a JSON array of byte values) as bytes; null for anything else. */
export function bufferOf(value: unknown): Uint8Array | null {
  if (typeof value === "string") return decodeBase64(value);
  if (Array.isArray(value)) return Uint8Array.from(value, (v) => num(v) & 0xff);
  return null;
}

/** Base64 text of bytes (the inverse of `decodeBase64`), for the mock robot's wire format. */
export function encodeBase64(bytes: Uint8Array): string {
  let binary = "";
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) binary += String.fromCharCode(...bytes.subarray(i, i + chunk));
  return btoa(binary);
}

/** Little-endian f32 values of whole 4-byte groups. */
function float32s(bytes: Uint8Array): Float32Array {
  const n = bytes.byteLength >>> 2;
  const view = new DataView(bytes.buffer, bytes.byteOffset, n * 4);
  const out = new Float32Array(n);
  for (let i = 0; i < n; i++) out[i] = view.getFloat32(i * 4, true);
  return out;
}

/** Little-endian u32 values of whole 4-byte groups. */
function uint32s(bytes: Uint8Array): Uint32Array {
  const n = bytes.byteLength >>> 2;
  const view = new DataView(bytes.buffer, bytes.byteOffset, n * 4);
  const out = new Uint32Array(n);
  for (let i = 0; i < n; i++) out[i] = view.getUint32(i * 4, true);
  return out;
}

/**
 * The `dense_mesh` value as typed arrays. The vertex count is what the positions hold (whole
 * vertices, 12 bytes each). Normals must be as long as the positions, grey one byte per vertex;
 * otherwise they are dropped and the scene derives or omits them. Triangles with an index past
 * the vertices, or a non-finite corner, are dropped and counted. Null when the value is not a mesh.
 */
export function parseDenseMesh(value: unknown): DenseMeshView | null {
  if (!isObject(value)) return null;
  const positionBytes = bufferOf(value.positions);
  if (!positionBytes) return null;
  const vertices = Math.floor(positionBytes.byteLength / 12);
  const positions = float32s(positionBytes.subarray(0, vertices * 12));

  const normalBytes = bufferOf(value.normals);
  const normals = normalBytes && normalBytes.byteLength === vertices * 12 ? float32s(normalBytes) : null;
  const greyBytes = bufferOf(value.grey);
  const grey = greyBytes && vertices > 0 && greyBytes.byteLength === vertices ? greyBytes : null;

  const indexBytes = bufferOf(value.indices) ?? new Uint8Array(0);
  const declared = Math.floor(indexBytes.byteLength / 12);
  const raw = uint32s(indexBytes.subarray(0, declared * 12));
  const kept = new Uint32Array(declared * 3);
  let triangles = 0;
  const finite = (v: number) => Number.isFinite(positions[v * 3]) && Number.isFinite(positions[v * 3 + 1]) && Number.isFinite(positions[v * 3 + 2]);
  for (let t = 0; t < declared; t++) {
    const a = raw[t * 3];
    const b = raw[t * 3 + 1];
    const c = raw[t * 3 + 2];
    if (a >= vertices || b >= vertices || c >= vertices || !finite(a) || !finite(b) || !finite(c)) continue;
    kept[triangles * 3] = a;
    kept[triangles * 3 + 1] = b;
    kept[triangles * 3 + 2] = c;
    triangles++;
  }

  return {
    seq: num(value.seq),
    frame: num(value.frame),
    tNs: num(value.t_ns),
    keyframes: num(value.keyframes),
    submaps: num(value.submaps),
    voxelM: num(value.voxel_m),
    vertices,
    triangles,
    positions,
    normals,
    grey,
    indices: kept.slice(0, triangles * 3),
    droppedTriangles: declared - triangles,
  };
}

/** The `dense_depth` value: `width * height` bytes, or null when the size does not match. */
export function parseDenseDepth(value: unknown): DenseDepthView | null {
  if (!isObject(value)) return null;
  const width = Math.floor(num(value.width));
  const height = Math.floor(num(value.height));
  if (width <= 0 || height <= 0) return null;
  const bytes = bufferOf(value.inverse_depth);
  if (!bytes || bytes.byteLength !== width * height) return null;
  return { frame: num(value.frame), width, height, inverseDepth: bytes };
}

/** The `dense_timing` value, or null when it is not one. */
export function parseDenseTiming(value: unknown): DenseTimingView | null {
  if (!isObject(value) || typeof value.state !== "string") return null;
  return {
    frame: num(value.frame),
    tNs: num(value.t_ns),
    state: value.state,
    processed: num(value.processed),
    keyframes: num(value.keyframes),
    droppedKeyframes: num(value.dropped_keyframes),
    pending: num(value.pending),
    keyframeRate: num(value.keyframe_rate),
    sweepMs: num(value.sweep_ms),
    fillMs: num(value.fill_ms),
    integrateMs: num(value.integrate_ms),
    meshMs: num(value.mesh_ms),
    budgetMs: num(value.budget_ms),
    spacing: num(value.spacing),
    costMs: num(value.cost_ms),
    arrived: num(value.arrived),
    droppedBlocks: num(value.dropped_blocks),
    submaps: num(value.submaps),
    blocks: num(value.blocks),
    volumeBytes: num(value.volume_bytes),
  };
}

/**
 * Viridis, nine stops: the colour of a depth byte runs dark purple (far, or small inverse depth)
 * to yellow (near). Perceptually even and readable in both themes.
 */
const VIRIDIS: [number, number, number][] = [
  [68, 1, 84],
  [71, 44, 122],
  [59, 81, 139],
  [44, 113, 142],
  [33, 144, 141],
  [39, 173, 129],
  [92, 200, 99],
  [170, 220, 50],
  [253, 231, 37],
];

/** The colour map as 256 RGB entries (index = the depth byte). */
export const DEPTH_COLOURS: Uint8ClampedArray = (() => {
  const out = new Uint8ClampedArray(256 * 3);
  for (let i = 0; i < 256; i++) {
    const t = (i / 255) * (VIRIDIS.length - 1);
    const lo = Math.floor(t);
    const hi = Math.min(lo + 1, VIRIDIS.length - 1);
    const f = t - lo;
    for (let c = 0; c < 3; c++) out[i * 3 + c] = Math.round(VIRIDIS[lo][c] * (1 - f) + VIRIDIS[hi][c] * f);
  }
  return out;
})();

/**
 * The depth as RGBA pixels: colour-mapped, and transparent where the depth is unknown (byte 0), so
 * the image behind shows through. Ready for `new ImageData(rgba, width, height)`.
 */
export function depthRgba(depth: Pick<DenseDepthView, "width" | "height" | "inverseDepth">): Uint8ClampedArray<ArrayBuffer> {
  const count = depth.width * depth.height;
  const out = new Uint8ClampedArray(count * 4);
  for (let i = 0; i < count; i++) {
    const v = depth.inverseDepth[i];
    if (v === 0) continue;
    out[i * 4] = DEPTH_COLOURS[v * 3];
    out[i * 4 + 1] = DEPTH_COLOURS[v * 3 + 1];
    out[i * 4 + 2] = DEPTH_COLOURS[v * 3 + 2];
    out[i * 4 + 3] = 255;
  }
  return out;
}

/**
 * Per-vertex colours for the grey shading: the grey byte lifted off black (a dark surface still
 * reads against the dark background) and a touch cool, as the map's points are drawn.
 */
export function greyColours(grey: Uint8Array): Float32Array {
  const out = new Float32Array(grey.length * 3);
  for (let i = 0; i < grey.length; i++) {
    const v = 0.32 + 0.68 * (grey[i] / 255);
    out[i * 3] = v * 0.94;
    out[i * 3 + 1] = v * 0.97;
    out[i * 3 + 2] = v;
  }
  return out;
}

/** The mesh's bounds, or null without vertices. */
export function meshBounds(mesh: Pick<DenseMeshView, "positions" | "vertices">): { min: [number, number, number]; max: [number, number, number] } | null {
  if (mesh.vertices === 0) return null;
  const min: [number, number, number] = [Infinity, Infinity, Infinity];
  const max: [number, number, number] = [-Infinity, -Infinity, -Infinity];
  const p = mesh.positions;
  for (let i = 0; i < mesh.vertices; i++) {
    for (let axis = 0; axis < 3; axis++) {
      const v = p[i * 3 + axis];
      if (!Number.isFinite(v)) continue;
      if (v < min[axis]) min[axis] = v;
      if (v > max[axis]) max[axis] = v;
    }
  }
  return Number.isFinite(min[0]) ? { min, max } : null;
}

/** A pipeline's latest dense mapper outputs. `depthAt` and `meshAt` are when the UI received them (performance.now()). */
export interface DenseView {
  mesh: DenseMeshView | null;
  depth: DenseDepthView | null;
  timing: DenseTimingView | null;
  depthAt: number;
  meshAt: number;
}
