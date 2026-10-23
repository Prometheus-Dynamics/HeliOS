// The frames of `/v1/streams/ws` (helios-api's `live_streams/codec.rs` is the other end): a
// catalog as JSON text, samples as little-endian binary. Times are µs since the Unix epoch and
// values are f64, so one decoder serves every series kind.

/** A series of the node's stream, as the catalog names it. Its index is its place in the list. */
export interface CatalogSeries {
  name: string;
  unit: string | null;
  kind: "f32" | "f64" | "i64" | "u32";
  period_us: number;
}

export interface Catalog {
  stream: string;
  owner: string;
  epoch: number;
  live: boolean;
  series: CatalogSeries[];
}

/** The follower's state: `connecting`, `unavailable`, `waiting`, `live`, or `stale`. */
export interface StreamStatus {
  state: string;
  reason: string;
}

export type TextFrame = { type: "catalog"; catalog: Catalog } | { type: "status"; status: StreamStatus } | { type: "lagged"; missed: number };

/** One series' samples in a batch: times (µs) and values, in order. */
export interface SeriesBlock {
  series: number;
  t: Float64Array;
  v: Float64Array;
}

export interface SamplesBatch {
  /** Orion's backfill: the history a subscription starts with. */
  backfill: boolean;
  blocks: SeriesBlock[];
}

export const SAMPLES_FRAME = 1;
export const FLAG_BACKFILL = 1;

/** A text frame: the catalog, the status, or a lag notice. Unknown or malformed frames are null. */
export function parseTextFrame(text: string): TextFrame | null {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    return null;
  }
  if (!value || typeof value !== "object") return null;
  const frame = value as Record<string, unknown>;
  if (frame.type === "catalog" && Array.isArray(frame.series)) {
    return { type: "catalog", catalog: frame as unknown as Catalog };
  }
  if (frame.type === "status" && typeof frame.state === "string") {
    return { type: "status", status: { state: frame.state, reason: typeof frame.reason === "string" ? frame.reason : "" } };
  }
  if (frame.type === "lagged" && typeof frame.missed === "number") {
    return { type: "lagged", missed: frame.missed };
  }
  return null;
}

/** A samples frame, decoded. `null` when the bytes are not one (or are cut short). */
export function decodeSamples(buffer: ArrayBuffer): SamplesBatch | null {
  const view = new DataView(buffer);
  if (buffer.byteLength < 4 || view.getUint8(0) !== SAMPLES_FRAME) return null;
  const flags = view.getUint8(1);
  const blockCount = view.getUint16(2, true);
  let at = 4;
  const blocks: SeriesBlock[] = [];
  for (let b = 0; b < blockCount; b++) {
    if (at + 6 > buffer.byteLength) return null;
    const series = view.getUint16(at, true);
    const n = view.getUint32(at + 2, true);
    at += 6;
    if (at + n * 16 > buffer.byteLength) return null;
    const t = new Float64Array(n);
    const v = new Float64Array(n);
    for (let i = 0; i < n; i++) t[i] = view.getFloat64(at + i * 8, true);
    at += n * 8;
    for (let i = 0; i < n; i++) v[i] = view.getFloat64(at + i * 8, true);
    at += n * 8;
    blocks.push({ series, t, v });
  }
  if (at !== buffer.byteLength) return null;
  return { backfill: (flags & FLAG_BACKFILL) !== 0, blocks };
}

/** The bytes of a samples frame: what `decodeSamples` reads back (the simulated robot sends these). */
export function encodeSamples(batch: { backfill: boolean; blocks: { series: number; samples: [number, number][] }[] }): ArrayBuffer {
  const total = batch.blocks.reduce((sum, block) => sum + block.samples.length, 0);
  const buffer = new ArrayBuffer(4 + batch.blocks.length * 6 + total * 16);
  const view = new DataView(buffer);
  view.setUint8(0, SAMPLES_FRAME);
  view.setUint8(1, batch.backfill ? FLAG_BACKFILL : 0);
  view.setUint16(2, batch.blocks.length, true);
  let at = 4;
  for (const block of batch.blocks) {
    view.setUint16(at, block.series, true);
    view.setUint32(at + 2, block.samples.length, true);
    at += 6;
    for (const [t] of block.samples) {
      view.setFloat64(at, t, true);
      at += 8;
    }
    for (const [, value] of block.samples) {
      view.setFloat64(at, value, true);
      at += 8;
    }
  }
  return buffer;
}
