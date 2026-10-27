// Plain-language entries for the palette: type the problem ("image too
// dark") or the goal ("calibrate") and land on the job and pane for it.

import { commands } from "#lib/core/commands.svelte.js";
import { selection } from "#lib/core/selection.svelte.js";
import { shell } from "#lib/core/shell.svelte.js";
import type { FeatureName } from "#lib/api/types.js";
import type { IconName } from "#lib/ui/icons.js";
import { cluster } from "#lib/stores/cluster.svelte.js";

function camera() {
  if (!selection.last.camera && cluster.cameras[0]) selection.select({ kind: "camera", id: cluster.cameras[0].resourceId });
}
const open = (screen: string, type: string, then?: () => void) => () => {
  shell.open(screen, type);
  then?.();
};

const INTENTS: { title: string; words: string; icon: IconName; run: () => void; requires?: FeatureName }[] = [
  { title: "Image too dark or too bright", words: "dark bright exposure gain light dim washed", icon: "sun", run: open("tune", "camera-controls", camera) },
  { title: "Image blurry when moving", words: "blur blurry motion smear exposure", icon: "focus-2", run: open("tune", "camera-controls", camera) },
  { title: "No tags showing / change what a camera detects", words: "tags job apriltag aruco detect pipeline video off family", icon: "target", run: open("setup", "camera-jobs", camera) },
  { title: "Rename or recolour a camera", words: "name rename colour color icon label", icon: "pencil", run: open("setup", "camera-jobs", camera) },
  { title: "Set a region of interest", words: "roi region crop", icon: "marquee-2", run: open("tune", "camera", camera) },
  { title: "Calibrate a camera", words: "calibrate calibration lens distortion intrinsics board charuco", icon: "grid-4x4", run: open("setup", "calibration") },
  { title: "Print a calibration board", words: "board print charuco chessboard aprilgrid", icon: "printer", run: open("setup", "board") },
  { title: "Set camera position on the robot", words: "mount position offset xyz roll pitch yaw extrinsics transform place", icon: "ruler-2", run: open("setup", "mounts") },
  { title: "Load robot CAD", words: "cad 3d model stl glb step obj", icon: "cube", run: open("setup", "robot3d") },
  { title: "Load the field layout", words: "field layout wpilib json tags map season", icon: "map", run: open("setup", "field") },
  { title: "Team number / NetworkTables for robot code", words: "team networktables nt4 table topic robot code", icon: "plug", run: open("device", "robot") },
  { title: "Edit a pipeline graph", words: "graph node pipeline template edit plan transfer", icon: "schema", run: open("tune", "graph") },
  { title: "Pipeline slow / where time goes", words: "slow latency profiler timing fps ms budget", icon: "stopwatch", run: open("tune", "profiler") },
  { title: "Tune detection (resolution, family, stages)", words: "resolution pyramid search tune settings stage", icon: "adjustments-horizontal", run: open("tune", "pipeline-settings") },
  { title: "Record a camera / capture the last seconds", words: "record recording capture ring mcap export", icon: "player-record", run: open("device", "recordings") },
  { title: "Replay a recording", words: "playback replay recording image folder", icon: "player-play", run: open("tune", "playback") },
  { title: "Processes, cores, kill or pin", words: "process kill cpu core pin priority nice memory", icon: "terminal-2", run: open("device", "processes") },
  { title: "Fan, LEDs, GPIO, IMU, power", words: "fan led gpio imu gyro power rail pins sensor ring", icon: "plug", run: open("device", "peripherals") },
  { title: "Install an update / roll back", words: "update upgrade install slot rollback version", icon: "cloud-download", run: open("device", "system"), requires: "updates" },
  { title: "Something crashed or is stuck", words: "crash stuck broken restart reboot frozen quarantined error problem", icon: "lifebuoy", run: open("run", "status") },
  { title: "Setup checklist", words: "setup start new first checklist getting started", icon: "list-check", run: open("setup", "setup") },
  { title: "Secure this device / password / API tokens", words: "security secure password lock login sign token atlas open", icon: "lock", run: open("device", "robot"), requires: "security" },
  { title: "Theme, density, backup", words: "theme dark light density backup export import restore layout", icon: "settings", run: open("device", "preferences") },
];

export function registerIntents() {
  commands.register("intents", () => INTENTS.map((i, n) => ({ id: `how:${n}`, title: i.title, group: "How do I…", icon: i.icon, keywords: i.words, requires: i.requires, run: i.run })));
}
