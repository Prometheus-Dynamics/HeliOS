// Plain-language entries for the palette: type the problem ("image too
// dark") or the goal ("calibrate") and land on the screen and control for it.

import { commands } from "#lib/core/commands.svelte.js";
import { selection } from "#lib/core/selection.svelte.js";
import { shell } from "#lib/core/shell.svelte.js";
import { pane, workspaces } from "#lib/core/workspace.svelte.js";
import type { IconName } from "#lib/ui/icons.js";
import { cluster } from "#lib/stores/cluster.svelte.js";

function camera() {
  if (!selection.last.camera && cluster.cameras[0]) selection.select({ kind: "camera", id: cluster.cameras[0].resourceId });
}
const go = (screen: string, then?: () => void) => () => {
  shell.go(screen);
  then?.();
};
const open = (type: string) => () => workspaces.open(shell.active, pane(type));

const INTENTS: { title: string; words: string; icon: IconName; run: () => void }[] = [
  { title: "Image too dark or too bright", words: "dark bright exposure gain light dim washed", icon: "sun", run: go("cameras", camera) },
  { title: "Image blurry when moving", words: "blur blurry motion smear exposure", icon: "focus-2", run: go("cameras", camera) },
  { title: "Change what a camera detects", words: "job apriltag aruco detect pipeline video off", icon: "target", run: go("cameras", camera) },
  { title: "Rename or recolour a camera", words: "name rename colour color icon label", icon: "pencil", run: go("cameras", camera) },
  { title: "Set a region of interest", words: "roi region crop", icon: "marquee-2", run: go("cameras", camera) },
  { title: "Calibrate a camera", words: "calibrate calibration lens distortion intrinsics board charuco", icon: "grid-4x4", run: go("calibration") },
  { title: "Print a calibration board", words: "board print charuco chessboard aprilgrid", icon: "printer", run: go("calibration", open("board")) },
  { title: "Set camera position on the robot", words: "mount position offset xyz roll pitch yaw extrinsics transform", icon: "ruler-2", run: go("robot") },
  { title: "Load robot CAD", words: "cad 3d model stl glb step obj", icon: "cube", run: go("robot") },
  { title: "Load the field layout", words: "field layout wpilib json tags map season", icon: "map", run: go("robot", open("field")) },
  { title: "NetworkTables names for robot code", words: "networktables nt4 table topic robot code", icon: "plug", run: go("pipelines") },
  { title: "Edit a pipeline graph", words: "graph node pipeline template edit", icon: "schema", run: go("pipelines") },
  { title: "Pipeline slow / where time goes", words: "slow latency profiler timing fps ms", icon: "stopwatch", run: go("pipelines", open("profiler")) },
  { title: "Processes, cores, kill or pin", words: "process kill cpu core pin priority nice memory", icon: "terminal-2", run: go("hardware") },
  { title: "Fan, LEDs, GPIO, IMU, power", words: "fan led gpio imu gyro power rail pins sensor", icon: "plug", run: go("hardware") },
  { title: "Install an update / roll back", words: "update upgrade install slot rollback version", icon: "cloud-download", run: go("hardware") },
  { title: "Something crashed or is stuck", words: "crash stuck broken restart reboot frozen quarantined error", icon: "lifebuoy", run: go("overview") },
  { title: "Setup checklist", words: "setup start new first checklist getting started", icon: "list-check", run: open("setup") },
  { title: "Secure this device / password / API tokens", words: "security secure password lock login sign token atlas open", icon: "lock", run: go("settings") },
  { title: "Theme, density, backup", words: "theme dark light density backup export import restore", icon: "settings", run: go("settings") },
];

export function registerIntents() {
  commands.register("intents", () => INTENTS.map((i, n) => ({ id: `how:${n}`, title: i.title, group: "How do I…", icon: i.icon, keywords: i.words, run: i.run })));
}
