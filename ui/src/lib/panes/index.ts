// Every pane this build provides, each on the job it belongs to (its `group`: the built-in screen it
// is found on). Adding a pane: write the component, add a line here; it appears in every "Add pane"
// menu and the palette, under its job.

import { identity } from "#lib/core/identity.svelte.js";
import { cluster } from "#lib/stores/cluster.svelte.js";
import { definePane } from "#lib/workspace/panes.js";
import BoardPane from "./BoardPane.svelte";
import CalibrationPane from "./CalibrationPane.svelte";
import CameraControlsPane from "./CameraControlsPane.svelte";
import CameraJobsPane from "./CameraJobsPane.svelte";
import CameraListPane from "./CameraListPane.svelte";
import CameraMosaicPane from "./CameraMosaicPane.svelte";
import CameraViewPane from "./CameraViewPane.svelte";
import CatalogPane from "./CatalogPane.svelte";
import DepthPane from "./DepthPane.svelte";
import DevicesPane from "./DevicesPane.svelte";
import FieldPane from "./FieldPane.svelte";
import GraphPane from "./GraphPane.svelte";
import LogsPane from "./LogsPane.svelte";
import MapPane from "./MapPane.svelte";
import MountsPane from "./MountsPane.svelte";
import PeripheralsPane from "./PeripheralsPane.svelte";
import PipelineSettingsPane from "./PipelineSettingsPane.svelte";
import PipelinesPane from "./PipelinesPane.svelte";
import PlaybackPane from "./PlaybackPane.svelte";
import PreferencesPane from "./PreferencesPane.svelte";
import ProcessesPane from "./ProcessesPane.svelte";
import ProfilerPane from "./ProfilerPane.svelte";
import RecordingsPane from "./RecordingsPane.svelte";
import Robot3DPane from "./Robot3DPane.svelte";
import RobotLinkPane from "./RobotLinkPane.svelte";
import SetupPane from "./SetupPane.svelte";
import StatusPane from "./StatusPane.svelte";
import SystemPane from "./SystemPane.svelte";
import WelcomePane from "./WelcomePane.svelte";

const pinnedCamera = (p: Record<string, unknown>) => (p.camera ? identity.name(p.camera as string, cluster.camera(p.camera as string)?.name ?? "") : null);
const pinnedWorkload = (p: Record<string, unknown>) => (p.workload ? identity.name(p.workload as string, cluster.workload(p.workload as string)?.name ?? "") : null);
const pinnedDevice = (p: Record<string, unknown>) => (p.device ? identity.name(p.device as string, cluster.node(p.device as string)?.name ?? "") : null);
const id = (k: string) => (p: Record<string, unknown>) => (p[k] as string) ?? null;

// Set up
definePane({ type: "setup", title: "Setup checklist", icon: "list-check", group: "setup", summary: "Each camera's status chain as a checklist, and what the robot still needs", component: SetupPane });
definePane({ type: "camera-jobs", title: "Cameras", icon: "camera", group: "setup", summary: "Name each camera and pick what it runs: localization, 3D map, depth, markers or feature tracks", component: CameraJobsPane });
definePane({ type: "calibration", title: "Calibration", icon: "grid-4x4", group: "setup", summary: "Guided camera calibration", component: CalibrationPane });
definePane({ type: "board", title: "Board maker", icon: "printer", group: "setup", summary: "Printable chessboard, ChArUco or AprilGrid", component: BoardPane });
definePane({ type: "mounts", title: "Mounts", icon: "ruler-2", group: "setup", summary: "Where each camera sits on the robot, as numbers, and the CAD model", component: MountsPane });
definePane({ type: "robot3d", title: "Robot 3D", icon: "cube", group: "setup", summary: "Cameras, fields of view, tags and your CAD; click a part to place a camera", component: Robot3DPane });
definePane({ type: "field", title: "Field", icon: "map", group: "setup", summary: "Field layout editor (WPILib JSON)", component: FieldPane });

// Run
definePane({ type: "mosaic", title: "All cameras", icon: "layout-grid", group: "run", summary: "Every camera at once with its tags and status strip, tiled to fit", component: CameraMosaicPane });
definePane({ type: "map", title: "Pose", icon: "route", group: "run", summary: "The robot on the field with its cameras and tags, and the live map's points, path, keyframes and mesh", component: MapPane, label: pinnedWorkload, identityOf: id("workload") });
definePane({ type: "status", title: "Health", icon: "heartbeat", group: "run", summary: "What needs attention with its fix, and every device's load", component: StatusPane });
definePane({ type: "depth", title: "Depth", icon: "scan", group: "run", summary: "The dense mapper's depth of the camera, colour-mapped, with its frame, age and work per keyframe", component: DepthPane, label: pinnedWorkload, identityOf: id("workload") });

// Tune
definePane({ type: "cameras", title: "Cameras", icon: "aperture", group: "tune", summary: "Every camera with its rate, tags and the stage that stops it; click to tune it", component: CameraListPane });
definePane({ type: "pipelines", title: "Pipelines", icon: "list-details", group: "tune", summary: "Every pipeline with its state and speed; start, stop, roll back, new", component: PipelinesPane });
definePane({ type: "catalog", title: "Nodes", icon: "box-multiple", group: "tune", summary: "Node types from the installed plugins, to drag into the graph", component: CatalogPane });
definePane({ type: "camera", title: "Camera", icon: "camera", group: "tune", summary: "One live camera with overlays, ROI and histogram", component: CameraViewPane, label: pinnedCamera, identityOf: id("camera") });
definePane({ type: "playback", title: "Playback", icon: "player-play", group: "tune", summary: "Replay a recording or image folder as a camera, with transport and pipeline runs", component: PlaybackPane });
definePane({ type: "graph", title: "Graph", icon: "schema", group: "tune", summary: "The pipeline's node graph with its plan (device transfers) and per-node timing", component: GraphPane, label: pinnedWorkload, identityOf: id("workload") });
definePane({ type: "profiler", title: "Profiler", icon: "stopwatch", group: "tune", summary: "Per-node timing against the frame budget", component: ProfilerPane, label: pinnedWorkload, identityOf: id("workload") });
definePane({ type: "camera-controls", title: "Camera controls", icon: "adjustments-horizontal", group: "tune", summary: "The camera's own settings: sensor, exposure, white balance, flicker", component: CameraControlsPane, label: (p) => (pinnedCamera(p) ? `${pinnedCamera(p)} controls` : null), identityOf: id("camera") });
definePane({ type: "pipeline-settings", title: "Pipeline settings", icon: "settings-automation", group: "tune", summary: "The pipeline's inputs, detection, tuning by stage, output and core", component: PipelineSettingsPane, label: (p) => (pinnedWorkload(p) ? `${pinnedWorkload(p)} settings` : null), identityOf: id("workload") });

// Device
definePane({ type: "devices", title: "Devices", icon: "server", group: "device", summary: "Every device with live load, link, clock and recovery", component: DevicesPane });
definePane({ type: "system", title: "Software", icon: "versions", group: "device", summary: "Boot slots, updates, services, recovery, reboot", component: SystemPane, label: pinnedDevice, identityOf: id("device") });
definePane({ type: "peripherals", title: "Hardware", icon: "plug", group: "device", summary: "Fan, LED ring, GPIO, IMU, power", component: PeripheralsPane, label: pinnedDevice, identityOf: id("device") });
definePane({ type: "processes", title: "Processes", icon: "terminal-2", group: "device", summary: "Cores and processes: pause, kill, pin", component: ProcessesPane, label: pinnedDevice, identityOf: id("device") });
definePane({ type: "logs", title: "Logs", icon: "file-text", group: "device", summary: "Logs from every device", component: LogsPane });
definePane({ type: "robot", title: "Robot link", icon: "plug-connected", group: "device", summary: "Team number and NetworkTables for robot code, and who may change this device", component: RobotLinkPane });
definePane({ type: "recordings", title: "Recordings", icon: "player-record", group: "device", summary: "Record a camera, capture the last seconds, the recorder's limits, stored recordings", component: RecordingsPane });
definePane({ type: "preferences", title: "This browser", icon: "settings", group: "device", summary: "Theme, density, names and colours, layouts, backup, keyboard", component: PreferencesPane });

definePane({ type: "welcome", title: "Empty", icon: "plus", group: "tune", summary: "Pick a pane for this spot", component: WelcomePane });
