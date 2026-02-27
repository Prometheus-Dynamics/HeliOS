export type Vec3 = [number, number, number];

export type PoseQuaternion = {
  x: number;
  y: number;
  z: number;
  w: number;
};

export type PoseTransform = {
  position: Vec3;
  quaternion: PoseQuaternion;
};

export function normalizeQuaternion(quaternion: PoseQuaternion): PoseQuaternion {
  const { x, y, z, w } = quaternion;
  const mag = Math.sqrt(x * x + y * y + z * z + w * w);
  if (!Number.isFinite(mag) || mag < Number.EPSILON) {
    return { x: 0, y: 0, z: 0, w: 1 };
  }
  const inv = 1 / mag;
  return { x: x * inv, y: y * inv, z: z * inv, w: w * inv };
}

export function conjugateQuaternion(quaternion: PoseQuaternion): PoseQuaternion {
  return { x: -quaternion.x, y: -quaternion.y, z: -quaternion.z, w: quaternion.w };
}

// Quaternion multiplication representing composition: result = a * b.
export function multiplyQuaternions(a: PoseQuaternion, b: PoseQuaternion): PoseQuaternion {
  const ax = a.x;
  const ay = a.y;
  const az = a.z;
  const aw = a.w;
  const bx = b.x;
  const by = b.y;
  const bz = b.z;
  const bw = b.w;

  return {
    x: aw * bx + ax * bw + ay * bz - az * by,
    y: aw * by - ax * bz + ay * bw + az * bx,
    z: aw * bz + ax * by - ay * bx + az * bw,
    w: aw * bw - ax * bx - ay * by - az * bz
  };
}

export function rotateVectorByQuaternion(vector: Vec3, quaternion: PoseQuaternion): Vec3 {
  const q = normalizeQuaternion(quaternion);
  const x = vector[0];
  const y = vector[1];
  const z = vector[2];

  const qx = q.x;
  const qy = q.y;
  const qz = q.z;
  const qw = q.w;

  // q * v
  const ix = qw * x + qy * z - qz * y;
  const iy = qw * y + qz * x - qx * z;
  const iz = qw * z + qx * y - qy * x;
  const iw = -qx * x - qy * y - qz * z;

  // (q * v) * q^-1
  return [
    ix * qw + iw * -qx + iy * -qz - iz * -qy,
    iy * qw + iw * -qy + iz * -qx - ix * -qz,
    iz * qw + iw * -qz + ix * -qy - iy * -qx
  ];
}

function degToRad(degrees: number): number {
  return (degrees * Math.PI) / 180;
}

function radToDeg(radians: number): number {
  return (radians * 180) / Math.PI;
}

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}

/**
 * Builds a quaternion from Euler degrees using Three-style XYZ order:
 * - pitch around +X
 * - yaw around +Y
 * - roll around +Z
 */
export function eulerDegreesToQuaternionXYZ(rotation: { pitch: number; yaw: number; roll: number }): PoseQuaternion {
  const x = degToRad(rotation.pitch);
  const y = degToRad(rotation.yaw);
  const z = degToRad(rotation.roll);

  const cx = Math.cos(x / 2);
  const sx = Math.sin(x / 2);
  const cy = Math.cos(y / 2);
  const sy = Math.sin(y / 2);
  const cz = Math.cos(z / 2);
  const sz = Math.sin(z / 2);

  // Equivalent to qx * qy * qz for XYZ intrinsic rotations.
  return normalizeQuaternion({
    x: sx * cy * cz + cx * sy * sz,
    y: cx * sy * cz - sx * cy * sz,
    z: cx * cy * sz + sx * sy * cz,
    w: cx * cy * cz - sx * sy * sz
  });
}

export function yawDegreesToQuaternion(yawDegrees: number): PoseQuaternion {
  return eulerDegreesToQuaternionXYZ({ pitch: 0, yaw: yawDegrees, roll: 0 });
}

export function quaternionToEulerDegreesXYZ(quaternion: PoseQuaternion): { pitch: number; yaw: number; roll: number } {
  const q = normalizeQuaternion(quaternion);
  const x = q.x;
  const y = q.y;
  const z = q.z;
  const w = q.w;

  const x2 = x + x;
  const y2 = y + y;
  const z2 = z + z;

  const xx = x * x2;
  const xy = x * y2;
  const xz = x * z2;
  const yy = y * y2;
  const yz = y * z2;
  const zz = z * z2;
  const wx = w * x2;
  const wy = w * y2;
  const wz = w * z2;

  const m11 = 1 - (yy + zz);
  const m12 = xy - wz;
  const m13 = xz + wy;
  const m22 = 1 - (xx + zz);
  const m23 = yz - wx;
  const m32 = yz + wx;
  const m33 = 1 - (xx + yy);

  const yaw = Math.asin(clamp(m13, -1, 1));
  let pitch: number;
  let roll: number;

  if (Math.abs(m13) < 0.9999999) {
    pitch = Math.atan2(-m23, m33);
    roll = Math.atan2(-m12, m11);
  } else {
    pitch = Math.atan2(m32, m22);
    roll = 0;
  }

  return { pitch: radToDeg(pitch), yaw: radToDeg(yaw), roll: radToDeg(roll) };
}

export function invertTransform(transform: PoseTransform): PoseTransform {
  const quaternionInv = conjugateQuaternion(normalizeQuaternion(transform.quaternion));
  const rotated = rotateVectorByQuaternion(transform.position, quaternionInv);
  return {
    position: [-rotated[0], -rotated[1], -rotated[2]],
    quaternion: quaternionInv
  };
}

export function applyTransformToPoint(transform: PoseTransform, point: Vec3): Vec3 {
  const rotated = rotateVectorByQuaternion(point, transform.quaternion);
  return [rotated[0] + transform.position[0], rotated[1] + transform.position[1], rotated[2] + transform.position[2]];
}

// Compose transforms (parent_from_child * child_from_grandchild).
export function composeTransforms(parentFromChild: PoseTransform, childFromGrandchild: PoseTransform): PoseTransform {
  const position = applyTransformToPoint(parentFromChild, childFromGrandchild.position);
  const quaternion = normalizeQuaternion(multiplyQuaternions(parentFromChild.quaternion, childFromGrandchild.quaternion));
  return { position, quaternion };
}

export function identityTransform(): PoseTransform {
  return { position: [0, 0, 0], quaternion: { x: 0, y: 0, z: 0, w: 1 } };
}
