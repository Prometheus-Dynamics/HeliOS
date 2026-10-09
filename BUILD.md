# HeliOS Image Build (Gaia)

HeliOS images are built with the Gaia builder CLI (`gaia`). The build
configuration lives in this repo under `gaia/`.

## 1) Prerequisites

Install the Gaia CLI:

```bash
cargo install --git https://github.com/Prometheus-Dynamics/Gaia-Image-Builder --branch main gaia
```

It installs into `~/.cargo/bin`; make sure that is on your `PATH`. The build
requires Gaia 2.0.0 or newer (`gaia_version` in `builds/raze.toml`).

HeliOS runs on one device, the Raze (CM5 + OV9782). Its device support
(kernel, OV9782 driver, libcamera/libpisp, boot overlays, fan, USB port power,
LED ring, USB gadget, identity endpoint, SSH keys from the boot partition,
hardware watchdog, default hostname, the A/B update writer) comes from the Raze
device package in Atlas Hardware Manager (`devices/raze`, 1.5.0), imported as
the pinned git source `atlas` in `gaia/configs/builds/raze.toml`. `orion-node`
comes from Orion's own Gaia layer (`packaging/gaia/orion-node.toml`),
imported from the pinned git source `orion`. HeliOS adds its own OS on top:
the read-only EROFS root with its `/data` partition, the HeliOS services and
the vision plugin (built from `backend/`), the UI (built from `ui/`), the A/B
update hooks, and its own `config.txt`/`cmdline.txt`/`autoboot.txt`.

You also need Docker (Gaia cross-compiles the Rust artifacts in an image it
builds from `gaia/docker/aarch64/Dockerfile.aarch64-rpi4` when missing) and Bun
(the full profile stages the UI's static build from `ui/build`).

## 2) Build An Image

```bash
# from the HeliOS repo root
./tools/build-os.sh                # Gaia TUI with every HeliOS build
./tools/build-os.sh raze           # full Raze image, non-interactive
./tools/build-os.sh raze base-os   # base OS only
```

The only target is `raze` (`gaia/configs/builds/raze.toml`). It exposes a
`profile` input: `base-os` or `full`.

Before running Gaia, the script builds the UI (`cd ui && bun install
--frozen-lockfile && bun run build`) when `ui/build` is missing or older than
its inputs (`FORCE_UI_BUILD=1` forces a rebuild).

Direct Gaia invocation, from the repo root (build the UI first for `full`):

```bash
(cd ui && bun install --frozen-lockfile && bun run build)
gaia validate gaia/configs/builds/raze.toml --set input.profile=full
gaia plan gaia/configs/builds/raze.toml --set input.profile=full
gaia run gaia/configs/builds/raze.toml --set input.profile=full
```

### Local Atlas or Orion checkout

The build fetches Atlas and Orion at their pinned `rev`. To build against a
local checkout instead (for example while changing the device package, or
before the pinned commit is pushed), point the source at it:

```bash
./tools/build-os.sh raze full --set sources.atlas.path=$PWD/../Atlas-Hardware-Manager
gaia plan gaia/configs/builds/raze.toml --set input.profile=full \
  --set sources.atlas.path=$PWD/../Atlas-Hardware-Manager \
  --set sources.orion.path=$PWD/../Orion
```

The path is absolute or relative to the repo root. Do not run `gaia lock`
with this override (Gaia then treats the source as a path source).

## 3) Output Paths

For build `helios-<profile>-raze` (for example `helios-full-raze`):

- Build state: `gaia/build/helios-full-raze/`
- Rust artifacts: `gaia/output/helios-full-raze/artifacts/`
- Images: `gaia/output/helios-full-raze/images/` (`sdcard.img`, `boot.vfat`,
  `rootfs.erofs`, `data.ext4`)
- Compressed image: `output/helios-full-raze-<version>.img.xz`
- Reports: `gaia/output/helios-full-raze/.gaia/reports/`

The disk is the Raze A/B layout shared with the PhotonVision image: p1
autoboot (16M), p2/p3 boot A/B (128M), p5/p6 root A/B (512M, read-only
EROFS), p7 `/data` (grown to fill the eMMC on first boot). See
`gaia/configs/README.md` for the layout, the update flow and where everything
HeliOS writes at runtime goes.

## 4) Flashing and updating

Flash `sdcard.img` (or the `.img.xz`) with Atlas Hardware Manager over USB
boot. A board still on the old two-partition squashfs layout must be
reflashed this way once.

A running board updates from the same `.img.xz`:

- from the HeliOS UI or the API: upload it (`POST /v1/update/uploads`), then
  `POST /v1/update/apply` (docs/docs/api/http.md, "Updates");
- from Atlas, over SSH or HTTP (`/v1/ota/*`);
- on the board: copy it to `/data`, then
  `/usr/lib/board/update stage /data/<image>.img.xz --sha256 <hex>` and
  `/usr/lib/board/update apply` (the device package's A/B writer).

The board reboots into the new slot on trial and keeps it once
`/etc/board/update-health` passes; otherwise it falls back to the previous
slot by itself.

On the device, `/etc/default/helios-image.env` records the image version and
the exact HeliOS, device package and Orion commits it was built from.

## 5) Gates

`tools/gates.sh` runs what a change must pass, sharing one target directory:
`cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace`, the UI's `bun run check` and `bun run build`, and
`gaia validate` for both profiles (`tools/gates.sh backend|ui|gaia` runs one
group). There is no separate `cargo check` gate: clippy over all targets
type-checks everything `cargo check --all-targets` does.

What keeps the backend gates cheap (`backend/Cargo.toml` and the crates'
manifests):

- Debug builds and tests keep line tables only (`[profile.dev] debug =
  "line-tables-only"`): panics and backtraces keep file and line; full debug
  info, most of what the compiler writes and the linker reads per test binary,
  is gone. `--config profile.dev.debug=true` brings it back for a debugger.
- Binaries without tests have `test = false` and libraries without doc tests
  `doctest = false`, so `cargo test` links no empty test harness of the
  engine, API, peripherals, diagnostics and probe binaries and runs no
  rustdoc pass. clippy still checks those binaries.

Host-only timings (x86-64, 24 threads, shared and loaded host, load average
35 to 50, a disk that stalls; one run each from a cold target directory with
the dependencies already downloaded; CPU is user + system):

| Step | Before wall / CPU | After wall / CPU |
|---|---|---|
| `cargo check --workspace --all-targets` (cold) | 56 s / 230 s | dropped (clippy covers it) |
| `cargo clippy --workspace --all-targets` (cold) | 28 to 37 s / 196 to 204 s | 31 s / 195 s |
| `cargo test --workspace` (cold) | 74 to 79 s / 523 to 530 s | 67 to 72 s / 383 to 433 s |
| `cargo test --workspace` after touching the engine | 18 to 22 s / 72 to 73 s | 12 to 14 s / 59 to 60 s |
| `cargo build --workspace` (cold, dev) | 61 to 71 s / 454 to 489 s | 50 to 51 s / 372 to 380 s |
| `cargo build --workspace` after touching the engine | 12 to 19 s / 7 to 8 s | 6 to 7 s / 5 to 7 s |
| dev target directory after a cold build and tests | 5.1 GB | 2.8 GB |
| `bun run check` / `bun run build` | 149 s / 16 s, 939 s / 19 s (wall is the disk) | unchanged |
| `gaia validate`, full / base-os | 606 s first run (fetching the import sources) / 0.5 s; under 1.5 s CPU | unchanged |

After the round-5 pins (Daedalus 66659f7, Styx 40069d0, Eidos 52d0c9e60, Lemnos cca50f7 with
`lemnosd`'s mock as a test dependency of helios-peripherals), same host and method (load average
about 45): cold clippy 33 s / 204 s CPU, cold `cargo test --workspace` 73 s / 433 s CPU, 2.3 GB
target directory. No noticeable change.

cargo-nextest was tried and not adopted: the tests themselves run in a few
seconds, so `cargo nextest run --workspace` (56 s / 403 s cold) saved nothing
over `cargo test`. Optimizing dependencies in the test profile was not
adopted either: it adds compile time to save test time the suite does not
spend.

## Notes

- Image customization lives in `gaia/configs` and `gaia/assets`; see
  `gaia/configs/README.md` for the layer layout.
- Git sources are pinned with `rev` or a tag, and `gaia lock` records the
  resolved commits in `<build>.gaia.lock` next to each build file. Commit the
  lock files; refresh them with `gaia lock <build> --update`. Sources with an
  explicit `rev` (Atlas, Orion) are pinned by that `rev` and get no lock
  entry; to move the Raze device package or Orion, change `rev` for `atlas`
  or `orion` in `gaia/configs/builds/raze.toml`. Keep `orion` at the commit
  `backend/Cargo.lock` pins for the `orion` crate.
- The backend's git dependencies (Daedalus, Styx, Eidos, Orion, Lemnos) are
  pinned by `backend/Cargo.lock`; cargo fetches them inside the cross-build
  container, so the build needs network access to GitHub.
- Developer binary deploys (`tools/deploy-live.sh`) cannot replace `/usr/bin`
  on the read-only root: they upload to `/data/helios-dev/<revision>` and point
  the services there with runtime drop-ins in `/run/systemd/system`, which a
  reboot discards.
