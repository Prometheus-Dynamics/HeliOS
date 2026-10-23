// Styx's ground truth for a simulated camera rendering a glTF scene (`truth=<path>`, see Styx's
// docs/simulation.md, "Ground truth"): one JSON line per frame. The camera is in the glTF scene's
// frame; the tags' corners are the truth points `tag<ID>/<corner>` (tl, tr, br, bl as printed).
// This module reads them as HeliOS sees the field: WPILib's frame (X forward, Y left, Z up) and
// each tag's four corners in pixels.

export type Vec3 = [number, number, number];

/** A tag with all four corners in the frame, top-left, top-right, bottom-right, bottom-left, in pixels. */
export interface TruthTag {
  id: number;
  corners: [number, number][];
}

export interface TruthFrame {
  /** The frame's start on CLOCK_MONOTONIC, ns. */
  ts: number;
  /** The camera in the field (WPILib frame), metres. */
  position: Vec3;
  tags: TruthTag[];
}

const CORNERS = ["tl", "tr", "br", "bl"] as const;

/** glTF's scene frame to WPILib's: (x, y, z) -> (x, -z, y). */
export function wpilibFromScene(p: Vec3): Vec3 {
  return [p[0], -p[2], p[1]];
}

/** The tags (all four corners in the frame) among a frame's truth points. */
export function tagsOf(points: { name: string; x: number; y: number }[]): TruthTag[] {
  const byTag = new Map<number, Map<string, [number, number]>>();
  for (const point of points) {
    const match = /^tag(\d+)\/(tl|tr|br|bl)$/.exec(point.name);
    if (!match) continue;
    const id = Number(match[1]);
    const corners = byTag.get(id) ?? new Map<string, [number, number]>();
    corners.set(match[2], [point.x, point.y]);
    byTag.set(id, corners);
  }
  const tags: TruthTag[] = [];
  for (const [id, corners] of byTag) {
    if (!CORNERS.every((c) => corners.has(c))) continue;
    tags.push({ id, corners: CORNERS.map((c) => corners.get(c)!) as TruthTag["corners"] });
  }
  return tags;
}

/** Styx's truth file (JSON Lines) as frames. */
export function readTruth(text: string): TruthFrame[] {
  return text
    .split("\n")
    .filter(Boolean)
    .map((line) => {
      const frame = JSON.parse(line);
      return {
        ts: frame.timestamp_ns as number,
        position: wpilibFromScene(frame.camera.position as Vec3),
        tags: tagsOf(frame.points ?? []),
      };
    });
}
