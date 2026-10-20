// Feature tracks (`eidos:features.track`) on a camera feed: the output's value
// parsed, each track's short trail by id, the positions scaled into the feed's
// pixels the way detection corners are, and how a track's dot is drawn.

import type { FeatureTrack, FeatureTracks } from "#lib/api/model.js";

/** Positions kept per track id for its trail (the newest last). */
export const TRAIL_LENGTH = 10;
/** Ages from this one draw as fully settled. */
const SETTLED_AGE = 30;

export interface TrailPoint {
  x: number;
  y: number;
}

/** A camera's tracks of the latest tick, with each live track's trail. */
export interface CameraFeatures {
  tracks: FeatureTracks;
  trails: Map<number, TrailPoint[]>;
  /** The output's value as received; an unchanged value is the same tick, not a new one. */
  signature: string;
}

/**
 * Keeps each track's last positions by id. Advancing by a tick appends the track's position and
 * drops the ids the tick does not list (their tracks ended). Trails are replaced, never mutated,
 * so the map an advance returns can be held as state.
 */
export class TrailBook {
  private trails = new Map<number, TrailPoint[]>();

  advance(tracks: FeatureTrack[]): Map<number, TrailPoint[]> {
    const next = new Map<number, TrailPoint[]>();
    for (const track of tracks) {
      const previous = this.trails.get(track.id) ?? [];
      next.set(track.id, [...previous, { x: track.x, y: track.y }].slice(-TRAIL_LENGTH));
    }
    this.trails = next;
    return next;
  }
}

function isTrack(value: unknown): value is FeatureTrack {
  const t = value as Partial<FeatureTrack> | null;
  return (
    typeof t === "object" &&
    t !== null &&
    typeof t.id === "number" &&
    typeof t.x === "number" &&
    typeof t.y === "number" &&
    Number.isFinite(t.x) &&
    Number.isFinite(t.y) &&
    typeof t.age === "number" &&
    (t.status === "new" || t.status === "tracked")
  );
}

/** A `tracks` output's value as overlay tracks, in the pipeline's frame (`frame`, when known). Null when it is not one. */
export function toFeatureTracks(value: unknown, frame?: { w: number; h: number }): FeatureTracks | null {
  const out = value as { tracks?: unknown; summary?: FeatureTracks["summary"] } | null | undefined;
  if (!out || !Array.isArray(out.tracks)) return null;
  const tracks = out.tracks.filter(isTrack).map((t) => ({ id: t.id, x: t.x, y: t.y, age: t.age, status: t.status }));
  const summary = out.summary ?? {
    count: tracks.length,
    tracked: tracks.filter((t) => t.status === "tracked").length,
    new: tracks.filter((t) => t.status === "new").length,
    lost: 0,
  };
  return { tracks, summary, frame };
}

/**
 * The tracks and trails in the feed's pixels (`fw` x `fh`). Positions are scaled from the
 * pipeline's frame exactly as detection corners are; without a known frame they are feed pixels.
 */
export function toFeedSpace(features: CameraFeatures, fw: number, fh: number): { tracks: FeatureTrack[]; trails: [number, TrailPoint[]][] } {
  const frame = features.tracks.frame;
  const x = (v: number) => (frame ? (v * fw) / frame.w : v);
  const y = (v: number) => (frame ? (v * fh) / frame.h : v);
  return {
    tracks: features.tracks.tracks.map((t) => ({ ...t, x: x(t.x), y: y(t.y) })),
    trails: [...features.trails].map(([id, points]) => [id, points.map((p) => ({ x: x(p.x), y: y(p.y) }))]),
  };
}

/** A track's dot colour: a new track is bright; an older one settles toward a calm cyan. */
export function trackFill(track: Pick<FeatureTrack, "status" | "age">): string {
  if (track.status === "new") return "#fef08a";
  const settled = Math.min(Math.max(track.age, 0), SETTLED_AGE) / SETTLED_AGE;
  return `rgba(34, 211, 238, ${(0.9 - 0.4 * settled).toFixed(2)})`;
}

/** The summary chip's text: "12 tracks · 9 tracked · 3 new · 1 lost" (`short`, for a small feed: "12 · 3 new · 1 lost"). */
export function featureSummaryText(summary: FeatureTracks["summary"], short = false): string {
  if (short) return `${summary.count} · ${summary.new} new · ${summary.lost} lost`;
  return `${summary.count} tracks · ${summary.tracked} tracked · ${summary.new} new · ${summary.lost} lost`;
}
