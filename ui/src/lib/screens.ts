// The built-in screens: one per job (docs/docs/ui-information-architecture.md). Each is a workspace
// preset: rearrange it and your version is kept; "Reset" brings this one back. Every control is on
// exactly one of them.

import { col, pane, row, stack, workspaces, type LayoutNode, type Workspace } from "#lib/core/workspace.svelte.js";
import type { IconName } from "#lib/ui/icons.js";

function ws(id: string, name: string, icon: IconName, color: number, summary: string, root: LayoutNode): () => Workspace {
  return () => ({ id, name, icon, color, summary, root });
}

/** The four jobs, in rail order. */
export const JOBS = ["setup", "run", "tune", "device"] as const;

export function registerScreens() {
  workspaces.registerPreset(
    "setup",
    ws("setup", "Set up", "list-check", 7, "First run, or a new camera or board: the checklist, what each camera runs, calibration, where it sits, the field.",
      row([0.34, 0.66],
        stack(pane("setup")),
        stack(pane("camera-jobs"), pane("calibration"), pane("mounts"), pane("robot3d"), pane("field"), pane("board")),
      ),
    ),
  );
  workspaces.registerPreset(
    "run",
    ws("run", "Run", "eye", 6, "Match time: every camera with its tags and status, the robot on the field, and health. Nothing to edit.",
      row([0.64, 0.36],
        stack(pane("mosaic")),
        col([0.52, 0.48], stack(pane("map")), stack(pane("status"))),
      ),
    ),
  );
  workspaces.registerPreset(
    "tune",
    ws("tune", "Tune", "adjustments-horizontal", 4, "One camera or pipeline at a time: camera controls, pipeline settings, the graph with its plan and timings.",
      row([0.18, 0.52, 0.3],
        stack(pane("cameras"), pane("pipelines"), pane("catalog")),
        col([0.56, 0.44], stack(pane("camera"), pane("playback")), stack(pane("graph"), pane("profiler"))),
        stack(pane("camera-controls"), pane("pipeline-settings")),
      ),
    ),
  );
  workspaces.registerPreset(
    "device",
    ws("device", "Device", "cpu", 1, "Boards and this browser: software and updates, hardware, processes, logs, recordings, robot link and security, look and backup.",
      col([0.34, 0.66],
        stack(pane("devices")),
        row([0.5, 0.5], stack(pane("system"), pane("peripherals"), pane("processes"), pane("logs")), stack(pane("robot"), pane("recordings"), pane("preferences"))),
      ),
    ),
  );
  workspaces.prune();
}

/** Prebuilt groups of panes you can drop into any screen. */
export const KITS: { id: string; title: string; summary: string; icon: IconName; make: () => LayoutNode }[] = [
  { id: "perception", title: "Perception", summary: "The camera view with its depth, the 3D scene of the field and the map, and the depth.", icon: "eye", make: () => row([0.36, 0.38, 0.26], stack(pane("camera", { overlays: { depth: true } })), stack(pane("map")), stack(pane("depth"))) },
  { id: "tune-camera", title: "Tune a camera", summary: "Live view with ROI and histogram, next to every camera setting.", icon: "camera", make: () => row([0.6, 0.4], stack(pane("camera")), stack(pane("camera-controls"))) },
  { id: "edit-pipeline", title: "Edit a pipeline", summary: "Node catalog, graph canvas and the pipeline's settings.", icon: "schema", make: () => row([0.2, 0.55, 0.25], stack(pane("catalog")), stack(pane("graph")), stack(pane("pipeline-settings"))) },
  { id: "performance", title: "Performance", summary: "Per-node timings and the processes and cores behind them.", icon: "stopwatch", make: () => col([0.5, 0.5], stack(pane("profiler")), stack(pane("processes"))) },
  { id: "geometry", title: "Robot geometry", summary: "3D robot with camera fields of view, and the mounts table.", icon: "cube", make: () => col([0.62, 0.38], stack(pane("robot3d")), stack(pane("mounts"))) },
  { id: "field", title: "Field", summary: "Field layout editor and what each camera can see from where the robot is.", icon: "map", make: () => stack(pane("field")) },
  { id: "calibrate", title: "Calibrate", summary: "The calibration wizard and the board maker.", icon: "grid-4x4", make: () => stack(pane("calibration"), pane("board")) },
  { id: "health", title: "Health", summary: "Problems with fixes, every device's load, and the logs.", icon: "heartbeat", make: () => col([0.5, 0.5], stack(pane("status")), stack(pane("logs"))) },
  { id: "hardware", title: "Device hardware", summary: "Fan, LEDs, GPIO, IMU, power, updates and recovery for one device.", icon: "plug", make: () => row([0.5, 0.5], stack(pane("peripherals")), stack(pane("system"))) },
];
