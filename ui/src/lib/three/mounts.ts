// Camera mounts on the robot, and the maths that turns a tag pose seen by a
// camera (OpenCV camera frame: x right, y down, z forward) into the robot
// frame (x forward, y left, z up, metres, origin at the chassis centre on the
// ground). The scene puts the robot frame into three.js's y-up world.

import * as THREE from "three";
import type { Camera, TagPose } from "$lib/api/model";

/** AprilTag 36h11 FRC tag edge, metres. */
export const TAG_SIZE = 0.1651;
/** Horizontal field of view and aspect of the mounted cameras (OV9782, 1280x800). */
export const CAMERA_HFOV_DEG = 80;
export const CAMERA_ASPECT = 16 / 10;

export interface Mount {
  camera: Camera;
  /** Position on the robot, robot frame, metres. */
  position: THREE.Vector3;
  yawDeg: number;
  pitchDeg: number;
  rollDeg: number;
  color: string;
  /** Camera frame -> robot frame. */
  matrix: THREE.Matrix4;
}

/** Where a camera sits on the robot: robot frame, metres and degrees. */
export interface Placement {
  pos: [number, number, number];
  yaw: number;
  pitch: number;
  roll: number;
  /** CAD part the camera is attached to, when placed by picking. */
  part?: string;
}

const PLACEMENT: Record<string, Placement> = {
  front: { pos: [0.4, 0, 0.32], yaw: 0, pitch: 18, roll: 0 },
  back: { pos: [-0.4, 0, 0.36], yaw: 180, pitch: 15, roll: 0 },
  left: { pos: [0.05, 0.4, 0.34], yaw: 90, pitch: 15, roll: 0 },
  right: { pos: [-0.05, -0.4, 0.34], yaw: -90, pitch: 20, roll: 0 },
};

/** A first guess from the camera's name and mount text, or null. */
export function defaultPlacement(camera: Camera): Placement | null {
  if (camera.foreign) return null;
  const text = `${camera.nodeId} ${camera.name}`.toLowerCase();
  const key = Object.keys(PLACEMENT).find((k) => text.includes(k));
  if (!key) return null;
  const tilt = /(\d+(?:\.\d+)?)\s*°\s*up/i.exec(camera.mount);
  return { ...PLACEMENT[key], pos: [...PLACEMENT[key].pos], pitch: tilt ? Number(tilt[1]) : PLACEMENT[key].pitch };
}

/** Camera frame -> robot frame for a camera at `pos`, yawed, tilted up and rolled. */
export function mountMatrix(pos: THREE.Vector3, yawDeg: number, pitchDeg: number, rollDeg = 0): THREE.Matrix4 {
  const yaw = THREE.MathUtils.degToRad(yawDeg);
  const pitch = THREE.MathUtils.degToRad(pitchDeg);
  const forward = new THREE.Vector3(Math.cos(pitch) * Math.cos(yaw), Math.cos(pitch) * Math.sin(yaw), Math.sin(pitch));
  const right = new THREE.Vector3(Math.sin(yaw), -Math.cos(yaw), 0);
  if (rollDeg) right.applyAxisAngle(forward, THREE.MathUtils.degToRad(rollDeg));
  const down = new THREE.Vector3().crossVectors(forward, right);
  return new THREE.Matrix4().makeBasis(right, down, forward).setPosition(pos);
}

export function mountsFor(cameras: Camera[], placements: Record<string, Placement>, colorOf: (id: string) => string): Mount[] {
  const mounts: Mount[] = [];
  for (const camera of cameras) {
    const place = placements[camera.resourceId];
    if (!place) continue;
    const position = new THREE.Vector3(...place.pos);
    mounts.push({
      camera,
      position,
      yawDeg: place.yaw,
      pitchDeg: place.pitch,
      rollDeg: place.roll,
      color: colorOf(camera.resourceId),
      matrix: mountMatrix(position, place.yaw, place.pitch, place.roll),
    });
  }
  return mounts;
}

export interface PlacedTag {
  /** Unique per camera + tag id. */
  key: string;
  id: number;
  cameraId: string;
  cameraName: string;
  color: string;
  /** Tag frame -> robot frame. */
  matrix: THREE.Matrix4;
  /** Tag centre, robot frame. */
  position: THREE.Vector3;
  /** Straight-line distance from the camera, metres. */
  distance: number;
  /** Bearing from the robot centre, degrees; 0 ahead, positive to the left. */
  bearing: number;
}

const axis = new THREE.Vector3();

/** Tag frame -> camera frame from an OpenCV rvec/tvec. */
export function poseMatrix(pose: TagPose): THREE.Matrix4 {
  const [rx, ry, rz] = pose.r;
  const angle = Math.hypot(rx, ry, rz);
  const m = new THREE.Matrix4();
  if (angle > 1e-9) m.makeRotationAxis(axis.set(rx / angle, ry / angle, rz / angle), angle);
  return m.setPosition(pose.t[0], pose.t[1], pose.t[2]);
}

export function placeTag(mount: Mount, pose: TagPose): PlacedTag {
  const matrix = new THREE.Matrix4().multiplyMatrices(mount.matrix, poseMatrix(pose));
  const position = new THREE.Vector3().setFromMatrixPosition(matrix);
  return {
    key: `${mount.camera.resourceId}#${pose.id}`,
    id: pose.id,
    cameraId: mount.camera.resourceId,
    cameraName: mount.camera.name,
    color: mount.color,
    matrix,
    position,
    distance: Math.hypot(...pose.t),
    bearing: THREE.MathUtils.radToDeg(Math.atan2(position.y, position.x)),
  };
}
