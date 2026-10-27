# UI end-to-end tests

Playwright (Chromium only) drives the real UI with clicks and typing, against two targets:

- **mock**: the UI's simulated robot (`?mock=1`) on the Vite dev server (port 5899, started by the
  Playwright config).
- **sim**: a running HeliOS sim (`tools/sim/up.sh`: the real orion-node, lemnosd, helios-peripherals
  with a simulated OV9782, helios-engine and helios-api, which serves this UI). Each UI action is
  checked on the device through `GET /v1/pipelines`, `GET /v1/pipelines/{id}`,
  `GET /v1/cameras/{id}/settings` and `GET /v1/pipelines/templates` (see `device.ts`).

## Running

```sh
cd ui && bun run e2e:mock            # the mock target
cd ui && bun run e2e:sim             # the sim target, against a sim that is already running
tools/sim/ui-e2e.sh                  # the sim target: starts the sim, runs it, always stops the sim
```

`tools/sim/ui-e2e.sh` starts `tools/sim/up.sh` with the prebuilt binaries, waits for helios-api,
builds the UI when `ui/build` is missing, runs the `sim` project, stops the sim by PID, and exits
non-zero when the suite fails. Its variables (`SIM_BIN_DIR`, `ORION_NODE_BIN`, `LEMNOSD_BIN_DIR`,
`SIM_UI_DIR`, `SIM_BIND`, `SIM_RUN_DIR`, `SIM_PLAYWRIGHT_ARGS`) default to the main checkout's
prebuilt binaries and to 127.0.0.1:5811; the header of the script lists them. Keep `SIM_RUN_DIR`
short (under `$XDG_RUNTIME_DIR`): the services bind Unix sockets there.

The mock target needs `ui/static/mock/robot/` (JPEG frames, detections and poses for the simulated
camera). It is dev data, gitignored and not shipped: generate it with `scripts/gen-mock.py` or copy
it from a checkout that has it. Without it the mock's camera frames are missing and tests fail on
404 console errors.

Chromium comes from `bunx playwright install chromium`.

## What runs

| File | Flow |
| --- | --- |
| `default-pipelines.spec.ts` | Set up → Cameras: AprilTag with the 3D map and depth, then ArUco, then video only. Sim: one enabled Eidos pipeline per camera, the same id throughout, and the graph has `live_map` and `dense.map` (then none). |
| `pipeline-settings.spec.ts` | Tune → Pipeline settings: Resolution and pyramid levels (binding `output_width`, `output_height`, `pyramid`); tag family, search mode and full-search interval (`dictionary`, `search_mode`, `full_search_every`). |
| `tuning.spec.ts` | Tuning, by stage: a tab-through of a float field changes nothing; one parameter is edited and deployed. Sim: the whole graph's const inputs, type and value, match before and after except the one edited input; `requires` and edges are unchanged. |
| `graph-editor.spec.ts` | Select a node (its settings show), Find centres it, Auto-arrange moves nodes and does not ask for a deploy. |
| `camera-controls.spec.ts` | Search filters the sections, Changed only, exposure and gain typed in manual mode and read back from the camera, reset. |
| `setup.spec.ts` | The checklist: a camera's next step opens the pane that does it; Set up → Cameras has each camera's chooser; the first-run wizard shows once and can be skipped. |
| `screens.spec.ts` | Each of the four jobs opens with its panes; the rail is the four jobs; the status bar jumps to Run and Set up; the add-a-screen gallery opens. |
| `device-features.spec.ts` | A device's own features (LED ring, security, reboot, boot slots, recovery ladder) show only where the device has them. Mock: the Raze image (all of them) and `?mock=systemcore`, a guest (none). Sim: the UI matches what `GET /v1/device` reports, so `HELIOS_E2E_URL` can point at a real device (read-only). |

Sim tests start from whatever the device holds (a previous run's pipeline, its tag family, its
camera settings), so each one sets the state it needs through the UI and then checks the device
with polls. Mock tests check the screen only.

## Conventions

- **Global check.** `fixtures.ts` fails a test on any console error or uncaught page error. The
  allow-list there is short and each entry says why. A test that expects one device condition adds
  it with `test.use({ allowConsole: [...] })`, in that test file, with the reason.
- **Locators.** Roles, labels and visible text. `.step` (the setup checklist row) and `.prop`-style
  lookups are the only class-based ones; `PipelineSettings.svelte` labels its choices so the tests
  do not need them.
- **Clicks.** Real clicks and typing. Typed values are committed with Enter, and a helper
  (`applyIfChanged` in `flows.ts`) waits for a chooser's write to finish before the device is read.
- **Writes land before a page closes.** The `page` fixture ends a test by waiting for
  `window.__helios.settled()` (the cluster store: no device write in flight, no control change
  waiting to be sent). A page closed with a PUT in flight once cut the device's handler off between
  its Orion write and its revision record, and the next test's deploy showed no revision change.
  `settleWrites(page)` (in `fixtures.ts`) is the same wait for a test that reloads or navigates.
- **The device as found (sim).** The automatic `deviceAsFound` fixture runs for every sim test. Before
  the test it waits for each frame camera to be ready (its service online, its frames flowing or
  not yet asked for; up to 3 minutes, failing with the stage that holds it up, so a device that has
  just booted is not tested too early), then snapshots the setup flag, every pipeline's spec, and each
  frame camera's kept settings and mode. After the page's last write it puts them back (`restoreDevice`
  in `device.ts`): a pipeline is rewritten, recreated or removed as needed, a camera's kept settings
  are reset and re-applied, and the restore waits until the device shows them. A camera that was
  delivering frames must deliver them again. A test does not clean up after itself for these.

## Not automated

- Real cameras, calibration boards and their captures; the LED ring and the GPIO, IMU and fan
  hardware; the 3D robot and the map pane's canvas (other tests cover their data).
- Security and sign-in flows (the sim runs open), updates and reboots, and the recordings' files.
- Pixel checks of the live views: the tests read the DOM and the device, not the picture.
