import { invertTransform, yawDegreesToQuaternion, type PoseTransform, type Vec3 } from './poseMath';

export type FrcFieldOrigin = 'center' | 'blue' | 'red';

export type PlanarFieldOrigin = {
  id: string;
  name: string;
  x: number;
  z: number;
  yawDeg: number;
};

export type FieldDimensions = {
  width: number;
  depth: number;
};

function originPoseInFieldCenter(origin: PlanarFieldOrigin): PoseTransform {
  const position: Vec3 = [origin.x, 0, origin.z];
  const quaternion = yawDegreesToQuaternion(origin.yawDeg);
  return { position, quaternion };
}

export function transformFromFieldCenter(origin: PlanarFieldOrigin): PoseTransform {
  return invertTransform(originPoseInFieldCenter(origin));
}

export function frcOriginDefinition(origin: FrcFieldOrigin, field: FieldDimensions): PlanarFieldOrigin {
  const halfWidth = field.width / 2;
  const halfDepth = field.depth / 2;
  if (origin === 'blue') {
    return { id: 'blue', name: 'Blue origin', x: -halfWidth, z: -halfDepth, yawDeg: 0 };
  }
  if (origin === 'red') {
    return { id: 'red', name: 'Red origin', x: halfWidth, z: halfDepth, yawDeg: 180 };
  }
  return { id: 'center', name: 'Center origin', x: 0, z: 0, yawDeg: 0 };
}
