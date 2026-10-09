// The robot's geometry: where each camera is mounted, and an optional CAD
// model to place them on. Placements persist (on the device's camera mounts
// when live, in the browser with mocks); the model is loaded from a file you
// pick (GLB, glTF, STL, OBJ, PLY or 3MF) and kept for the session.

import type * as THREE from "three";
import { api, errorText } from "#lib/api/client.js";
import type { CameraMount } from "#lib/api/types.js";
import { loadRaw, save } from "#lib/core/persist.js";
import { defaultPlacement, type Placement } from "#lib/three/mounts.js";
import { cluster } from "./cluster.svelte";
import { liveState } from "./live.svelte";
import { toasts } from "./toasts.svelte";

/** Device mounts use the robot-frame convention of exportTransforms (pitch positive down). */
function toMount(p: Placement): CameraMount {
  return { x: p.pos[0], y: p.pos[1], z: p.pos[2], roll: p.roll, pitch: -p.pitch, yaw: p.yaw };
}

function fromMount(m: CameraMount): Placement {
  return { pos: [m.x, m.y, m.z], roll: m.roll, pitch: -m.pitch, yaw: m.yaw };
}

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

  #pushTimers = new Map<string, number>();

  placement(cameraId: string): Placement | null {
    if (cluster.mock) {
      if (this.#placements[cameraId]) return this.#placements[cameraId];
    } else if (liveState.mounts[cameraId]) {
      return fromMount(liveState.mounts[cameraId]);
    }
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
    return cluster.mock ? Boolean(this.#placements[cameraId]) : Boolean(liveState.mounts[cameraId]);
  }

  /** Store a mount on the device (debounced while dragging). The CAD part is browser-only. */
  #push(cameraId: string, mount: CameraMount | null) {
    clearTimeout(this.#pushTimers.get(cameraId));
    this.#pushTimers.set(
      cameraId,
      window.setTimeout(async () => {
        try {
          if (mount) await api.setMount(cameraId, mount);
          else await api.clearMount(cameraId);
        } catch (error) {
          toasts.error(`Camera mount not saved: ${errorText(error)}`);
        }
      }, 400),
    );
  }

  set(cameraId: string, patch: Partial<Placement>) {
    const base = this.placement(cameraId) ?? { pos: [0, 0, 0.3], yaw: 0, pitch: 15, roll: 0 };
    const next: Placement = { ...base, ...patch, pos: [...(patch.pos ?? base.pos)] as [number, number, number] };
    if (cluster.mock) {
      this.#placements[cameraId] = next;
      save("placements", this.#placements);
      return;
    }
    liveState.mounts[cameraId] = toMount(next);
    this.#push(cameraId, liveState.mounts[cameraId]);
  }

  reset(cameraId: string) {
    if (cluster.mock) {
      delete this.#placements[cameraId];
      save("placements", this.#placements);
      return;
    }
    delete liveState.mounts[cameraId];
    this.#push(cameraId, null);
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
      const { loadModelFile } = await import("#lib/three/loaders.js");
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
