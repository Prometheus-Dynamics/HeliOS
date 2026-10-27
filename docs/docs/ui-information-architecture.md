---
title: UI information architecture (by job)
description: How the HeliOS UI is organised: four jobs in the rail (Set up, Run, Tune, Device), one place per control, and the camera status chain as the guide everywhere.
---

**Status: implemented** (`ui/src/lib/screens.ts` is the source of truth for the layouts).

## Why

The previous UI was organised by *thing* (Cameras, Pipelines, Robot, Hardware, Calibration,
Recordings, Playback, Settings, plus Start, Overview, Vision and Perception). The same control
appeared in several of them (exposure in the Inspector and in Camera controls; what a camera runs in
the Inspector, in *What cameras run*, in Camera controls and in the first-run wizard; camera
placement in Mounts and in Camera controls; LEDs in Settings and in Peripherals), and nothing said
why tags were not showing, why the calibration bootstrap took so long, or why depth and features were
empty.

The UI is now organised by *job*. Every control lives in exactly one pane, every pane lives in
exactly one job, and every empty state names the first stage of the camera's status chain that is
not ok and what to do about it.

## The four jobs

| Rail | Job | Who | What is on it |
| --- | --- | --- | --- |
| **Set up** | First run; adding a camera or a board | The person building the robot | The per-camera checklist (status chain + name, job, calibration, placement), the field layout, robot code. Ends with tags visible and the camera placed. |
| **Run** | Match time | Drive team, pit crew | Every camera live with its tags and status strip, the robot's pose on the field, health with one-click recovery. Nothing editable. |
| **Tune** | One camera or one pipeline at a time | The vision person | Camera controls (sensor, exposure, white balance, flicker) separate from pipeline settings (inputs, detection, tuning by stage, output); the graph with its plan and per-node timing; playback. |
| **Device** | Boards and the browser | Whoever maintains the box | Devices, software (slots, updates, services, recovery), hardware (fan, LEDs, GPIO, IMU, power), processes, logs, recordings, robot link and security, and this browser's look, names and backups. Only what each board has. |

The rail shows these four, always, then any screens the user made (a copy of a job, or a blank one
with kits of panes). Number keys 1–4 switch jobs. There is no hiding of built-in screens any more:
four is few enough.

The rest of the shell is unchanged in kind: workspaces are layouts of panes (split, stack, drag,
maximize), the palette (Ctrl K) reaches every screen, pane, camera, pipeline and device, the status
bar shows the selection, problems and the next setup step, and the top bar's readouts jump to the
job that explains them (nodes → Device, pipelines and tick → Tune, tags → Run).

## One control, one place

| Control | Only here |
| --- | --- |
| Name, icon, colour of a camera | Set up › Cameras (the identity row of each camera) |
| What a camera runs (AprilTag / ArUco / none, 3D map, depth, features) | Set up › Cameras |
| Calibration | Set up › Calibration (and the board maker beside it) |
| Where a camera is on the robot | Set up › Mounts and Set up › Robot 3D (the same numbers) |
| Field layout | Set up › Field |
| Team number, NetworkTables | Device › Robot link |
| Exposure, gain, white balance, frame rate, flicker, sensor controls | Tune › Camera controls |
| Resolution, pyramid, tag family, search, per-stage tuning, output, core | Tune › Pipeline settings |
| The graph, templates, import/export, deploy, roll back | Tune › Graph |
| Start/stop/restart a pipeline, new custom pipeline | Tune › Pipelines |
| Reboot, safe mode, slots, updates, services, recovery | Device › Software |
| Fan, LED ring, GPIO, IMU, power | Device › Hardware |
| Recording settings, record now, capture last, stored recordings | Device › Recordings |
| Security (password, tokens) | Device › Robot link |
| Theme, density, accent, layouts, backup, keyboard | Device › This browser |

Removed because they duplicated one of the above: the **Inspector** (its camera quick controls, its
pipeline choice and its pipeline settings each had a home already; the graph node's settings are the
stage's section in Pipeline settings), **Settings** (split into Robot link and This browser),
**Streams** (the output path is in Pipeline settings; rates are in Pipelines), the standalone
**IMU** pane (Hardware has it), and the *Runs / Change* row and *Lens and placement* section of
Camera controls.

## Guidance: the status chain is the checklist

helios-api gives each camera a chain: camera › service › frames › pipeline › tags › calibration ›
map › depth › features, with the first stage that is `waiting` or `blocked`, its reason, a hint and
progress where there is some (frames toward a self-calibration window, keyframes toward depth).

- **Set up** lists each camera with the chain as its checklist. The stage that stops it is the
  next step, and its button opens the pane that fixes it (no pipeline → the job chooser; no tags
  → the family or the exposure; calibrating → it says how far; not placed → Mounts).
- **Run** draws the strip over every camera; it opens to the whole chain.
- **Tune**'s camera view has the same strip, so a change to exposure or the family shows its
  effect in the picture, the tag count and the strip at once.
- Every pane that has nothing to show says which stage is missing and what to do (the depth pane
  without depth, the 3D scene without a map, the profiler without a running pipeline), instead of a
  blank.

## Wireframes

Laptop (1366×768). Each box is a pane stack; `|` tabs.

```
SET UP
┌──────┬──────────────────────────┬────────────────────────────────────┐
│ rail │ Setup checklist          │ Cameras | Calibration | Mounts |   │
│      │                          │ Robot 3D | Field | Board maker     │
│ Set  │ Front  ●●●●○○○           │                                    │
│ up   │  next: Tags: no tags     │  Front  [identity] [AprilTag▾]     │
│      │        (family 16h5,     │         Field position · 3D map ·  │
│ Run  │        field is 36h11)   │         Depth           [Apply]    │
│      │        → Open pipeline   │                                    │
│ Tune │ Back   ●●○               │  Back   [identity] [None ▾]        │
│      │  next: Frames: in use    │  ...                               │
│ Dev  │        by visionserver   │                                    │
│      │                          │                                    │
│      │ Robot                    │                                    │
│      │  ○ Field layout: demo    │                                    │
│      │  ● Robot code: NT 10.x   │                                    │
└──────┴──────────────────────────┴────────────────────────────────────┘

RUN
┌──────┬──────────────────────────────────────────┬─────────────────────┐
│ rail │ All cameras (tiles, tags, status strip)  │ Pose (field, robot, │
│      │ ┌──────────┐ ┌──────────┐                │ cameras, map)       │
│      │ │ Front    │ │ Back     │                ├─────────────────────┤
│      │ │ 7 tags   │ │ ■ in use │                │ Health              │
│      │ └──────────┘ └──────────┘                │  ✓ no problems      │
│      │ ┌──────────┐ ┌──────────┐                │  devices: cpu/temp  │
│      │ │ Left     │ │ Right    │                │  cameras: fps/tags  │
│      │ └──────────┘ └──────────┘                │                     │
└──────┴──────────────────────────────────────────┴─────────────────────┘

TUNE
┌──────┬──────────┬───────────────────────────┬───────────────────────┐
│ rail │ Cameras |│ Camera (live, strip)      │ Camera controls |     │
│      │ Pipelines│                           │ Pipeline settings     │
│      │ | Nodes  │                           │                       │
│      │          ├───────────────────────────┤  Exposure ───────     │
│      │ Front 60 │ Graph | Profiler          │  Gain     ───────     │
│      │ Back  0  │ ┌cam┐→┌detect┐→┌nt┐       │  ...                  │
│      │          │  0.4ms  0.8ms   0.1ms     │                       │
│      │ AprilTag │ p50 1.3 ms · 1 transfer   │                       │
└──────┴──────────┴───────────────────────────┴───────────────────────┘

DEVICE
┌──────┬────────────────────────────────────────────────────────────────┐
│ rail │ Devices (health, link, cpu, mem, temp, clock, recovery)        │
│      ├──────────────────────────────┬─────────────────────────────────┤
│      │ Software | Hardware |        │ Robot link | Recordings |       │
│      │ Processes | Logs             │ This browser                    │
│      │  slots · updates · services  │  team · NT · security           │
└──────┴──────────────────────────────┴─────────────────────────────────┘
```

Phone-ish width (< 760 px): the rail becomes a bar along the bottom with the four jobs, the top
bar keeps only the screen name and the search, and a job's layout collapses to one stack of tabs
(every pane of the job is a tab), so the same panes work in the pit one at a time.

```
┌──────────────────────────────┐
│ Run                     ⌕    │
├──────────────────────────────┤
│ All cameras | Pose | Health  │
│ ┌──────────┐ ┌──────────┐    │
│ │ Front    │ │ Back     │    │
│ └──────────┘ └──────────┘    │
│                              │
├──────────────────────────────┤
│  Set up    Run   Tune  Device│
└──────────────────────────────┘
```

## The pipeline viewer

Tune › Graph is the one place a pipeline is read as a whole:

- the graph, each node with its live time and its share of the tick;
- the **plan** from `GET /v1/pipelines/{id}/plan`: every device transfer (a DeviceUpload or
  DeviceDownload adapter step, or an edge that crosses residency) is drawn on its edge and listed
  under the canvas, with a count in the header (AGENTS.md rule 4: check for round trips);
- the header line: state, revision, p50/p99, rate, share of the 1 ms budget;
- the Profiler tab for the same pipeline, node by node, sortable by graph order or time.

## First run

The first-run wizard asks only what nothing else can: the team number (unless the robot controller
gives it) and, on a device that has security, whether to leave it open. It then lands on **Set up**,
whose checklist carries the rest (names, jobs, calibration, placement, field, robot code). The
checklist stays in the status bar until it is done.

## What stayed the same

Per-device feature gating (`device.has`), the host integration for the team, live sync of settings
across tabs, one robot through the controller's gateway with per-node features, default pipelines
chosen per camera, bounded recording, themes, the palette and the keyboard: all kept, moved to the
job they belong to.
