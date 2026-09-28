# Configuration Layout

HeliOS images are Gaia v2 builds. Run Gaia from the repository root; see
`BUILD.md` for commands.

## Entry points

`builds/` holds one build per hardware target: `cm5.toml`, `cm4.toml` and
`generic-aarch64-linux.toml`. Each build is named
`helios-${input.profile}-<target>` and exposes a `profile` input:

- `base-os` imports `layers/base-os.toml`, the target's firmware/hardware layer
  and `targets/<target>.toml`.
- `full` additionally imports `layers/app-layer.toml` and
  `layers/release-layer.toml` (`when = { profile = "full" }`).

## Directories

- `layers/` – the composition units the builds import:
  - `base-os.toml`: workspace, Buildroot OS baseline, squashfs rootfs, minimal network.
  - `firmware-hardware-pi.toml` / `firmware-hardware-generic-aarch64-linux.toml`:
    platform-family layers.
  - `app-layer.toml`: Rust payloads, frontend bundle, runtime services and their
    package dependencies.
  - `release-layer.toml`: identity, hardware data, writable-state storage,
    runtime config and ops.
- `targets/` – board selection only (defconfig, kernel, image assembly).
- `platform-families/` – shared defaults for `raspberry-pi` and `aarch64-linux`.
- `workspace/` – `root_dir`, `build_dir`/`out_dir` (`gaia/build/${build.name}`,
  `gaia/output/${build.name}`), the `@assets` alias, reporting and failure policy.
- `os/`, `network/`, `hardware/`, `identity/`, `ops/`, `storage/`,
  `runtime-config/` – layer fragments imported by `layers/`.
- `payloads/` – what gets built and installed: Rust artifacts (built in the
  `helios-cross-rust194` Docker image), `orion-node`/`orionctl` from a pinned Orion
  revision, and the prebuilt frontend bundle.
- `runtime-services/` – systemd units, sockets and service assets for those payloads.

Non-TOML inputs (the Buildroot external tree, systemd units, overlays, tuning
files) live under `gaia/assets/`. The cross-build image lives under
`gaia/docker/`.

## Conventions

- Keep target files limited to board selection; shared behaviour belongs in a
  layer or platform family.
- Pin git sources with `rev`. `pin = "locked"` alone only stops re-fetching;
  the revision is not recorded anywhere.
- Paths use `@assets/...` or are relative to the repository root.
