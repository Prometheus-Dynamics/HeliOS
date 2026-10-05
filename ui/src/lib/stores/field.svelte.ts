// The field: its size and where every tag is. Imports and exports WPILib's
// AprilTag field layout JSON, so robot code and HeliOS share one file.

import { loadRaw, save } from "$lib/core/persist";
import { toasts } from "./toasts.svelte";

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

/** A demo layout: tags around a 16.5 × 8.1 m field (not an official game layout). */
function demoLayout(): FieldLayout {
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
  for (const [cx, cy] of [[4.9, W / 2], [L - 4.9, W / 2]]) {
    add(cx - 0.6, cy, 0.31, 180);
    add(cx + 0.6, cy, 0.31, 0);
    add(cx, cy - 0.6, 0.31, -90);
    add(cx, cy + 0.6, 0.31, 90);
  }
  return { name: "Demo field", length: L, width: W, tagSize: 0.1651, family: "36h11", tags };
}

class FieldStore {
  layout = $state<FieldLayout>(loadRaw("field", demoLayout()));
  /** Where the robot is, for previews: metres, degrees. */
  robotPose = $state({ x: 3.2, y: 2.4, heading: 20 });

  persist() {
    save("field", this.layout);
  }

  update(id: number, patch: Partial<FieldTag>) {
    const t = this.layout.tags.find((x) => x.id === id);
    if (!t) return;
    Object.assign(t, patch);
    this.persist();
  }

  add(x: number, y: number): number {
    const id = Math.max(0, ...this.layout.tags.map((t) => t.id)) + 1;
    this.layout.tags.push({ id, x: +x.toFixed(3), y: +y.toFixed(3), z: 0.5, yaw: 0, pitch: 0 });
    this.persist();
    return id;
  }

  remove(id: number) {
    this.layout.tags = this.layout.tags.filter((t) => t.id !== id);
    this.persist();
  }

  resetDemo() {
    this.layout = demoLayout();
    this.persist();
  }

  toWpilib() {
    const q = (yaw: number, pitch: number) => {
      const cy = Math.cos((yaw * Math.PI) / 360);
      const sy = Math.sin((yaw * Math.PI) / 360);
      const cp = Math.cos((pitch * Math.PI) / 360);
      const sp = Math.sin((pitch * Math.PI) / 360);
      return { W: cy * cp, X: -sy * sp, Y: cy * sp, Z: sy * cp };
    };
    return {
      tags: this.layout.tags.map((t) => ({ ID: t.id, pose: { translation: { x: t.x, y: t.y, z: t.z }, rotation: { quaternion: q(t.yaw, t.pitch) } } })),
      field: { length: this.layout.length, width: this.layout.width },
    };
  }

  fromWpilib(doc: unknown, name: string): boolean {
    const d = doc as { tags?: { ID: number; pose: { translation: { x: number; y: number; z: number }; rotation: { quaternion: { W: number; X: number; Y: number; Z: number } } } }[]; field?: { length: number; width: number } };
    if (!Array.isArray(d?.tags) || !d.field) {
      toasts.error("That file is not a WPILib AprilTag field layout");
      return false;
    }
    const tags = d.tags.map((t) => {
      const { W, X, Y, Z } = t.pose.rotation.quaternion;
      const yaw = (Math.atan2(2 * (W * Z + X * Y), 1 - 2 * (Y * Y + Z * Z)) * 180) / Math.PI;
      const pitch = (Math.asin(Math.max(-1, Math.min(1, 2 * (W * Y - Z * X)))) * 180) / Math.PI;
      return { id: t.ID, x: t.pose.translation.x, y: t.pose.translation.y, z: t.pose.translation.z, yaw: +yaw.toFixed(2), pitch: +pitch.toFixed(2) };
    });
    this.layout = { ...this.layout, name, length: d.field.length, width: d.field.width, tags };
    this.persist();
    toasts.success(`Loaded ${tags.length} tags from ${name}`);
    return true;
  }
}

export const field = new FieldStore();
