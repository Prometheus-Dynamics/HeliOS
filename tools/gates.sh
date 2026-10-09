#!/usr/bin/env bash
# HeliOS's gates, in the order that shares the most work, from the repo root:
#
#   tools/gates.sh            # everything
#   tools/gates.sh backend    # fmt, clippy, tests
#   tools/gates.sh ui         # UI check and build
#   tools/gates.sh gaia       # gaia validate, both profiles (needs ui/build for full)
#
# - `cargo clippy --workspace --all-targets` type-checks every target, so there is no separate
#   `cargo check` gate; clippy and `cargo check` share their dependency metadata anyway.
# - `cargo test --workspace` builds no test harness for binaries without tests (`test = false`)
#   and runs no rustdoc pass for libraries without doc tests (`doctest = false`).
# - Everything uses one target directory (CARGO_TARGET_DIR, else backend/.cargo's `../target`).
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
what=${1:-all}

backend() {
  (cd "$root/backend" && cargo fmt --all -- --check)
  (cd "$root/backend" && cargo clippy --workspace --all-targets -- -D warnings)
  (cd "$root/backend" && cargo test --workspace)
}

ui() {
  (cd "$root/ui" && bun install --frozen-lockfile && bun run check && bun run build)
}

gaia_validate() {
  (cd "$root" && gaia validate gaia/configs/builds/raze.toml --set input.profile=full)
  (cd "$root" && gaia validate gaia/configs/builds/raze.toml --set input.profile=base-os)
}

case $what in
  backend) backend ;;
  ui) ui ;;
  gaia) gaia_validate ;;
  all) backend; ui; gaia_validate ;;
  *) echo "usage: $0 [all|backend|ui|gaia]" >&2; exit 2 ;;
esac
