# Configuration Layout

Active build entrypoints live in `configs/builds/`.

- `builds/` – top-level build definitions (currently `HeliOS-cm5.toml`).
- `workspace/` – workspace-level defaults (`root_dir`, `build_dir`, `out_dir`, path aliases, cleanup mode).
- `distros/` – distro composition (HeliOS module imports).
- `platforms/` – board/CPU platform overlays (Raspberry Pi CM5 config).
- `modules/` – reusable module configs for `buildroot`, `program`, and `stage`.
- `env/` – imported environment sets used by staged services.

`configs/` is TOML-centric. Non-TOML runtime/build assets live under `assets/`
(Buildroot tree, services, overlays, templates, SDK cache, etc.).

## Path Resolution

Path-like fields now resolve in this order:
- `@alias/...` from `[workspace.paths]` (plus built-ins: `@root`, `@build`, `@out`)
- absolute path
- relative path under `[workspace].root_dir` (default: current working directory)

Example:

```toml
[workspace]
root_dir = "."
build_dir = "build"
out_dir = "output"

[workspace.paths]
assets = "assets"
packages = "assets/buildroot/packages"
helios = ".."
```

Useful `buildroot` output controls live in `configs/modules/buildroot/base.toml`:
- `collect_out_dir` (where collected images are copied),
- `shrink_ext` (shrink copied ext rootfs images),
- `archive_format`/`archive_mode`/`archive_name` (create archives, including flashable `img.xz`),
- `report`/`report_hashes` (emit `image-report.json` with sizes and hashes).

Build output paths support templates:
- `{build}` -> build filename stem (for `configs/builds/HeliOS-cm5.toml`, this is `HeliOS-cm5`)
- `{version}` -> `[build].version` from the build file

Example:

```toml
[build]
version = "1.20250915"

[buildroot]
collect_out_dir = "output/{build}/{version}/images"
archive_mode = "image"
archive_format = "img.xz"
archive_name = "{build}-{version}-sdcard"
```

The current CM5 build file is:
- `configs/builds/HeliOS-cm5.toml`

It produces the squashfs/overlay image used for both release and live-deploy
development workflows.
