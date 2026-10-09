// Guidance on top of the stores: what each camera is doing (its "job"),
// the setup checklist, and plain-language answers for the command palette.
// Everything resolves to the same screens, panes and settings experts use.

import type { Camera, Workload } from "#lib/api/model.js";
import { identity } from "#lib/core/identity.svelte.js";
import { tableFor, templateGraph } from "#lib/graph/graph.js";
import type { IconName } from "#lib/ui/icons.js";
import { calibration } from "#lib/stores/calibration.svelte.js";
import { cluster } from "#lib/stores/cluster.svelte.js";
import { field } from "#lib/stores/field.svelte.js";
import { robot } from "#lib/stores/robot.svelte.js";
import { toasts } from "#lib/stores/toasts.svelte.js";

export type Job = "apriltag" | "aruco" | "view";

export const JOBS: { id: Job; title: string; detail: string; icon: IconName }[] = [
  { id: "apriltag", title: "Find AprilTags", detail: "Works out where the robot is on the field from the tags on the walls.", icon: "target" },
  { id: "aruco", title: "Find ArUco markers", detail: "Spots small printed markers, for game pieces or practice targets.", icon: "tag" },
  { id: "view", title: "Just show video", detail: "No detection. Use it as a driver camera.", icon: "eye" },
];

function dictionaryOf(w: Workload): string {
  const d = w.graph.nodes.find((n) => n.type === "eidos:aruco.decode");
  return String(d?.params.dictionary ?? "");
}

class GuideStore {
  workloadsFor(c: Camera): Workload[] {
    return cluster.workloads.filter((w) => w.bindings.camera === c.resourceId);
  }

  /** The pipeline doing this camera's job right now, if any. */
  active(c: Camera): Workload | undefined {
    return this.workloadsFor(c).find((w) => w.state === "running" || w.state === "starting");
  }

  job(c: Camera): Job {
    const w = this.active(c);
    if (!w) return "view";
    return dictionaryOf(w).startsWith("apriltag") ? "apriltag" : "aruco";
  }

  /** Something is wrong with this camera, in words. */
  problem(c: Camera): string | null {
    const bad = this.workloadsFor(c).find((w) => w.state === "quarantined");
    if (bad) return `${bad.name} kept crashing and was stopped`;
    if (cluster.node(c.nodeId)?.health === "offline") return "Its device is offline";
    if (c.stats.fps < c.settings.fps * 0.8) return "Running slower than it should";
    return null;
  }

  async setJob(c: Camera, job: Job) {
    if (c.foreign) {
      toasts.info(`${c.name} is run by ${c.foreign}; change its job there`);
      return;
    }
    const mine = this.workloadsFor(c);
    for (const w of mine) if (w.state === "running" || w.state === "starting") await cluster.stopWorkload(w.id);
    if (job === "view") return;
    const family = job === "apriltag" ? "apriltag" : "aruco";
    const existing = mine.find((w) => w.state !== "quarantined" && dictionaryOf(w).startsWith(family));
    if (existing) {
      await cluster.restartWorkload(existing.id);
      return;
    }
    const name = `${job === "apriltag" ? "AprilTags" : "ArUco"} · ${identity.name(c.resourceId, c.name)}`;
    await cluster.createWorkload(name, c.nodeId, c.resourceId, templateGraph(job, tableFor(c.nodeId)));
  }

  /** Where this camera's results go for robot code. */
  table(c: Camera): string | null {
    const w = this.active(c);
    const nt = w?.graph.nodes.find((n) => n.type === "helios:nt4");
    return nt ? String(nt.params.table) : null;
  }

  calibrated(c: Camera) {
    return Boolean(calibration.saved[calibration.key(c)]);
  }

  get nativeCameras(): Camera[] {
    return cluster.cameras.filter((c) => !c.foreign);
  }

  /** The setup checklist, in the order a new team should do it. */
  get checklist(): { id: string; title: string; detail: string; done: boolean; icon: IconName }[] {
    const cams = this.nativeCameras;
    // Built-in defaults give icons, not names; a name means someone chose it.
    const named = cams.every((c) => identity.get(c.resourceId).name);
    const placed = cams.every((c) => robot.isCustom(c.resourceId));
    const cal = cams.filter((c) => this.calibrated(c)).length;
    return [
      { id: "name", title: "Name your cameras", detail: named ? "Every camera has a name and colour." : "Give each camera a name and colour so you can tell them apart.", done: named, icon: "tag" },
      { id: "place", title: "Tell HeliOS where each camera is", detail: placed ? "All cameras are placed on the robot." : "Where it sits on the robot and which way it looks. Needed for positions.", done: placed, icon: "cube" },
      { id: "calibrate", title: "Calibrate each camera", detail: `${cal} of ${cams.length} done. About two minutes each with a printed board.`, done: cal === cams.length, icon: "grid-4x4" },
      { id: "field", title: "Pick this year's field", detail: field.layout.name === "Demo field" ? "Using the demo field. Load the official layout file." : `Using “${field.layout.name}”.`, done: field.layout.name !== "Demo field", icon: "map" },
      { id: "robot-code", title: "Connect to robot code", detail: "Results are published on NetworkTables. Check the names match your code.", done: cams.every((c) => this.job(c) === "view" || this.table(c)), icon: "plug" },
    ];
  }
}

export const guide = new GuideStore();
