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

HeliOS runs on one device, the Raze (CM5 + OV9782). Its device support
(kernel, OV9782 driver, libcamera/libpisp, boot overlays, fan, USB port power,
LED ring, USB gadget, identity endpoint) comes from the Raze device package in
Atlas Hardware Manager (`devices/raze`), imported as the pinned git source
`atlas` in `gaia/configs/builds/raze.toml`. HeliOS adds its own OS on top:
squashfs A/B storage and initramfs, Orion and the HeliOS services, the
frontend, and its own `config.txt`/`cmdline.txt`.

You also need Docker (Gaia cross-compiles the Rust artifacts in an image it
builds from `gaia/docker/aarch64/Dockerfile.aarch64-rpi4` when missing) and Bun
(the full profile stages the frontend bundle from `frontend/build`).

## 2) Build An Image

```bash
# from the HeliOS repo root
./tools/build-os.sh                # Gaia TUI with every HeliOS build
./tools/build-os.sh raze           # full Raze image, non-interactive
./tools/build-os.sh raze base-os   # base OS only
```

The only target is `raze` (`gaia/configs/builds/raze.toml`). It exposes a
`profile` input: `base-os` or `full`.

Before running Gaia, the script rebuilds `frontend/build` when its inputs
changed (`FORCE_FRONTEND_BUILD=1` forces a rebuild).

Direct Gaia invocation, from the repo root:

```bash
gaia validate gaia/configs/builds/raze.toml --set input.profile=full
gaia plan gaia/configs/builds/raze.toml --set input.profile=full
gaia run gaia/configs/builds/raze.toml --set input.profile=full
```

### Local Atlas checkout

The build fetches Atlas at the pinned `rev`. To build against a local
checkout instead (for example while changing the device package, or before
the pinned commit is pushed), point the `atlas` source at it:

```bash
./tools/build-os.sh raze full --set sources.atlas.path=$PWD/../Atlas-Hardware-Manager
gaia plan gaia/configs/builds/raze.toml --set input.profile=full \
  --set sources.atlas.path=$PWD/../Atlas-Hardware-Manager
```

The path is absolute or relative to the repo root. Do not run `gaia lock`
with this override (Gaia then treats `atlas` as a path source).

## 3) Output Paths

For build `helios-<profile>-raze` (for example `helios-full-raze`):

- Build state: `gaia/build/helios-full-raze/`
- Rust artifacts: `gaia/output/helios-full-raze/artifacts/`
- Images: `gaia/output/helios-full-raze/images/` (`sdcard.img`, `boot.vfat`,
  `rootfs.squashfs`)
- Reports: `gaia/output/helios-full-raze/.gaia/reports/`

## 4) Flashing

Flash `gaia/output/helios-full-raze/images/sdcard.img` with Atlas Hardware
Manager.

## Notes

- Image customization lives in `gaia/configs` and `gaia/assets`; see
  `gaia/configs/README.md` for the layer layout.
- Git sources are pinned with `rev` or a tag, and `gaia lock` records the
  resolved commits in `<build>.gaia.lock` next to each build file. Commit the
  lock files; refresh them with `gaia lock <build> --update`. Sources with an
  explicit `rev` (Atlas, Orion) are pinned by that `rev` and get no lock
  entry; to move the Raze device package, change `rev` for `atlas` in
  `gaia/configs/builds/raze.toml`.
