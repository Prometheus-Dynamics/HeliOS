---
title: Running HeliOS in the simulator
description: Run the HeliOS services on an x86 PC with simulated cameras, and check them with a smoke test.
---

The simulator runs the real HeliOS stack on your PC, without a board: `orion-node`,
`helios-peripherals`, `helios-engine` and `helios-api`, with the UI served by `helios-api` on
the same port as on the device. Cameras are Styx's simulated sensors behind the real camera
service, so discovery, pipelines and the preview run unchanged. It is tier 1 of the fast test
pipeline: function, not timing. Timing is measured on the board.

## What you need

- Linux x86_64, Rust (the repo's `rust-toolchain.toml`), `git`, `curl`.
- `bun` for the UI build and the smoke test (<https://bun.sh>). Without it the API runs with
  no UI, and `tools/sim/smoke.sh` does not run.
- Network access on the first run (crates, the Orion and Styx git sources, `bun install`).

No lemnosd, no camera, no kernel modules: the services run without them.

## Start it

```sh
tools/sim/up.sh
```

The first run builds the four services with the dev profile, enables Styx's `sim` feature for
`helios-peripherals` only, and installs `orion-node` from the Orion commit `backend/Cargo.toml`
pins. It then starts the services in that order, on a temporary runtime directory (sockets,
state, logs; printed at start), and waits until `GET /v1/health` answers.

```text
  HeliOS sim is up.
  UI and API:  http://127.0.0.1:5800/
  Cameras:     ov9782,scene=gltf,path=<run dir>/scene.glb,lens=fisheye,...
  Smoke test:  tools/sim/smoke.sh (SIM_URL=http://127.0.0.1:5800)
```

Open the UI at the printed address. Ctrl-C stops the services the script started (by PID, with
SIGTERM, then SIGKILL after 15 s). If one of them exits, the script stops the rest and exits
non-zero; its log is in the runtime directory.

Useful settings (environment variables):

| Variable | Default | What it does |
|---|---|---|
| `STYX_SIM_CAMERAS` | the default scene (below) | Simulated cameras, one spec per `;`. See Styx's `docs/simulation.md` (faults, recordings, chart and flat scenes). |
| `SIM_BIND` | `127.0.0.1:5800` | helios-api's address. |
| `SIM_RUN_DIR` | a new `mktemp` directory | Runtime directory to use (and keep). |
| `SIM_SKIP_BUILD=1` | off | Run prebuilt binaries: no cargo, no bun. Set `SIM_BIN_DIR` and `ORION_NODE_BIN` to them. |
| `SIM_UI_DIR` | `ui/build` | The UI's static build. |
| `SIM_JOBS` | `8` | Cargo jobs. |

Two simulated cameras: `STYX_SIM_CAMERAS="ov9782,scene=chart;ov9782,scene=flat,noise=off"`.

## The default scene

The default camera renders a glTF scene, built at start by `helios-sim-scene` (a dev-only crate in
`backend/src/helios/sim-scene`, never part of a board image). It places the four AprilTags of
`tools/sim/scenarios/default/layout.json` (ids 1 to 4, 36h11, FRC size) on a wall 3 m ahead, with
a floor, and puts the camera at `camera.json`'s pose, seen through the Raze OV9782 lens of
`tools/sim/ov9782-calibration.json`. Styx renders the scene's `.glb` (`scene=gltf`) and writes each
frame's true camera pose and tag corners to `truth.jsonl`, which the smoke test scores detections
against. The builder takes any WPILib field layout and a camera pose or trajectory, so
`tools/sim/scenario.sh` uses it for each scenario under `tools/sim/scenarios/`.
`SIM_SCENE_LAYOUT`, `SIM_SCENE_CAMERA` and `SIM_SCENE_CALIBRATION` replace the default's files.

## Check it

With the sim running, in another shell:

```sh
tools/sim/smoke.sh
```

It checks, in order:

1. the API answers;
2. the simulated camera appears (a camera whose name says `simulated`, with a frame service);
3. an AprilTag pipeline is created from `GET /v1/pipelines/templates` on that camera;
4. the pipeline ticks (at least 20) with no failed ticks;
5. `GET /v1/live/ws` delivers the pipeline's outputs, each with `frame_ts_ns`;
6. the camera's preview WebSocket (`/v1/cameras/{id}/preview/ws`, Styx's `SPV1` frames) carries
   capture times that match live outputs' `frame_ts_ns`, so each preview frame can be drawn with
   the outputs of its own frame.

It prints a PASS or FAIL line per check with its time, the timings, and a summary, and exits
non-zero on any failure. It replaces its own pipeline (`sim-smoke-apriltag`) on each run.
Settings: `SIM_URL`, `SIM_SMOKE_TIMEOUT_S` (60), `SIM_SMOKE_WINDOW_MS` (4000).

The `scene=chart` picture has no AprilTags, so the detector ticks and publishes no detections.
Check 5 and 6 then prove the plumbing, not a detection.

## Notes

- The sim is not the board: no kernel, no sensor drivers, no LEDs, no IMU, no fan, and no
  lemnosd. Those are checked on the board (or, later, by the Gaia sim image).
- The sim's runtime paths are the temporary directory's; the board's `/etc` and `/var` paths
  are not used.
- `tools/sim/up.sh` is the first step of the Docker sim image (`sim-x86_64`): the same binaries,
  environment and checks, in a container.
