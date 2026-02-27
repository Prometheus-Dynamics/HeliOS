# HeliOS Image Build (Gaia)

Use Gaia as the image builder for HeliOS. The Gaia buildchain is vendored in this repo under `gaia/`.

## 1) Install Gaia CLI

```bash
# from the HeliOS repo root
cargo install --locked --git https://github.com/Prometheus-Dynamics/Gaia-Image-Builder --package gaia-image-builder --bin gaia --force
```

This installs `gaia` (typically into `~/.cargo/bin`).

If needed, add Cargo bin to your shell `PATH`:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

Verify install:

```bash
gaia --help
```

## 2) Build The HeliOS CM5 Image

Recommended:

```bash
# from the HeliOS repo root
./tools/build-os.sh cm5
```

This opens Gaia TUI with HeliOS builds loaded.

Direct Gaia invocation:

```bash
# from the HeliOS repo root
cd gaia
gaia tui --builds-dir configs/builds
```

## 3) Useful Commands

```bash
cd gaia

# inspect merged config
gaia resolve configs/builds/HeliOS-cm5.toml

# print task plan
gaia plan configs/builds/HeliOS-cm5.toml
```

## 4) Output Paths

- Build state: `gaia/build`
- Artifacts/images: `gaia/output`

## 5) Flashing

Recommended: flash the generated image using Atlas Hardware Manager for HeliOS devices.

Image path to select in Atlas Hardware Manager:

- `gaia/output/HeliOS-cm5/<version>/images/*.img` (or the archived `.img.xz`)

## Notes

- Current production target is CM5 via `gaia/configs/builds/HeliOS-cm5.toml`.
- HeliOS image customization is done in `gaia/configs` and `gaia/assets`.
- Gaia frontend artifact builds are input-aware: `program.custom` now runs `frontend/scripts/build-if-changed.mjs`, which fingerprints frontend + docs + API-codegen/schema inputs and skips rebuilds when unchanged.
- To force a frontend rebuild on the next Gaia run: `FORCE_FRONTEND_BUILD=1 ./tools/build-os.sh cm5`.
