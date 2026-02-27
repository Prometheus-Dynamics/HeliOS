export type QuaternionWxyz = { w: number; x: number; y: number; z: number };

export type Vec3 = { x: number; y: number; z: number };

// Basis conversion between backend IMU frame and Three.js viewer frame.
//
// Backend (fusion + payloads): +X forward, +Y right, +Z up.
// Three.js viewer:             +X right,   +Y up,    +Z forward.
//
// This is a pure rotation (det=+1) that permutes axes:
// - backend +X -> three +Z
// - backend +Y -> three +X
// - backend +Z -> three +Y
//
// Equivalent to a -120° rotation about the (1,1,1) axis.
const BACKEND_TO_THREE: QuaternionWxyz = { w: 0.5, x: -0.5, y: -0.5, z: -0.5 };

export function quatConjugate(q: QuaternionWxyz): QuaternionWxyz {
  return { w: q.w, x: -q.x, y: -q.y, z: -q.z };
}

export function quatMultiply(a: QuaternionWxyz, b: QuaternionWxyz): QuaternionWxyz {
  return {
    w: a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z,
    x: a.w * b.x + a.x * b.w + a.y * b.z - a.z * b.y,
    y: a.w * b.y - a.x * b.z + a.y * b.w + a.z * b.x,
    z: a.w * b.z + a.x * b.y - a.y * b.x + a.z * b.w
  };
}

export function quatNormalize(q: QuaternionWxyz): QuaternionWxyz | null {
  const n2 = q.w * q.w + q.x * q.x + q.y * q.y + q.z * q.z;
  if (!Number.isFinite(n2) || n2 <= Number.EPSILON) return null;
  const inv = 1 / Math.sqrt(n2);
  return { w: q.w * inv, x: q.x * inv, y: q.y * inv, z: q.z * inv };
}

export function isFiniteQuaternion(q: QuaternionWxyz | null | undefined): q is QuaternionWxyz {
  return Boolean(q) && Number.isFinite(q.w) && Number.isFinite(q.x) && Number.isFinite(q.y) && Number.isFinite(q.z);
}

// Express the backend quaternion in the Three.js viewer basis (similarity transform).
export function imuQuaternionToThree(qBackend: QuaternionWxyz): QuaternionWxyz {
  const q = quatNormalize(qBackend) ?? qBackend;
  const c = BACKEND_TO_THREE;
  const out = quatMultiply(quatMultiply(c, q), quatConjugate(c));
  return quatNormalize(out) ?? out;
}

export function rotateVec3ByQuat(v: Vec3, q: QuaternionWxyz): Vec3 {
  // v' = q * v * q*
  const vq: QuaternionWxyz = { w: 0, x: v.x, y: v.y, z: v.z };
  const qc = quatConjugate(q);
  const rotated = quatMultiply(quatMultiply(q, vq), qc);
  return { x: rotated.x, y: rotated.y, z: rotated.z };
}

export function imuVec3ToThree(vBackend: Vec3): Vec3 {
  return rotateVec3ByQuat(vBackend, BACKEND_TO_THREE);
}

// Euler angles (degrees) matching Three.js usage in the IMU viewer:
// apply rotations in order X (pitch), Y (yaw), Z (roll) with order "XYZ".
export function quatToEulerXyzDeg(qWxyz: QuaternionWxyz): { x: number; y: number; z: number } {
  const q = quatNormalize(qWxyz) ?? qWxyz;
  const { w, x, y, z } = q;

  const r11 = 1 - 2 * (y * y + z * z);
  const r12 = 2 * (x * y - w * z);
  const r13 = 2 * (x * z + w * y);
  const r23 = 2 * (y * z - w * x);
  const r33 = 1 - 2 * (x * x + y * y);

  const clamp = (v: number) => Math.max(-1, Math.min(1, v));
  const yAngle = Math.asin(clamp(r13));
  const xAngle = Math.atan2(-r23, r33);
  const zAngle = Math.atan2(-r12, r11);

  const radToDeg = (r: number) => (r * 180) / Math.PI;
  return { x: radToDeg(xAngle), y: radToDeg(yAngle), z: radToDeg(zAngle) };
}

export function normalizeAngleDeg180(value: number): number {
  if (!Number.isFinite(value)) return 0;
  const wrapped = ((value + 180) % 360 + 360) % 360 - 180;
  return Object.is(wrapped, -180) ? 180 : wrapped;
}
