// The field's layout as the UI edits it, and its conversions to and from the device's field
// layouts (`/v1/field-layouts`, WPILib's AprilTag field JSON). Pure: no state here.

import type { FieldLayoutDocument } from "#lib/api/types.js";

export interface FieldTag {
  id: number;
  x: number;
  y: number;
  z: number;
  /** Facing direction on the field, degrees CCW from +x. */
  yaw: number;
  pitch: number;
}

export interface FieldLayout {
  name: string;
  length: number;
  width: number;
  tagSize: number;
  family: string;
  tags: FieldTag[];
}

/** WPILib's AprilTag field layout JSON. */
export interface WpilibFieldLayout {
  tags: { ID: number; pose: { translation: { x: number; y: number; z: number }; rotation: { quaternion: { W: number; X: number; Y: number; Z: number } } } }[];
  field: { length: number; width: number };
}

export const DEMO_FIELD_NAME = "Demo field";

/** A demo layout: tags around a 16.5 × 8.1 m field (not an official game layout). */
export function demoLayout(): FieldLayout {
  const L = 16.54;
  const W = 8.07;
  const tags: FieldTag[] = [];
  let id = 1;
  const add = (x: number, y: number, z: number, yaw: number) => tags.push({ id: id++, x: +x.toFixed(3), y: +y.toFixed(3), z, yaw, pitch: 0 });
  // Driver-station walls, both ends.
  for (const y of [1.2, 2.6, 5.5, 6.9]) add(0.02, y, 1.35, 0);
  for (const y of [1.2, 2.6, 5.5, 6.9]) add(L - 0.02, y, 1.35, 180);
  // Side walls.
  for (const x of [4.1, 8.27, 12.4]) add(x, 0.02, 1.2, 90);
  for (const x of [4.1, 8.27, 12.4]) add(x, W - 0.02, 1.2, -90);
  // Two central structures, four faces each.
  for (const [cx, cy] of [
    [4.9, W / 2],
    [L - 4.9, W / 2],
  ]) {
    add(cx - 0.6, cy, 0.31, 180);
    add(cx + 0.6, cy, 0.31, 0);
    add(cx, cy - 0.6, 0.31, -90);
    add(cx, cy + 0.6, 0.31, 90);
  }
  return { name: DEMO_FIELD_NAME, length: L, width: W, tagSize: 0.1651, family: "36h11", tags };
}

/** A tag's rotation for a yaw and pitch (degrees), as WPILib's quaternion {W, X, Y, Z}. */
export function quaternionOf(yaw: number, pitch: number) {
  const cy = Math.cos((yaw * Math.PI) / 360);
  const sy = Math.sin((yaw * Math.PI) / 360);
  const cp = Math.cos((pitch * Math.PI) / 360);
  const sp = Math.sin((pitch * Math.PI) / 360);
  return { W: cy * cp, X: -sy * sp, Y: cy * sp, Z: sy * cp };
}

/** The yaw and pitch (degrees) of a tag's rotation quaternion. */
export function anglesOf(q: { W: number; X: number; Y: number; Z: number }): { yaw: number; pitch: number } {
  const yaw = (Math.atan2(2 * (q.W * q.Z + q.X * q.Y), 1 - 2 * (q.Y * q.Y + q.Z * q.Z)) * 180) / Math.PI;
  const pitch = (Math.asin(Math.max(-1, Math.min(1, 2 * (q.W * q.Y - q.Z * q.X)))) * 180) / Math.PI;
  return { yaw: +yaw.toFixed(2), pitch: +pitch.toFixed(2) };
}

/** The layout as WPILib's AprilTag field JSON (what the device stores and robot code reads). */
export function layoutToWpilib(layout: FieldLayout): WpilibFieldLayout {
  return {
    tags: layout.tags.map((t) => ({ ID: t.id, pose: { translation: { x: t.x, y: t.y, z: t.z }, rotation: { quaternion: quaternionOf(t.yaw, t.pitch) } } })),
    field: { length: layout.length, width: layout.width },
  };
}

/** A device layout (`GET /v1/field-layouts/{id}`) as the UI edits it. */
export function layoutFromDocument(doc: FieldLayoutDocument): FieldLayout {
  const tags = doc.layout.tags.map((t) => {
    const { yaw, pitch } = anglesOf({ W: t.field_from_tag.rotation.w, X: t.field_from_tag.rotation.x, Y: t.field_from_tag.rotation.y, Z: t.field_from_tag.rotation.z });
    const { x, y, z } = t.field_from_tag.translation;
    return { id: t.id, x, y, z, yaw, pitch };
  });
  return { name: doc.name, length: doc.layout.length_m, width: doc.layout.width_m, tagSize: doc.layout.tags[0]?.side_m ?? 0.1651, family: "36h11", tags };
}

/** WPILib's field JSON as a device layout document (the simulated device stores them this way). */
export function documentFromWpilib(id: string, name: string, json: WpilibFieldLayout, tagSide: number): FieldLayoutDocument {
  return {
    id,
    name,
    layout: {
      length_m: json.field.length,
      width_m: json.field.width,
      tags: json.tags.map((t) => ({
        id: t.ID,
        side_m: tagSide,
        field_from_tag: { translation: t.pose.translation, rotation: { w: t.pose.rotation.quaternion.W, x: t.pose.rotation.quaternion.X, y: t.pose.rotation.quaternion.Y, z: t.pose.rotation.quaternion.Z } },
      })),
    },
  };
}

/** The demo layout as a device layout document. */
export function demoDocument(id: string): FieldLayoutDocument {
  return documentFromWpilib(id, DEMO_FIELD_NAME, layoutToWpilib(demoLayout()), demoLayout().tagSize);
}
