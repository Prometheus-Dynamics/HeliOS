// The built-in screens. Each is a workspace preset: rearrange it and your
// version is kept; "Reset" brings this one back. Start is the simple one new
// users begin with; the rest are added from the gallery when needed.

import { col, pane, row, stack, workspaces, type LayoutNode, type Workspace } from "$lib/core/workspace.svelte";
import type { IconName } from "$lib/ui/icons";

function ws(id: string, name: string, icon: IconName, color: number, summary: string, root: LayoutNode): () => Workspace {
  return () => ({ id, name, icon, color, summary, root });
}

export function registerScreens() {
  workspaces.registerPreset(
    "start",
    ws("start", "Start", "home", 7, "Every camera live, what still needs setting up, and anything that is wrong.",
      row([0.68, 0.32],
        stack(pane("mosaic")),
        col([0.55, 0.45], stack(pane("setup"), pane("status")), stack(pane("inspector"))),
      ),
    ),
  );
  workspaces.registerPreset(
    "overview",
    ws("overview", "Overview", "layout-grid", 8, "The whole robot: devices, cameras, pipelines, 3D and logs on one screen.",
      row([0.22, 0.5, 0.28],
        col([0.62, 0.38], stack(pane("status"), pane("setup")), stack(pane("inspector"))),
        col([0.64, 0.36], stack(pane("mosaic")), stack(pane("pipelines"), pane("streams"))),
        col([0.6, 0.4], stack(pane("robot3d"), pane("field")), stack(pane("logs"))),
      ),
    ),
  );
  workspaces.registerPreset(
    "cameras",
    ws("cameras", "Cameras", "camera", 4, "Tune one camera: live view with ROI and histogram, and every setting.",
      row([0.17, 0.55, 0.28],
        stack(pane("cameras")),
        col([0.72, 0.28], stack(pane("camera")), stack(pane("logs"))),
        stack(pane("camera-controls")),
      ),
    ),
  );
  workspaces.registerPreset(
    "vision",
    ws("vision", "Vision", "eye", 6, "A camera and its pipeline side by side, with timings: for tuning detection.",
      row([0.16, 0.56, 0.28],
        stack(pane("cameras")),
        col([0.6, 0.4], stack(pane("camera")), stack(pane("graph"))),
        col([0.62, 0.38], stack(pane("inspector"), pane("camera-controls")), stack(pane("profiler"))),
      ),
    ),
  );
  workspaces.registerPreset(
    "pipelines",
    ws("pipelines", "Pipelines", "schema", 10, "Edit pipeline graphs: node catalog, canvas, inspector, profiler.",
      row([0.15, 0.6, 0.25],
        stack(pane("catalog")),
        col([0.68, 0.32], stack(pane("graph")), stack(pane("pipelines"), pane("profiler"))),
        col([0.6, 0.4], stack(pane("inspector")), stack(pane("camera"))),
      ),
    ),
  );
  workspaces.registerPreset(
    "robot",
    ws("robot", "Robot", "cube", 3, "Where each camera is on the robot (numbers or CAD), and the field layout.",
      row([0.66, 0.34],
        col([0.64, 0.36], stack(pane("robot3d")), stack(pane("mounts"))),
        col([0.5, 0.5], stack(pane("field")), stack(pane("inspector"))),
      ),
    ),
  );
  workspaces.registerPreset(
    "calibration",
    ws("calibration", "Calibration", "grid-4x4", 2, "Calibrate cameras and print calibration boards.",
      row([0.75, 0.25], stack(pane("calibration"), pane("board")), col([0.5, 0.5], stack(pane("cameras")), stack(pane("camera-controls")))),
    ),
  );
  workspaces.registerPreset(
    "hardware",
    ws("hardware", "Hardware", "cpu", 1, "Every device: processes and cores, fan, LEDs, GPIO, IMU, updates and recovery.",
      col([0.36, 0.64],
        row([0.72, 0.28], stack(pane("devices")), stack(pane("inspector"))),
        row([0.42, 0.3, 0.28], stack(pane("processes")), stack(pane("peripherals")), stack(pane("system"), pane("logs"))),
      ),
    ),
  );
  workspaces.registerPreset("settings", ws("settings", "Settings", "settings", 9, "Security, theme, names and colours, layouts, backup.", stack(pane("settings"))));
}

/** Prebuilt groups of panes you can drop into any screen. */
export const KITS: { id: string; title: string; summary: string; icon: IconName; make: () => LayoutNode }[] = [
  { id: "tune-camera", title: "Tune a camera", summary: "Live view with ROI and histogram, next to every camera setting.", icon: "camera", make: () => row([0.6, 0.4], stack(pane("camera")), stack(pane("camera-controls"))) },
  { id: "edit-pipeline", title: "Edit a pipeline", summary: "Node catalog, graph canvas and inspector.", icon: "schema", make: () => row([0.2, 0.55, 0.25], stack(pane("catalog")), stack(pane("graph")), stack(pane("inspector"))) },
  { id: "performance", title: "Performance", summary: "Per-node timings and the processes and cores behind them.", icon: "stopwatch", make: () => col([0.5, 0.5], stack(pane("profiler")), stack(pane("processes"))) },
  { id: "geometry", title: "Robot geometry", summary: "3D robot with camera fields of view, and the mounts table.", icon: "cube", make: () => col([0.62, 0.38], stack(pane("robot3d")), stack(pane("mounts"))) },
  { id: "field", title: "Field", summary: "Field layout editor and what each camera can see from where the robot is.", icon: "map", make: () => stack(pane("field")) },
  { id: "calibrate", title: "Calibrate", summary: "The calibration wizard and the board maker.", icon: "grid-4x4", make: () => stack(pane("calibration"), pane("board")) },
  { id: "health", title: "Health", summary: "Problems with fixes, every device's load, and the logs.", icon: "heartbeat", make: () => col([0.5, 0.5], stack(pane("status")), stack(pane("logs"))) },
  { id: "hardware", title: "Device hardware", summary: "Fan, LEDs, GPIO, IMU, power, updates and recovery for one device.", icon: "plug", make: () => row([0.5, 0.5], stack(pane("peripherals")), stack(pane("system"))) },
];
