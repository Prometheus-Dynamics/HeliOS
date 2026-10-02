# HeliOS Image Build (Gaia)

HeliOS images are built with the Gaia builder CLI (`gaia`). The build
configuration lives in this repo under `gaia/`.

## 1) Prerequisites

Install the Gaia CLI:

```bash
cargo install --git https://github.com/Prometheus-Dynamics/Gaia-Image-Builder --branch dev gaia
```

It installs into `~/.cargo/bin`; make sure that is on your `PATH`. The builds
require Gaia 2.1.0 or newer (`gaia_version` in each build file).

You also need Docker (Gaia cross-compiles the Rust artifacts in an image it
builds from `gaia/docker/aarch64/Dockerfile.aarch64-rpi4` when missing) and Bun
(the full profile stages the frontend bundle from `frontend/build`).

## 2) Build An Image

```bash
# from the HeliOS repo root
./tools/build-os.sh                # Gaia TUI with every HeliOS build
./tools/build-os.sh cm5            # full cm5 image, non-interactive
./tools/build-os.sh cm5 base-os    # base OS only
```

Targets are the files in `gaia/configs/builds/` (`cm5`, `cm4`,
`generic-aarch64-linux`). Each exposes a `profile` input: `base-os` or `full`.

Before running Gaia, the script rebuilds `frontend/build` when its inputs
changed (`FORCE_FRONTEND_BUILD=1` forces a rebuild).

Direct Gaia invocation, from the repo root:

```bash
gaia validate gaia/configs/builds/cm5.toml --set input.profile=full
gaia plan gaia/configs/builds/cm5.toml --set input.profile=full
gaia run gaia/configs/builds/cm5.toml --set input.profile=full
```

## 3) Output Paths

For build `helios-<profile>-<target>` (for example `helios-full-cm5`):

- Build state: `gaia/build/helios-full-cm5/`
- Rust artifacts: `gaia/output/helios-full-cm5/artifacts/`
- Images: `gaia/output/helios-full-cm5/images/` (`sdcard.img`, `boot.vfat`,
  `rootfs.squashfs`)
- Reports: `gaia/output/helios-full-cm5/.gaia/reports/`

## 4) Flashing

Flash `gaia/output/helios-full-cm5/images/sdcard.img` with Atlas Hardware
Manager.

## Notes

- Image customization lives in `gaia/configs` and `gaia/assets`; see
  `gaia/configs/README.md` for the layer layout.
- Git sources are pinned with `rev` or a tag, and `gaia lock` records the
  resolved commits in `<build>.gaia.lock` next to each build file. Commit the
  lock files; refresh them with `gaia lock <build> --update`.
