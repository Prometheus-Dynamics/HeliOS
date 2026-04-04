export type Corner2D = { x: number; y: number };

export type Detection = {
  id: number;
  rotation: number;
  corners: Corner2D[];
};

export type GuidanceStatus = {
  label: string;
  tone: 'good' | 'warn' | 'bad';
};

export type SourceResolution = { width: number; height: number } | null | undefined;
export type GuidanceGrid = { cols: number; rows: number };

const MIN_TAGS = 2;
const TARGET_TAGS = 6;
const MIN_AREA_RATIO = 0.003;
const MAX_AREA_RATIO = 0.08;
const MIN_PLAUSIBLE_AREA_RATIO = 0.00006;
const MAX_PLAUSIBLE_AREA_RATIO = 0.35;
const MIN_PLAUSIBLE_SIDE_PX = 5;
const MIN_PLAUSIBLE_SIDE_RATIO = 0.18;
const CLOSE_EXTENT_RATIO = 0.12;
const FAR_EXTENT_RATIO = 0.05;
const MIN_FRAME_COVERAGE = 0.12;
const EDGE_PAD_RATIO = 0.08;

export function clamp(v: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, v));
}

export function colorForId(id: number): string {
  const hue = ((id * 2654435761) >>> 0) % 360;
  return `hsl(${hue}, 92%, 60%)`;
}

export function quadArea(corners: Array<{ x: number; y: number }>): number {
  if (corners.length < 4) return 0;
  let sum = 0;
  for (let i = 0; i < corners.length; i++) {
    const a = corners[i];
    const b = corners[(i + 1) % corners.length];
    sum += a.x * b.y - b.x * a.y;
  }
  return Math.abs(sum) / 2;
}

function sideLengths(corners: Corner2D[]): number[] {
  if (corners.length < 4) return [];
  const lengths: number[] = [];
  for (let i = 0; i < 4; i++) {
    const a = corners[i];
    const b = corners[(i + 1) % 4];
    const len = Math.hypot(a.x - b.x, a.y - b.y);
    if (Number.isFinite(len) && len > 0) lengths.push(len);
  }
  return lengths;
}

function parseCorner(raw: unknown): Corner2D | null {
  if (!raw || typeof raw !== 'object') return null;
  const x = Number((raw as { x?: unknown }).x);
  const y = Number((raw as { y?: unknown }).y);
  if (!Number.isFinite(x) || !Number.isFinite(y)) return null;
  return { x, y };
}

function isValidResolution(sourceResolution: SourceResolution): sourceResolution is { width: number; height: number } {
  return Boolean(
    sourceResolution &&
      Number.isFinite(sourceResolution.width) &&
      Number.isFinite(sourceResolution.height) &&
      sourceResolution.width > 0 &&
      sourceResolution.height > 0
  );
}

function isPlausibleDetection(corners: Corner2D[], sourceResolution: SourceResolution): boolean {
  if (corners.length < 4) return false;

  const lengths = sideLengths(corners);
  if (lengths.length < 4) return false;
  const minLen = Math.min(...lengths);
  const maxLen = Math.max(...lengths);
  if (!Number.isFinite(minLen) || !Number.isFinite(maxLen) || minLen < MIN_PLAUSIBLE_SIDE_PX || maxLen <= 0) return false;
  if ((minLen / maxLen) < MIN_PLAUSIBLE_SIDE_RATIO) return false;
  if (!isValidResolution(sourceResolution)) return true;

  const boundX = sourceResolution.width * 0.5;
  const boundY = sourceResolution.height * 0.5;
  for (const c of corners) {
    if (c.x < -boundX || c.x > sourceResolution.width + boundX || c.y < -boundY || c.y > sourceResolution.height + boundY) {
      return false;
    }
  }

  const area = quadArea(corners);
  const frameArea = sourceResolution.width * sourceResolution.height;
  if (!Number.isFinite(area) || !Number.isFinite(frameArea) || area <= 0 || frameArea <= 0) return false;
  const areaRatio = area / frameArea;
  return areaRatio >= MIN_PLAUSIBLE_AREA_RATIO && areaRatio <= MAX_PLAUSIBLE_AREA_RATIO;
}

export function normalizeDetections(raw: unknown, sourceResolution: SourceResolution): Detection[] {
  if (!Array.isArray(raw)) return [];
  const out: Detection[] = [];
  for (const item of raw) {
    if (!item || typeof item !== 'object') continue;
    const cornersRaw = Array.isArray((item as { corners?: unknown }).corners)
      ? (item as { corners: unknown[] }).corners
      : [];
    const corners = cornersRaw.map(parseCorner).filter((corner): corner is Corner2D => corner !== null);
    if (corners.length < 4) continue;
    if (!isPlausibleDetection(corners, sourceResolution)) continue;
    const idRaw = Number((item as { id?: unknown }).id);
    const rotationRaw = Number((item as { rotation?: unknown }).rotation ?? 0);
    out.push({
      id: Number.isFinite(idRaw) ? idRaw : 0,
      rotation: Number.isFinite(rotationRaw) ? rotationRaw : 0,
      corners
    });
  }
  return out;
}

export function frameCoverageRatio(dets: Detection[], sourceResolution: SourceResolution, grid: GuidanceGrid): number {
  if (!isValidResolution(sourceResolution)) return 0;
  const visited = new Uint8Array(grid.cols * grid.rows);
  const mark = (xPx: number, yPx: number) => {
    const nx = clamp(xPx / sourceResolution.width, 0, 0.999999);
    const ny = clamp(yPx / sourceResolution.height, 0, 0.999999);
    const col = Math.floor(nx * grid.cols);
    const row = Math.floor(ny * grid.rows);
    const idx = row * grid.cols + col;
    if (idx < 0 || idx >= visited.length) return;
    visited[idx] = 1;
  };

  for (const det of dets) {
    const corners = det.corners ?? [];
    if (corners.length < 4) continue;
    let cx = 0;
    let cy = 0;
    for (const c of corners) {
      mark(c.x, c.y);
      cx += c.x;
      cy += c.y;
    }
    mark(cx / corners.length, cy / corners.length);
  }

  let count = 0;
  for (const v of visited) {
    if (v) count += 1;
  }
  return visited.length > 0 ? count / visited.length : 0;
}

function boundsForDetections(dets: Detection[]): { minX: number; minY: number; maxX: number; maxY: number } | null {
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const det of dets) {
    for (const c of det.corners ?? []) {
      if (!Number.isFinite(c.x) || !Number.isFinite(c.y)) continue;
      minX = Math.min(minX, c.x);
      minY = Math.min(minY, c.y);
      maxX = Math.max(maxX, c.x);
      maxY = Math.max(maxY, c.y);
    }
  }
  if (!Number.isFinite(minX) || !Number.isFinite(minY)) return null;
  return { minX, minY, maxX, maxY };
}

export function detectionExtentRatio(dets: Detection[], sourceResolution: SourceResolution): number {
  if (!isValidResolution(sourceResolution)) return 0;
  const bounds = boundsForDetections(dets);
  if (!bounds) return 0;
  const w = Math.max(0, bounds.maxX - bounds.minX);
  const h = Math.max(0, bounds.maxY - bounds.minY);
  const area = w * h;
  const frame = sourceResolution.width * sourceResolution.height;
  if (!Number.isFinite(area) || area <= 0 || !Number.isFinite(frame) || frame <= 0) return 0;
  return area / frame;
}

export function averageAreaRatio(dets: Detection[], sourceResolution: SourceResolution): number {
  if (!isValidResolution(sourceResolution)) return 0;
  const imgArea = sourceResolution.width * sourceResolution.height;
  let sum = 0;
  let count = 0;
  for (const det of dets) {
    const corners = det.corners ?? [];
    if (corners.length < 4) continue;
    const area = quadArea(corners);
    if (!Number.isFinite(area) || area <= 0) continue;
    sum += area;
    count += 1;
  }
  return count === 0 ? 0 : sum / count / imgArea;
}

export function edgeCoverageCount(dets: Detection[], sourceResolution: SourceResolution): number {
  if (!isValidResolution(sourceResolution)) return 0;
  const bounds = boundsForDetections(dets);
  if (!bounds) return 0;
  const left = bounds.minX <= sourceResolution.width * EDGE_PAD_RATIO;
  const right = bounds.maxX >= sourceResolution.width * (1 - EDGE_PAD_RATIO);
  const top = bounds.minY <= sourceResolution.height * EDGE_PAD_RATIO;
  const bottom = bounds.maxY >= sourceResolution.height * (1 - EDGE_PAD_RATIO);
  return Number(left) + Number(right) + Number(top) + Number(bottom);
}

export function cornerCoverageCount(dets: Detection[], sourceResolution: SourceResolution): number {
  return bitCount(cornerCoverageMask(dets, sourceResolution));
}

export function cornerCoverageMask(dets: Detection[], sourceResolution: SourceResolution): number {
  if (!isValidResolution(sourceResolution)) return 0;
  const bounds = boundsForDetections(dets);
  if (!bounds) return 0;
  const left = bounds.minX <= sourceResolution.width * EDGE_PAD_RATIO;
  const right = bounds.maxX >= sourceResolution.width * (1 - EDGE_PAD_RATIO);
  const top = bounds.minY <= sourceResolution.height * EDGE_PAD_RATIO;
  const bottom = bounds.maxY >= sourceResolution.height * (1 - EDGE_PAD_RATIO);
  let mask = 0;
  if (left && top) mask |= 1;
  if (right && top) mask |= 2;
  if (left && bottom) mask |= 4;
  if (right && bottom) mask |= 8;
  return mask;
}

export function bitCount(mask: number): number {
  let count = 0;
  let value = mask >>> 0;
  while (value) {
    count += value & 1;
    value >>>= 1;
  }
  return count;
}

export function averageSkewRatio(dets: Detection[]): number {
  if (!dets.length) return 1;
  let sum = 0;
  let count = 0;
  for (const det of dets) {
    const corners = det.corners ?? [];
    if (corners.length < 4) continue;
    const lengths = sideLengths(corners);
    if (lengths.length < 4) continue;
    const min = Math.min(...lengths);
    const max = Math.max(...lengths);
    if (max <= 0) continue;
    sum += min / max;
    count += 1;
  }
  return count === 0 ? 1 : sum / count;
}

function sizeScoreFromRatio(ratio: number): number {
  if (!Number.isFinite(ratio) || ratio <= 0) return 0;
  if (ratio < MIN_AREA_RATIO) return clamp(ratio / MIN_AREA_RATIO, 0, 1);
  if (ratio > MAX_AREA_RATIO) return clamp(MAX_AREA_RATIO / ratio, 0, 1);
  return 1;
}

export function computeGuidanceStatus(dets: Detection[], sourceResolution: SourceResolution): GuidanceStatus {
  const tags = dets.length;
  if (tags === 0) return { label: 'No tags detected', tone: 'bad' };
  const extentRatio = detectionExtentRatio(dets, sourceResolution);
  const far = extentRatio > 0 && extentRatio < FAR_EXTENT_RATIO * 0.8;
  const close = extentRatio > CLOSE_EXTENT_RATIO * 1.2;
  if (far) return { label: 'Move closer', tone: 'warn' };
  if (close) return { label: 'Move farther', tone: 'warn' };
  if (tags < MIN_TAGS) return { label: 'Need more tags in view', tone: 'warn' };
  return { label: 'Capture a snapshot', tone: 'good' };
}

export function scoreSnapshot(dets: Detection[], avgArea: number, coverageRatio: number, edges: number): number {
  const tagScore = clamp(dets.length / TARGET_TAGS, 0, 1);
  const sizeScore = sizeScoreFromRatio(avgArea);
  const coverageScore = clamp(coverageRatio / Math.max(MIN_FRAME_COVERAGE, 0.01), 0, 1);
  const edgeScore = clamp(edges / 4, 0, 1);
  return clamp(tagScore * 0.35 + sizeScore * 0.3 + coverageScore * 0.25 + edgeScore * 0.1, 0, 1);
}

export function statusColor(tone: GuidanceStatus['tone']): { fill: string; stroke: string; text: string } {
  if (tone === 'good') {
    return { fill: 'rgba(24, 180, 90, 0.75)', stroke: 'rgba(140, 255, 185, 0.8)', text: 'rgba(255,255,255,0.98)' };
  }
  if (tone === 'warn') {
    return { fill: 'rgba(245, 158, 11, 0.8)', stroke: 'rgba(255, 226, 140, 0.8)', text: 'rgba(18,18,18,0.95)' };
  }
  return { fill: 'rgba(239, 68, 68, 0.85)', stroke: 'rgba(255, 150, 150, 0.85)', text: 'rgba(255,255,255,0.98)' };
}
