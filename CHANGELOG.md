# Changelog

This file is the aggregate release log for HeliOS images.

- One section per `VERSION_ID`
- New releases should be added at the top
- Detailed docs-site release notes live under `docs/src/pages/release-notes/`

## [Unreleased]

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
