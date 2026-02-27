export type PaperChoice = 'auto' | 'letter' | 'a4' | 'custom';
export type OrientationChoice = 'auto' | 'portrait' | 'landscape';

const LETTER_W_MM = 215.9;
const LETTER_H_MM = 279.4;
const A4_W_MM = 210.0;
const A4_H_MM = 297.0;

function fitsWithin(requiredWmm: number, requiredHmm: number, pageWmm: number, pageHmm: number): boolean {
  return requiredWmm <= pageWmm + 1e-6 && requiredHmm <= pageHmm + 1e-6;
}

export function pickPaperDims(
  requiredWmm: number,
  requiredHmm: number,
  paper: PaperChoice,
  orientation: OrientationChoice
): {
  pageWmm: number;
  pageHmm: number;
  fits: boolean;
  resolvedPaper: Exclude<PaperChoice, 'auto'>;
  resolvedOrientation: Exclude<OrientationChoice, 'auto'>;
} {
  const pickOrientation = (
    portraitWmm: number,
    portraitHmm: number
  ): { pageWmm: number; pageHmm: number; fits: boolean; resolvedOrientation: Exclude<OrientationChoice, 'auto'> } => {
    if (orientation === 'portrait') {
      return {
        pageWmm: portraitWmm,
        pageHmm: portraitHmm,
        fits: fitsWithin(requiredWmm, requiredHmm, portraitWmm, portraitHmm),
        resolvedOrientation: 'portrait'
      };
    }
    if (orientation === 'landscape') {
      return {
        pageWmm: portraitHmm,
        pageHmm: portraitWmm,
        fits: fitsWithin(requiredWmm, requiredHmm, portraitHmm, portraitWmm),
        resolvedOrientation: 'landscape'
      };
    }

    if (fitsWithin(requiredWmm, requiredHmm, portraitWmm, portraitHmm)) {
      return { pageWmm: portraitWmm, pageHmm: portraitHmm, fits: true, resolvedOrientation: 'portrait' };
    }
    if (fitsWithin(requiredWmm, requiredHmm, portraitHmm, portraitWmm)) {
      return { pageWmm: portraitHmm, pageHmm: portraitWmm, fits: true, resolvedOrientation: 'landscape' };
    }
    return { pageWmm: portraitWmm, pageHmm: portraitHmm, fits: false, resolvedOrientation: 'portrait' };
  };

  if (paper === 'custom') {
    return { pageWmm: requiredWmm, pageHmm: requiredHmm, fits: true, resolvedPaper: 'custom', resolvedOrientation: 'portrait' };
  }

  if (paper === 'letter') {
    const picked = pickOrientation(LETTER_W_MM, LETTER_H_MM);
    return { ...picked, resolvedPaper: 'letter' };
  }

  if (paper === 'a4') {
    const picked = pickOrientation(A4_W_MM, A4_H_MM);
    return { ...picked, resolvedPaper: 'a4' };
  }

  const letter = pickOrientation(LETTER_W_MM, LETTER_H_MM);
  if (letter.fits) return { ...letter, resolvedPaper: 'letter' };
  const a4 = pickOrientation(A4_W_MM, A4_H_MM);
  if (a4.fits) return { ...a4, resolvedPaper: 'a4' };
  return { pageWmm: requiredWmm, pageHmm: requiredHmm, fits: true, resolvedPaper: 'custom', resolvedOrientation: 'portrait' };
}

export function radToDeg(rad: number): number {
  return (rad * 180) / Math.PI;
}

export function safeFovDeg(sensorPx: number, focalPx: number): number | null {
  if (!Number.isFinite(sensorPx) || sensorPx <= 0) return null;
  if (!Number.isFinite(focalPx) || focalPx <= 0) return null;
  return radToDeg(2 * Math.atan(sensorPx / (2 * focalPx)));
}

type LensModel = 'pinhole' | 'fisheye';

export type CalibrationIntrinsicsLike = {
  fx: number;
  fy: number;
  cx: number;
  cy: number;
  k1: number;
  k2: number;
  p1: number;
  p2: number;
  k3: number;
  lensModel?: LensModel;
};

export type FovEstimateDeg = {
  hfov: number | null;
  vfov: number | null;
  dfov: number | null;
};

function angleBetween(a: [number, number, number], b: [number, number, number]): number {
  const dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
  const aa = a[0] * a[0] + a[1] * a[1] + a[2] * a[2];
  const bb = b[0] * b[0] + b[1] * b[1] + b[2] * b[2];
  if (!aa || !bb) return 0;
  const cos = dot / Math.sqrt(aa * bb);
  // Numeric noise can push just outside [-1, 1].
  return Math.acos(Math.min(1, Math.max(-1, cos)));
}

function normalize3(v: [number, number, number]): [number, number, number] {
  const n = Math.hypot(v[0], v[1], v[2]);
  if (!n || !Number.isFinite(n)) return [0, 0, 1];
  return [v[0] / n, v[1] / n, v[2] / n];
}

function rayFromPixelPinhole(u: number, v: number, k: CalibrationIntrinsicsLike): [number, number, number] | null {
  if (!Number.isFinite(k.fx) || k.fx <= 0) return null;
  if (!Number.isFinite(k.fy) || k.fy <= 0) return null;
  const xd = (u - k.cx) / k.fx;
  const yd = (v - k.cy) / k.fy;

  const k1 = Number(k.k1 ?? 0);
  const k2 = Number(k.k2 ?? 0);
  const k3 = Number(k.k3 ?? 0);
  const p1 = Number(k.p1 ?? 0);
  const p2 = Number(k.p2 ?? 0);
  const hasDistortion =
    Math.abs(k1) > 1e-12 ||
    Math.abs(k2) > 1e-12 ||
    Math.abs(k3) > 1e-12 ||
    Math.abs(p1) > 1e-12 ||
    Math.abs(p2) > 1e-12;
  if (!hasDistortion) {
    return normalize3([xd, yd, 1]);
  }

  // Invert OpenCV pinhole distortion via fixed-point/Newton-style iteration.
  // This lets FOV estimation reflect the distorted/raw image geometry.
  let xu = xd;
  let yu = yd;
  for (let i = 0; i < 10; i++) {
    const x2 = xu * xu;
    const y2 = yu * yu;
    const r2 = x2 + y2;
    const r4 = r2 * r2;
    const r6 = r4 * r2;
    const radial = 1 + k1 * r2 + k2 * r4 + k3 * r6;
    if (!Number.isFinite(radial) || Math.abs(radial) < 1e-12) break;

    const twoXY = 2 * xu * yu;
    const deltaX = p1 * twoXY + p2 * (r2 + 2 * x2);
    const deltaY = p1 * (r2 + 2 * y2) + p2 * twoXY;
    const nextX = (xd - deltaX) / radial;
    const nextY = (yd - deltaY) / radial;
    if (!Number.isFinite(nextX) || !Number.isFinite(nextY)) break;

    const step = Math.abs(nextX - xu) + Math.abs(nextY - yu);
    xu = nextX;
    yu = nextY;
    if (step < 1e-12) break;
  }

  return normalize3([xu, yu, 1]);
}

// Inverse of OpenCV fisheye distortion (k4 assumed 0). This is only used for reporting FOV
// in the UI; the runtime undistort still uses the backend calibration pipeline.
function rayFromPixelFisheye(u: number, v: number, k: CalibrationIntrinsicsLike): [number, number, number] | null {
  if (!Number.isFinite(k.fx) || k.fx <= 0) return null;
  if (!Number.isFinite(k.fy) || k.fy <= 0) return null;

  const xd = (u - k.cx) / k.fx;
  const yd = (v - k.cy) / k.fy;
  const rd = Math.hypot(xd, yd);
  if (!Number.isFinite(rd)) return null;
  if (rd < 1e-12) return [0, 0, 1];

  const k1 = Number(k.k1 ?? 0);
  const k2 = Number(k.k2 ?? 0);
  const k3 = Number(k.k3 ?? 0);
  // OpenCV fisheye uses 4 coefficients; we only store 3 in StreamCalibration. Treat k4 as 0.
  const k4 = 0;

  const thetaD = rd;
  let theta = thetaD;
  // Newton iterations to solve: theta * (1 + k1*theta^2 + k2*theta^4 + k3*theta^6 + k4*theta^8) = thetaD
  for (let i = 0; i < 10; i++) {
    const t2 = theta * theta;
    const t4 = t2 * t2;
    const t6 = t4 * t2;
    const t8 = t4 * t4;
    const poly = 1 + k1 * t2 + k2 * t4 + k3 * t6 + k4 * t8;
    const f = theta * poly - thetaD;
    const df = 1 + 3 * k1 * t2 + 5 * k2 * t4 + 7 * k3 * t6 + 9 * k4 * t8;
    if (!Number.isFinite(f) || !Number.isFinite(df) || Math.abs(df) < 1e-12) break;
    const step = f / df;
    theta -= step;
    if (!Number.isFinite(theta)) break;
    // Prevent tan(theta) from exploding. Our lenses are < 180deg diagonal in practice.
    theta = Math.max(0, Math.min(theta, 1.55));
    if (Math.abs(step) < 1e-10) break;
  }

  const ru = Math.tan(theta);
  if (!Number.isFinite(ru)) return null;
  const scale = ru / rd;
  return normalize3([xd * scale, yd * scale, 1]);
}

export function estimateCalibrationFovDegs(
  sourceResolution: { width: number; height: number } | null,
  calibration: CalibrationIntrinsicsLike | null
): FovEstimateDeg {
  const w = Number(sourceResolution?.width ?? 0);
  const h = Number(sourceResolution?.height ?? 0);
  if (!Number.isFinite(w) || w <= 0 || !Number.isFinite(h) || h <= 0) return { hfov: null, vfov: null, dfov: null };
  if (!calibration) return { hfov: null, vfov: null, dfov: null };

  const model: LensModel = (calibration.lensModel ?? 'pinhole') as LensModel;
  const rayFromPixel = model === 'fisheye' ? rayFromPixelFisheye : rayFromPixelPinhole;

  const uL = 0.5;
  const uR = w - 0.5;
  const vT = 0.5;
  const vB = h - 0.5;
  const uM = w * 0.5;
  const vM = h * 0.5;

  const left = rayFromPixel(uL, vM, calibration);
  const right = rayFromPixel(uR, vM, calibration);
  const top = rayFromPixel(uM, vT, calibration);
  const bottom = rayFromPixel(uM, vB, calibration);
  const tl = rayFromPixel(uL, vT, calibration);
  const br = rayFromPixel(uR, vB, calibration);
  const tr = rayFromPixel(uR, vT, calibration);
  const bl = rayFromPixel(uL, vB, calibration);

  const hfov = left && right ? radToDeg(angleBetween(left, right)) : null;
  const vfov = top && bottom ? radToDeg(angleBetween(top, bottom)) : null;
  const d1 = tl && br ? radToDeg(angleBetween(tl, br)) : null;
  const d2 = tr && bl ? radToDeg(angleBetween(tr, bl)) : null;
  const dfov = d1 !== null && d2 !== null ? Math.max(d1, d2) : d1 ?? d2;

  // Sanity: return nulls instead of nonsense.
  const sane = (x: number | null): number | null => (x !== null && Number.isFinite(x) && x > 0 && x < 179.999 ? x : null);
  return { hfov: sane(hfov), vfov: sane(vfov), dfov: sane(dfov) };
}

export function formatMaybeNumber(value: number | null, digits = 2): string {
  if (value === null) return '—';
  if (!Number.isFinite(value)) return '—';
  return value.toFixed(digits);
}

export function overlayUrlForImage(
  overlayByImage: Map<string, string>,
  apiPath: (path: string) => string,
  imageName: string
): string | null {
  const overlayName = overlayByImage.get(imageName);
  if (overlayName) {
    return apiPath(`/media/${encodeURIComponent(overlayName)}`);
  }
  return null;
}
