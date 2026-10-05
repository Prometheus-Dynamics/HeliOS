// The robot's geometry: where each camera is mounted, and an optional CAD
// model to place them on. Placements persist; the model is loaded from a
// file you pick (GLB, glTF, STL, OBJ, PLY or 3MF) and kept for the session.

import type * as THREE from "three";
import { loadRaw, save } from "$lib/core/persist";
import { defaultPlacement, type Placement } from "$lib/three/mounts";
import { cluster } from "./cluster.svelte";
import { toasts } from "./toasts.svelte";

export interface ModelInfo {
  name: string;
  /** Size of one model unit, metres (0.001 for millimetres). */
  unit: number;
  up: "z" | "y";
  /** Robot-frame offset applied after units and up-axis, metres. */
  offset: [number, number, number];
  yaw: number;
  parts: number;
  triangles: number;
}

export const UNITS = [
  { value: 0.001, label: "mm" },
  { value: 0.01, label: "cm" },
  { value: 0.0254, label: "in" },
  { value: 1, label: "m" },
];

class RobotStore {
  #placements = $state<Record<string, Placement>>(loadRaw("placements", {}));
  model = $state.raw<THREE.Object3D | null>(null);
  info = $state<ModelInfo | null>(null);
  /** The camera waiting for a click on the model, if any. */
  picking = $state<string | null>(null);

  placement(cameraId: string): Placement | null {
    if (this.#placements[cameraId]) return this.#placements[cameraId];
    const c = cluster.camera(cameraId);
    return c ? defaultPlacement(c) : null;
  }

  get placements(): Record<string, Placement> {
    const out: Record<string, Placement> = {};
    for (const c of cluster.cameras) {
      const p = this.placement(c.resourceId);
      if (p) out[c.resourceId] = p;
    }
    return out;
  }

  isCustom(cameraId: string) {
    return Boolean(this.#placements[cameraId]);
  }

  set(cameraId: string, patch: Partial<Placement>) {
    const base = this.placement(cameraId) ?? { pos: [0, 0, 0.3], yaw: 0, pitch: 15, roll: 0 };
    this.#placements[cameraId] = { ...base, ...patch, pos: [...(patch.pos ?? base.pos)] as [number, number, number] };
    save("placements", this.#placements);
  }

  reset(cameraId: string) {
    delete this.#placements[cameraId];
    save("placements", this.#placements);
  }

  /** WPILib-style robot-to-camera transforms for robot code. */
  exportTransforms() {
    return {
      format: "helios.robot-cameras",
      schema_version: 1,
      frame: "robot: x forward, y left, z up; metres, degrees",
      cameras: Object.entries(this.placements).map(([id, p]) => ({
        camera: id,
        translation: { x: p.pos[0], y: p.pos[1], z: p.pos[2] },
        rotation: { roll: p.roll, pitch: -p.pitch, yaw: p.yaw },
        part: p.part ?? null,
      })),
    };
  }

  async loadModel(file: File) {
    const ext = file.name.split(".").pop()?.toLowerCase() ?? "";
    if (ext === "step" || ext === "stp" || ext === "iges" || ext === "igs") {
      toasts.error("STEP/IGES: export the robot as GLB or STL from your CAD tool (Onshape, Fusion and SolidWorks all can)");
      return;
    }
    try {
      const { loadModelFile } = await import("$lib/three/loaders");
      const { object, parts, triangles, guessUnit } = await loadModelFile(file, ext);
      this.model = object;
      this.info = { name: file.name, unit: guessUnit, up: ext === "glb" || ext === "gltf" || ext === "obj" ? "y" : "z", offset: [0, 0, 0], yaw: 0, parts, triangles };
      toasts.success(`Loaded ${file.name}: ${parts} parts, ${(triangles / 1000).toFixed(0)}k triangles`);
    } catch (e) {
      toasts.error(`Could not read ${file.name}: ${(e as Error).message}`);
    }
  }

  clearModel() {
    this.model = null;
    this.info = null;
    this.picking = null;
  }
}

export const robot = new RobotStore();
