# Changelog

This file is the aggregate release log for HeliOS images.

- One section per `VERSION_ID`
- New releases should be added at the top
- Detailed docs-site release notes live under `docs/src/pages/release-notes/`

## [Unreleased]

- Tag search, round 7 (k0g6, the user's choice): the stored templates and camera pipelines default to a full search every 8th frame, no escalation of a missed track to a full search (`loss_full_search_after` 0) and window growth over 6 misses (`margin_growth_misses` 6); both are in the pipeline spec and DTO. The tracked group is stage nodes in the plan (per-stage profiling). CM5, the recorded test video: 3463 of the 3531 tags a full search finds (98.1%) at 0.324 ms per frame, against about 0.76 ms p50 (0.94 ms p99) for full search every frame; a new tag is found at most 7 frames after it appears (Eidos 3546efbf9).
- Pins (round 6, one source each): Daedalus dev c927136 (schedule order by node index; per-instance state, resources and metrics, so `ExecutionContext::node_id` is an `id@label` instance key when ids repeat; HeliOS reads no node id from the context), Styx dev 4e8e19c, Atlas dev 3175ad9 (`gaia/configs/builds/raze.toml`; Orion `clock.set`, eased LED blink, honest failed units, libcamera without libyuv/jpeg/bzip2, the trimmed kernel). Eidos main 52d0c9e60, Lemnos dev cca50f7 and Orion main 973ced7 unchanged for now.
- Image: the vision/media layer selects `BR2_PACKAGE_JPEG` for helios-api's and helios-peripherals' turbojpeg codec, since Atlas's libcamera no longer pulls libjpeg in. The build image (`gaia/docker/aarch64/Dockerfile.aarch64-rpi4`) installs `ccache`.
- Pins (lockstep, one source each): Daedalus dev 66659f7 (smaller `#[node]` expansions, plugin ABI 9; `paste` dropped), Styx dev 40069d0, Eidos main 52d0c9e60 (dormant tracks from 2bbad728d), Lemnos dev cca50f7 (crates and the Gaia `lemnos` source), Orion main 973ced7 (crates and the Gaia `orion` source), Atlas dev 934f538 (`e453fe3d`, `8cacdcfa`).
- Image: the `lemnos` user now comes from Lemnos's own Buildroot users table through the device package; HeliOS's `lemnos-users.table` is gone (`e453fe3d`, `8cacdcfa`).
- Tag search: the stored templates and camera pipelines default to a full search every 4th frame, with Eidos's defaults for track loss and dormant tracks. CM5, one thread, the recorded test video: 0.40-0.41 ms mean per frame, p99 about 1.0 ms, 3012 of 3149 reference tags (full search every frame: 3034 at 0.84 ms); new tags within 3 frames (`fd6d2e65`).
- Raw GPIO, PWM, I2C and SPI are back, brokered by lemnosd: a `lemnos.raw` resource with `gpio.claim|configure|get|set|release`, `pwm.claim|configure|release`, `i2c.transfer`, `spi.transfer` and `raw.renew`, `GET /v1/peripherals/io`, `POST /v1/peripherals/io/actions` and SSE `gpio` events. Claims are leases with a time to live, end with helios-peripherals' lemnosd connection and are claimed again after a lemnosd restart; board-owned lines and addresses are refused. The fan-override marker file is gone: lemnosd undoes HeliOS's writes when its connection closes. Tests run against Lemnos's `MockLemnosd` (`50662ad8`).
- TODO: resolved Lemnos gaps removed; new upstream gaps (Orion action replies, Lemnos light owner) and an Atlas ask for a named spare line (`ffedbbf7`).
- Pins: Eidos main d3e4b4a5b, Orion main 4a6945a (crates and the Gaia `orion` source), Lemnos dev 62c3caf (crates and a new Gaia `lemnos` source), Styx dev 185ad43, Daedalus dev bcc9f33, Atlas dev e12b079 (`f9e98b69`, `118d37d6`, `278b12ae`, `cd5173d5`).
- Pose: the stored graphs are Eidos's templates with `aruco.multi_tag_pose` and held structured `camera`/`extrinsics` inputs; camera context is Orion F64 (`camera.*`, `mount.*`), pushed without recompiling (`76ac0161`).
- Field layouts: `helios-field` converts WPILib AprilTag JSON and Limelight `.fmap` into Eidos known tag poses (the FRC 2026 AndyMark field built in); `/v1/field-layouts` uploads, lists and selects them; the pipeline `pose` summary's camera and robot in the field come from the multi-tag pose (`d8bf4eb8`, `ac3e06b1`).
- Image: systemd-sysusers and a build-time `lemnos` user; lemnosd built on the host; no empty linux-firmware selection.
- Gates: `tools/gates.sh`; dev builds keep line tables only, binaries without tests and libraries without doc tests build no test harness or rustdoc pass (`930e283a`). Field layouts parse with exact floats, so the stored graphs do not depend on feature unification (`643b049a`).
- helios-peripherals reaches the hardware only through lemnosd (`lemnos-ipc`, client `helios`): one Orion resource per board device with its readings, the fan read-only with a timed `fan.override` that always ends in a release to the kernel governor, and HeliOS's status on lemnosd's status layer. The in-process Lemnos runtime, the hwmon fan driver, raw GPIO/PWM/I2C/SPI actions, `HELIOS_SENSOR_CONFIG_PATHS` and the unit's sysfs fan `ExecStopPost` are gone.

## v2026.1.0

Date: March 15, 2026

### Summary

- The downloadable image went from about 2.4 GB to about 100 MB: about 95.8% smaller, or about 24x smaller.
- Major API and UI refresh with faster loading, stronger invalidation/realtime updates, and broader device/system state coverage.
- Stream workflows were reworked across registration, calibration, recording, and media handling.
- Localization and peers flows were hardened for seeded maps, pose handling, validation, and multi-source operation.
- Camera defaults and platform integration improved, including OV9782 tuning, libcamera fixes, startup/networking/persistence fixes, and Windows USB link fixes.

### OS + Platform

- Reduced downloadable image size from about 2.4 GB to about 100 MB: about 95.8% smaller, or about 24x smaller, than `v2026.0.0`.
- Reduced installed on-device footprint from about 7 GB to about 180 MB: about 97.4% smaller, or about 38.9x smaller.
- Reduced average RAM use from about 1 GB to about 250 MB: about 75.0% lower, or about 4x lower.
- Reduced idle CPU usage from about 15% to about 2%: about 86.7% lower, or about 7.5x lower.
- Reduced idle resource burn and continued memory footprint cleanup across the image and services.
- Fixed startup, persistence, dependency-upgrade, and networking issues that affected bring-up and long-running stability.
- Expanded system/device surfaces for revision, storage, metrics, bootloader, logs, and health reporting.

### API + UI

- Continued the API overhaul with new read-model plumbing, better invalidation, faster request paths, and stronger realtime updates.
- Expanded WebSocket coverage for runtime data, including sensor and IMU streams.
- Refined frontend data access and pipeline/stream UI plumbing for outputs, metrics, and device state.
- Renamed the old snapshots support workflow to `Diagnostics`, with updated UI/docs terminology.

### Cameras + Media

- Seeded sharper OV9782 defaults for new streams and tuned denoise/sharpen behavior.
- Improved stream registration and camera-page behavior, including responsive calibration/controls layouts on smaller screens.
- Relaxed calibration solve gating and improved advisory guidance during calibration and stream-quality work.
- Fixed upload and camera-page UI bugs and hardened recording/media workflows.

### Localization + Peers

- Fixed localization pose issues and hardened seeded field-map bootstrap behavior.
- Improved localization source handling, validation, and pipeline integration for more reliable multi-source operation.
- Continued improvements to peer discovery/registration and related networking flows.

### Reliability Fixes

- Fixed libcamera IPA install failures that could block image startup.
- Fixed Windows USB link handling issues.
- Improved backend validation coverage around streams, calibration, and related runtime workflows.

## v2026.0.0

Date: February 27, 2026

### Summary

- Initial public HeliOS image release.
- Introduced the first published CM5 image and docs-site release-notes flow.

### Notes

- This was the initial baseline release for the `v2026.x` line.
- Detailed per-area release notes were not captured in the same level of detail as later releases.
