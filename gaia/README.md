# Gaia 0.2 Base OS

This is the rebuilt Gaia 0.2 tree for the bare appliance OS layer.

Current scope:
- Buildroot-based bare OS
- squashfs rootfs output
- core OS package layer only
- shared graph for:
  - `cm5`
  - `cm4`
  - `generic-aarch64-linux`

Explicitly not part of this layer:
- staged assets
- env files
- service files
- application artifacts
- runtime payloads
- project-specific provisioning or recovery logic

The config tree is now shaped like the dependency graph:
- `configs/builds/`
  - concrete build entrypoints; use these directly or by short name
- `configs/workspace/`
  - execution, failure, reporting defaults
- `configs/os/`
  - shared base OS and storage-mode layers
- `configs/hardware/`
  - reusable hardware/media capability layers
- `configs/identity/`
  - hostname, users, locale, versioning, and access policy layers
- `configs/network/`
  - reusable network fabric modes
- `configs/payloads/`
  - app payload and install-set layers
- `configs/runtime-config/`
  - env generation and runtime tuning layers
- `configs/runtime-services/`
  - service graph layers
- `configs/ops/`
  - diagnostics, recovery, update, and maintenance layers
- `configs/platform-families/`
  - shared architecture/platform-family defaults
- `configs/targets/`
  - thin target-specific files only

The other `configs/` directories are reusable layers imported by those builds.

Current build entrypoints:
- `configs/builds/base-os-cm5.toml`
- `configs/builds/base-os-cm4.toml`
- `configs/builds/base-os-generic-aarch64-linux.toml`
- `configs/builds/full-cm5.toml`
- `configs/builds/full-cm4.toml`
- `configs/builds/full-generic-aarch64-linux.toml`

From this `gaia/` directory, short names resolve through `configs/builds/`:

```bash
gaia tui base-os-cm5
gaia validate full-cm5
gaia run full-cm5
```

Running `gaia tui` without a build opens the TUI on the first concrete build
entrypoint and lets you switch between entries in `configs/builds/`.

Output layout:
- `gaia/output/<build>/` is Gaia internal state: per-build collected images, Buildroot output, reports, runtime manifests, and reuse markers.
- `gaia/build/<build>/` is Gaia internal build state: materialized sources and build working directories.
- `output/` at the repository root is the release artifact directory for flashable deliverables.
- Final HeliOS image archives use `${project.root_dir}/output/${build.name}-${build.version}.tar.xz`.
- The release archive contains `sdcard.img`; flash the extracted image, not files from `gaia/output/<build>/`.
