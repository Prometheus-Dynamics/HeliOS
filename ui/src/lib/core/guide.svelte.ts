// Guidance on top of the stores: what each camera is doing (its "job"), the setup checklist (each
// camera's status chain, plus what the robot needs), and plain-language answers for the command
// palette. Everything resolves to the same screens, panes and settings experts use.

import type { Camera, Workload } from "#lib/api/model.js";
import type { StatusStage } from "#lib/api/types.js";
import { identity } from "#lib/core/identity.svelte.js";
import type { IconName } from "#lib/ui/icons.js";
import { calibration } from "#lib/stores/calibration.svelte.js";
import { cluster } from "#lib/stores/cluster.svelte.js";
import { liveState } from "#lib/stores/live.svelte.js";
import { field } from "#lib/stores/field.svelte.js";
import { robot } from "#lib/stores/robot.svelte.js";

/** The kind of job a camera's pipeline does, coarsely (what a camera runs is chosen in
 * `camera/PipelineChoice.svelte`, from the default templates in `core/defaults.ts`). */
export type Job = "apriltag" | "aruco" | "view";

/** The tag families a job can use, most common first (FRC uses 36h11 since 2024, 16h5 before). */
export const DICTIONARIES: Record<"apriltag" | "aruco", { value: string; label: string }[]> = {
  apriltag: [
    { value: "apriltag_36h11", label: "36h11 · FRC 2024+" },
    { value: "apriltag_16h5", label: "16h5 · FRC ≤2023" },
    { value: "apriltag_25h9", label: "25h9" },
    { value: "apriltag_36h10", label: "36h10" },
  ],
  aruco: ["4x4", "5x5", "6x6", "7x7"].flatMap((size) => [50, 100, 250, 1000].map((n) => ({ value: `aruco_${size}_${n}`, label: `${size} · ${n}` }))),
};

/** Where a setup step is done: the job screen and the pane on it. */
export interface StepTarget {
  screen: "setup" | "tune" | "device";
  pane: string;
}

/**
 * One step of a camera's setup: `ok` is done, `waiting` is in progress (the chain's stage says how
 * far), `todo` is not started, `blocked` is stopped by something (the stage's reason says what).
 */
export interface CameraStep {
  id: string;
  title: string;
  detail: string;
  state: "ok" | "waiting" | "todo" | "blocked";
  hint: string | null;
  progress: StatusStage["progress"];
  target: StepTarget;
}

/** The dictionary a pipeline's detector decodes ("" without one). */
function dictionaryOf(w: Workload): string {
  const d = w.graph.nodes.find((n) => n.params.dictionary !== undefined);
  return String(d?.params.dictionary ?? "");
}

class GuideStore {
  workloadsFor(c: Camera): Workload[] {
    return cluster.workloads.filter((w) => Object.values(w.bindings).includes(c.resourceId));
  }

  /** The pipeline doing this camera's job right now, if any. */
  active(c: Camera): Workload | undefined {
    return this.workloadsFor(c).find((w) => w.state === "running" || w.state === "starting");
  }

  job(c: Camera): Job {
    const w = this.active(c);
    if (!w) return "view";
    const d = dictionaryOf(w);
    return d.startsWith("aruco") ? "aruco" : d ? "apriltag" : "view";
  }

  /** The tag family this camera's job decodes ("" with no detection job). */
  dictionary(c: Camera): string {
    const w = this.active(c);
    return w ? dictionaryOf(w) : "";
  }

  async setDictionary(c: Camera, dictionary: string) {
    const w = this.active(c);
    if (w && dictionaryOf(w) !== dictionary) await cluster.setDictionary(w.id, dictionary);
  }

  /** Something is wrong with this camera, in words. */
  problem(c: Camera): string | null {
    const bad = this.workloadsFor(c).find((w) => w.state === "quarantined");
    if (bad) return `${bad.name} kept crashing and was stopped`;
    if (cluster.node(c.nodeId)?.health === "offline") return "Its device is offline";
    if (c.stats.fps < c.settings.fps * 0.8) return "Running slower than it should";
    return null;
  }

  /** Where this camera's results go for robot code. */
  table(c: Camera): string | null {
    const w = this.active(c);
    // On a device the engine publishes every pipeline's outputs under <prefix>/<pipeline>.
    if (cluster.live) {
      const nt = liveState.nt4;
      return w && nt?.server ? `${nt.settings.prefix.replace(/\/$/, "")}/${w.id}` : null;
    }
    const nt = w?.graph.nodes.find((n) => n.type === "helios:nt4");
    return nt ? String(nt.params.table) : null;
  }

  calibrated(c: Camera) {
    return Boolean(calibration.saved[calibration.key(c)]) || c.status?.stages.some((s) => s.stage === "calibration" && s.state === "ok") === true;
  }

  get nativeCameras(): Camera[] {
    return cluster.cameras.filter((c) => !c.foreign);
  }

  /**
   * One camera's setup, in order, from its status chain (helios-api `camera_status`) and what this
   * browser knows (its name, its placement): the stage that stops the chain is the next step, with
   * the chain's reason, hint and progress, and the pane that does it.
   */
  cameraSteps(c: Camera): CameraStep[] {
    const stage = (id: string) => c.status?.stages.find((s) => s.stage === id);
    const stop = c.status?.blocking ?? null;
    const chainStep = (id: string, title: string, fallback: string, target: StepTarget, done: () => boolean): CameraStep => {
      const s = stage(id);
      if (s && s.state !== "off") {
        const state = s.state === "ok" ? "ok" : s.state === "blocked" ? "blocked" : s.stage === stop ? "waiting" : "todo";
        return { id, title, detail: s.reason, state, hint: s.state === "ok" ? null : s.hint, progress: s.progress, target };
      }
      return { id, title, detail: fallback, state: done() ? "ok" : "todo", hint: null, progress: null, target };
    };
    const steps: CameraStep[] = [];
    // The camera itself, its service and its frames: only when something stops them.
    const front = ["camera", "service", "frames"].map(stage).find((s) => s && s.state !== "ok" && s.state !== "off");
    if (front) steps.push({ id: front.stage, title: "Camera", detail: front.reason, state: front.state === "blocked" ? "blocked" : "waiting", hint: front.hint, progress: front.progress, target: { screen: "device", pane: "devices" } });
    steps.push(
      chainStep("pipeline", "Choose what it runs", this.active(c) ? "Runs its pipeline." : "Localization, 3D map, depth, markers or feature tracks.", { screen: "setup", pane: "camera-jobs" }, () => Boolean(this.active(c))),
      chainStep("detections", "See tags", cluster.detectionsFor(c).length ? `${cluster.detectionsFor(c).length} tags in view.` : "Point it at a tag: its outline and id show over the picture.", { screen: "tune", pane: "camera-controls" }, () => cluster.detectionsFor(c).length > 0),
      chainStep("calibration", "Calibrate", this.calibrated(c) ? "Calibrated." : "About two minutes with a printed board; the live map also fits the lens on its own.", { screen: "setup", pane: "calibration" }, () => this.calibrated(c)),
      { id: "place", title: "Place it on the robot", detail: robot.isCustom(c.resourceId) ? "Placed." : "Where it sits and which way it looks: needed for positions.", state: robot.isCustom(c.resourceId) ? "ok" : "todo", hint: null, progress: null, target: { screen: "setup", pane: "mounts" } },
    );
    for (const [id, title] of [["map", "3D map"], ["depth", "Depth"], ["features", "Feature tracks"]] as const) {
      const s = stage(id);
      if (s && s.state !== "off") steps.push(chainStep(id, title, s.reason, { screen: "setup", pane: "camera-jobs" }, () => s.state === "ok"));
    }
    // A step blocked by something before it (no tags without a pipeline) is not the next one.
    return steps;
  }

  /** The step a camera is at: the first that is not done. */
  nextFor(c: Camera): CameraStep | undefined {
    return this.cameraSteps(c).find((s) => s.state !== "ok");
  }

  /** The setup checklist, in the order a new team should do it (the status bar and the rail count it). */
  get checklist(): { id: string; title: string; detail: string; done: boolean; icon: IconName }[] {
    const cams = this.nativeCameras;
    const placed = cams.every((c) => robot.isCustom(c.resourceId));
    const cal = cams.filter((c) => this.calibrated(c)).length;
    const working = cams.filter((c) => this.active(c)).length;
    return [
      { id: "jobs", title: "Choose what each camera does", detail: working === cams.length ? "Every camera runs its pipeline." : `${working} of ${cams.length} run a pipeline. Pick localization, the 3D map and depth, markers or feature tracks.`, done: working === cams.length, icon: "list-check" },
      { id: "place", title: "Tell HeliOS where each camera is", detail: placed ? "All cameras are placed on the robot." : "Where it sits on the robot and which way it looks. Needed for positions.", done: placed, icon: "cube" },
      { id: "calibrate", title: "Calibrate each camera", detail: `${cal} of ${cams.length} done. About two minutes each with a printed board.`, done: cal === cams.length, icon: "grid-4x4" },
      { id: "field", title: "Pick this year's field", detail: field.layout.name === "Demo field" ? "Using the demo field. Load the official layout file." : `Using “${field.layout.name}”.`, done: field.layout.name !== "Demo field", icon: "map" },
      cluster.live
        ? {
            id: "robot-code",
            title: "Connect to robot code",
            detail: liveState.nt4?.connected ? `Publishing to ${liveState.nt4.server} under ${liveState.nt4.settings.prefix}.` : liveState.nt4?.server ? `Waiting for ${liveState.nt4.server}. Is the device on the robot network?` : "Set your team number in Device › Robot link.",
            done: Boolean(liveState.nt4?.connected),
            icon: "plug",
          }
        : { id: "robot-code", title: "Connect to robot code", detail: "Results are published on NetworkTables. Check the names match your code.", done: cams.every((c) => this.job(c) === "view" || this.table(c)), icon: "plug" },
    ];
  }
}

export const guide = new GuideStore();
