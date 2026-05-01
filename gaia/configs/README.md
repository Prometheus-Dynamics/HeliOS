# HeliOS Gaia Configs

Use `configs/builds/` for concrete build entrypoints. Everything else under
`configs/` is a reusable layer imported by those builds.

## Build Entrypoints

- `builds/base-os-cm5.toml`: minimal CM5 OS image
- `builds/base-os-cm4.toml`: minimal CM4 OS image
- `builds/base-os-generic-aarch64-linux.toml`: minimal generic aarch64 Linux image
- `builds/full-cm5.toml`: full CM5 appliance image
- `builds/full-cm4.toml`: full CM4 appliance image
- `builds/full-generic-aarch64-linux.toml`: full generic aarch64 Linux image

From this `gaia/` directory, Gaia can resolve a build by short name:

```bash
gaia tui base-os-cm5
gaia validate full-cm5
gaia run full-cm5
```

Running `gaia tui` without a build opens the TUI on the first build entrypoint
and lets you switch between entries in `configs/builds/`.

## Layer Directories

- `workspace/`: workspace, output, provider, execution, and failure defaults
- `os/`: base OS and image output mode layers
- `platform-families/`: shared platform-family layers
- `targets/`: thin target selections
- `layers/`: reusable compositions
- `hardware/`, `network/`, `identity/`, `storage/`: domain layers
- `payloads/`, `runtime-config/`, `runtime-services/`: application/runtime layers
- `ops/`: diagnostics and maintenance layers
