#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

SSH_TARGET_DEFAULT="root@172.31.250.1"

SSH_TARGET="$SSH_TARGET_DEFAULT"
SSH_PASS="root"

TARGET_TRIPLE="${TARGET_TRIPLE:-aarch64-unknown-linux-gnu}"
PROFILE_FLAG="--profile dev-release"
RUSTFLAGS="${RUSTFLAGS:-}"
ENGINE_FEATURES="${ENGINE_FEATURES:-}"
API_FEATURES="${API_FEATURES:-}"

DOCKERFILE="${DOCKERFILE:-gaia/docker/aarch64/Dockerfile.aarch64-rpi4}"
DOCKER_CONTEXT="${DOCKER_CONTEXT:-$ROOT_DIR/gaia}"
IMAGE_TAG="${IMAGE_TAG:-helios-cross-rust194}"
REBUILD_IMAGE="0"

CROSS_BUILD_ROOT_DEFAULT="/var/tmp/helios-cross/${IMAGE_TAG}"
TARGET_BUILD_DIR="${TARGET_BUILD_DIR:-$CROSS_BUILD_ROOT_DEFAULT/target}"
CROSS_CARGO_HOME="${CROSS_CARGO_HOME:-$CROSS_BUILD_ROOT_DEFAULT/cargo}"
CROSS_SCCACHE_DIR="${CROSS_SCCACHE_DIR:-$CROSS_BUILD_ROOT_DEFAULT/sccache}"

# Optional local checkouts used when you want to patch these dependencies during development.
# In a clean/public clone, leave unset.
default_checkout_path() {
  local path="$1"
  if [[ -d "$path" ]]; then
    printf '%s' "$path"
  fi
}

DAEDALUS_HOST_PATH="${DAEDALUS_HOST_PATH:-$(default_checkout_path /home/sozo/Documents/GitHub/Daedalus)}"
STYX_HOST_PATH="${STYX_HOST_PATH:-$(default_checkout_path /home/sozo/Documents/GitHub/Styx)}"
LIBCAMERA_RS_HOST_PATH="${LIBCAMERA_RS_HOST_PATH:-$(default_checkout_path /home/sozo/Documents/GitHub/libcamera-rs)}"

BINS_DIR_DEFAULT="$ROOT_DIR/output/cm5/binaries"
PLUGINS_DIR_DEFAULT="$ROOT_DIR/output/cm5/plugins/daedalus"
BINS_DIR="$BINS_DIR_DEFAULT"
PLUGINS_DIR="$PLUGINS_DIR_DEFAULT"

BIN_DIR_REMOTE="${BIN_DIR_REMOTE:-/usr/bin}"
# Daedalus plugins are large (especially debug builds). Default to /var/lib/helios (separate partition on CM5)
# to avoid filling the root filesystem.
PLUGIN_DIR_REMOTE="${PLUGIN_DIR_REMOTE:-/var/lib/helios/plugins/daedalus}"
TEMPLATES_DIR_LOCAL="${TEMPLATES_DIR_LOCAL:-$ROOT_DIR/gaia/assets/templates}"
TEMPLATES_DIR_REMOTE="${TEMPLATES_DIR_REMOTE:-/usr/share/helios/pipeline-templates}"
FRONTEND_DIR_LOCAL="${FRONTEND_DIR_LOCAL:-$ROOT_DIR/frontend/build}"
FRONTEND_DIR_REMOTE="${FRONTEND_DIR_REMOTE:-/opt/helios/frontend}"

ONLY="all" # all|binaries|plugins
STRICT_BINARIES_ONLY="0"
BUILD="1"
UPLOAD="1"
RESTART_SERVICES="1"
DRY_RUN="0"
FAST_UPLOAD="${FAST_UPLOAD:-1}"
UPLOAD_TEMPLATES="1"
UPLOAD_FRONTEND="1"
STRIP_DEBUG="1"

usage() {
  cat <<EOF
Build + deploy Helios CM5 binaries and Daedalus plugins to a device, then restart services.

Usage:
  ./scripts/deploy-cm5.sh [options]

Options:
  --ssh <user@host>     SSH target (default: $SSH_TARGET_DEFAULT)
  --pass <password>     Optional SSH password (uses sshpass)
  --release|--debug|--dev-release
                        Build profile (default: dev-release)
  --engine-features <f> Cargo features for helios-engine (default: $ENGINE_FEATURES)
  --api-features <f>   Cargo features for helios-api (default: $API_FEATURES)
  --only <what>         all|binaries|plugins (default: all)
  --strict-binaries-only
                        Do not auto-sync Daedalus plugins when deploying binaries
  --no-build            Skip build; only upload/restart
  --no-upload           Only build; skip upload/restart
  --no-restart          Upload but do not restart services
  --bins-dir <dir>      Local binaries dir (default: $BINS_DIR_DEFAULT)
  --plugins-dir <dir>   Local plugins dir (default: $PLUGINS_DIR_DEFAULT)
  --bin-dir <dir>       Remote bin dir (default: $BIN_DIR_REMOTE)
  --plugin-dir <dir>    Remote plugin dir (default: $PLUGIN_DIR_REMOTE)
  --no-templates        Skip uploading pipeline templates
  --templates-dir <dir> Local templates dir (default: $TEMPLATES_DIR_LOCAL)
  --templates-remote <dir> Remote templates dir (default: $TEMPLATES_DIR_REMOTE)
  --no-frontend         Skip uploading frontend assets
  --frontend-dir <dir>  Local frontend build dir (default: $FRONTEND_DIR_LOCAL)
  --frontend-remote <dir> Remote frontend dir (default: $FRONTEND_DIR_REMOTE)
  --no-strip            Do not strip debug sections from built artifacts before upload
  --fast-upload         Upload everything without hashing (default)
  --slow-upload         Hash local/remote to avoid uploading unchanged artifacts
  --target <triple>     Rust target triple (default: $TARGET_TRIPLE)
  --dockerfile <path>   Cross-builder dockerfile (default: $DOCKERFILE)
  --docker-context <p>  Docker build context for cross image (default: $DOCKER_CONTEXT)
  --image-tag <tag>     Docker image tag (default: $IMAGE_TAG)
  --rebuild-image       Force rebuild of docker image (once)
  --dry-run             Print actions without executing
  -h, --help            Show this help

Env vars (optional):
  RUSTFLAGS             Passed through to build scripts
  BIN_DIR_REMOTE         Remote bin dir
  PLUGIN_DIR_REMOTE      Remote plugin dir
  DAEDALUS_HOST_PATH     Host path to a Daedalus checkout (optional dev override)
  STYX_HOST_PATH         Host path to a Styx checkout (optional dev override)
  LIBCAMERA_RS_HOST_PATH Host path to a libcamera-rs checkout (optional dev override)
  DOCKER_CONTEXT         Docker build context for the cross image
  TARGET_BUILD_DIR       Host path for cross-built Cargo target artifacts
  CROSS_CARGO_HOME       Host path for cross-build Cargo cache
  CROSS_SCCACHE_DIR      Host path for cross-build sccache data
EOF
}

die() { echo "error: $*" >&2; exit 2; }

run() {
  if [[ "$DRY_RUN" == "1" ]]; then
    printf '+'
    printf ' %q' "$@"
    printf '\n'
    return 0
  fi
  "$@"
}

run_with_env() {
  local -a env_args=()
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --) shift; break ;;
      *=*) env_args+=("$1"); shift ;;
      *) break ;;
    esac
  done

  if [[ "$DRY_RUN" == "1" ]]; then
    printf '+ env'
    local e
    for e in "${env_args[@]}"; do
      printf ' %q' "$e"
    done
    printf ' --'
    printf ' %q' "$@"
    printf '\n'
    return 0
  fi

  env "${env_args[@]}" "$@"
}

profile_label() {
  if [[ "$PROFILE_FLAG" == "--release" ]]; then
    echo "release"
  elif [[ "$PROFILE_FLAG" == "--profile dev-release" ]]; then
    echo "dev-release"
  else
    echo "debug"
  fi
}

profile_dir_from_flag() {
  local profile_flag="$1"
  if [[ "$profile_flag" == "--release" ]]; then
    echo "release"
  elif [[ "$profile_flag" == "--profile dev-release" ]]; then
    echo "dev-release"
  else
    echo "debug"
  fi
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --ssh) SSH_TARGET="${2:-}"; shift 2 ;;
    --pass) SSH_PASS="${2:-}"; shift 2 ;;
    --release) PROFILE_FLAG="--release"; shift ;;
    --debug) PROFILE_FLAG=""; shift ;;
    --dev-release) PROFILE_FLAG="--profile dev-release"; shift ;;
    --engine-features) ENGINE_FEATURES="${2:-}"; shift 2 ;;
    --api-features) API_FEATURES="${2:-}"; shift 2 ;;
    --only) ONLY="${2:-}"; shift 2 ;;
    --strict-binaries-only) STRICT_BINARIES_ONLY="1"; shift ;;
    --no-build) BUILD="0"; shift ;;
    --no-upload) UPLOAD="0"; shift ;;
    --no-restart) RESTART_SERVICES="0"; shift ;;
    --bins-dir) BINS_DIR="${2:-}"; shift 2 ;;
    --plugins-dir) PLUGINS_DIR="${2:-}"; shift 2 ;;
    --bin-dir) BIN_DIR_REMOTE="${2:-}"; shift 2 ;;
    --plugin-dir) PLUGIN_DIR_REMOTE="${2:-}"; shift 2 ;;
    --no-templates) UPLOAD_TEMPLATES="0"; shift ;;
    --templates-dir) TEMPLATES_DIR_LOCAL="${2:-}"; shift 2 ;;
    --templates-remote) TEMPLATES_DIR_REMOTE="${2:-}"; shift 2 ;;
    --no-frontend) UPLOAD_FRONTEND="0"; shift ;;
    --frontend-dir) FRONTEND_DIR_LOCAL="${2:-}"; shift 2 ;;
    --frontend-remote) FRONTEND_DIR_REMOTE="${2:-}"; shift 2 ;;
    --no-strip) STRIP_DEBUG="0"; shift ;;
    --fast-upload) FAST_UPLOAD="1"; shift ;;
    --slow-upload) FAST_UPLOAD="0"; shift ;;
    --target) TARGET_TRIPLE="${2:-}"; shift 2 ;;
    --dockerfile) DOCKERFILE="${2:-}"; shift 2 ;;
    --docker-context) DOCKER_CONTEXT="${2:-}"; shift 2 ;;
    --image-tag) IMAGE_TAG="${2:-}"; shift 2 ;;
    --rebuild-image) REBUILD_IMAGE="1"; shift ;;
    --dry-run) DRY_RUN="1"; shift ;;
    -h|--help) usage; exit 0 ;;
    *) die "unknown arg: $1 (try --help)" ;;
  esac
done

if [[ -z "${SSH_TARGET// }" ]]; then
  die "--ssh is required"
fi

# Enable profiler + perf counters by default in release builds so Helios profiling endpoints
# can actually return flamegraphs/counters on-device. Users can override via --engine-features.
if [[ ( "$PROFILE_FLAG" == "--release" || "$PROFILE_FLAG" == "--profile dev-release" ) && -z "${ENGINE_FEATURES// }" ]]; then
  ENGINE_FEATURES="pprof,perf-counters"
fi

case "$ONLY" in
  all|binaries|plugins) ;;
  *) die "--only must be one of: all, binaries, plugins" ;;
esac

do_binaries="0"
do_plugins="0"
case "$ONLY" in
  all) do_binaries="1"; do_plugins="1" ;;
  binaries) do_binaries="1" ;;
  plugins) do_plugins="1" ;;
esac

if [[ "$do_binaries" == "1" && "$ONLY" == "binaries" && "$STRICT_BINARIES_ONLY" != "1" ]]; then
  do_plugins="1"
fi

docker_image_built="0"

ensure_deps() {
  if [[ "$BUILD" == "1" ]]; then
    command -v docker >/dev/null 2>&1 || die "docker is required for build"
    if [[ -n "${DAEDALUS_HOST_PATH// }" ]]; then
      [[ -d "$DAEDALUS_HOST_PATH" ]] || die "missing DAEDALUS_HOST_PATH dir: $DAEDALUS_HOST_PATH"
    fi
    if [[ -n "${STYX_HOST_PATH// }" ]]; then
      [[ -d "$STYX_HOST_PATH" ]] || die "missing STYX_HOST_PATH dir: $STYX_HOST_PATH"
    fi
    if [[ -n "${LIBCAMERA_RS_HOST_PATH// }" ]]; then
      [[ -d "$LIBCAMERA_RS_HOST_PATH" ]] || die "missing LIBCAMERA_RS_HOST_PATH dir: $LIBCAMERA_RS_HOST_PATH"
    fi
  fi
  if [[ "$UPLOAD" == "1" ]]; then
    command -v ssh >/dev/null 2>&1 || die "ssh is required for upload"
    if [[ -n "${SSH_PASS// }" ]]; then
      command -v sshpass >/dev/null 2>&1 || die "sshpass is required when using --pass"
    fi
  fi
}

ensure_deps

rebuild_available="$REBUILD_IMAGE"

docker_image_exists() {
  docker image inspect "$IMAGE_TAG" >/dev/null 2>&1
}

ensure_docker_image() {
  if [[ "$BUILD" != "1" ]]; then
    return 0
  fi
  if [[ "$rebuild_available" == "1" ]]; then
    run docker rmi -f "$IMAGE_TAG" >/dev/null 2>&1 || true
    rebuild_available="0"
  fi
  if ! docker_image_exists; then
    echo "Building docker image '$IMAGE_TAG' from '$DOCKERFILE' (context: $DOCKER_CONTEXT)..."
    run env DOCKER_BUILDKIT=1 docker build -f "$DOCKERFILE" -t "$IMAGE_TAG" "$DOCKER_CONTEXT"
  fi
  docker_image_built="1"
}

docker_run_cargo_build() {
  local package="$1"
  local target_triple="$2"
  local profile_flag="$3"
  local repo_root="$4"
  local features="${5:-}"
  local build_kind="${6:-package}"
  local bin_name="${7:-$package}"

  local -a docker_args=(
    --rm
    -v "$repo_root/backend:/work/backend"
    -v "$TARGET_BUILD_DIR:/work/target"
    -v "$CROSS_CARGO_HOME:/work/.cargo"
    -v "$CROSS_SCCACHE_DIR:/root/.cache/sccache"
    -e "CARGO_HOME=/work/.cargo"
    -e "CARGO_TARGET_DIR=/work/target"
    -e "SCCACHE_DIR=/root/.cache/sccache"
    -w /work/backend
  )

  if [[ -n "${DAEDALUS_HOST_PATH// }" ]]; then
    docker_args+=(-v "$DAEDALUS_HOST_PATH:$DAEDALUS_HOST_PATH")
  fi
  if [[ -n "${STYX_HOST_PATH// }" ]]; then
    docker_args+=(-v "$STYX_HOST_PATH:$STYX_HOST_PATH")
  fi
  if [[ -n "${LIBCAMERA_RS_HOST_PATH// }" ]]; then
    docker_args+=(-v "$LIBCAMERA_RS_HOST_PATH:$LIBCAMERA_RS_HOST_PATH")
  fi

  if [[ -n "${SSH_AUTH_SOCK:-}" && -S "${SSH_AUTH_SOCK}" ]]; then
    docker_args+=(
      -v "$SSH_AUTH_SOCK:/ssh-agent"
      -e "SSH_AUTH_SOCK=/ssh-agent"
    )
  fi

  if [[ -d "${HOME:-}/.ssh" ]]; then
    docker_args+=(
      -v "${HOME}/.ssh:/root/.ssh:ro"
    )
  fi

  if [[ -n "${RUSTFLAGS// }" ]]; then
    docker_args+=(-e "RUSTFLAGS=$RUSTFLAGS")
  fi

  local features_arg=""
  if [[ -n "${features// }" ]]; then
    features_arg="--features '$features'"
  fi

  local target_arg="-p '$package'"
  if [[ "$build_kind" == "bin" ]]; then
    target_arg="-p '$package' --bin '$bin_name'"
  fi

  run docker run "${docker_args[@]}" "$IMAGE_TAG" bash -lc \
    "export GIT_CONFIG_GLOBAL=/tmp/gitconfig; \
     : > \"\$GIT_CONFIG_GLOBAL\"; \
     cargo build --target '$target_triple' $profile_flag ${features_arg:+$features_arg }$target_arg"
}

binary_output_path() {
  local package="$1"
  local profile_flag="$2"
  local target_triple="$3"
  local bin_name="${4:-$package}"

  local profile_dir
  profile_dir=$(profile_dir_from_flag "$profile_flag")

  echo "$TARGET_BUILD_DIR/$target_triple/$profile_dir/$bin_name"
}

copy_binary_out() {
  local package="$1"
  local profile_flag="$2"
  local target_triple="$3"
  local out_dir="$4"
  local bin_name="${5:-$package}"

  local bin_path
  bin_path=$(binary_output_path "$package" "$profile_flag" "$target_triple" "$bin_name")
  if [[ ! -f "$bin_path" ]]; then
    die "expected output not found: $bin_path"
  fi

  run mkdir -p "$out_dir"
  run cp -f "$bin_path" "$out_dir/"
  local out_path="$out_dir/$(basename "$bin_path")"
  strip_debug_in_place "$out_path"
  echo "Wrote: $out_path"
}

strip_debug_in_place() {
  local path="$1"
  if [[ "$STRIP_DEBUG" != "1" ]]; then
    return 0
  fi
  if [[ ! -f "$path" ]]; then
    return 0
  fi

  if command -v eu-strip >/dev/null 2>&1; then
    run eu-strip -g "$path" || true
    return 0
  fi

  # GNU strip on some hosts doesn't understand aarch64; best effort only.
  if command -v strip >/dev/null 2>&1; then
    run strip --strip-debug "$path" || true
    return 0
  fi
}

copy_plugins_out() {
  local profile_flag="$1"
  local target_triple="$2"
  local out_dir="$3"

  local profile_dir
  profile_dir=$(profile_dir_from_flag "$profile_flag")

  local so_glob="$TARGET_BUILD_DIR/$target_triple/$profile_dir/libhelios_daedalus_*_plugin.so"
  shopt -s nullglob
  local so_files=( $so_glob )
  shopt -u nullglob

  if [[ "${#so_files[@]}" -eq 0 ]]; then
    die "no plugin .so files found at: $so_glob"
  fi

  run mkdir -p "$out_dir"
  local so
  for so in "${so_files[@]}"; do
    run cp -f "$so" "$out_dir/"
    local out_path="$out_dir/$(basename "$so")"
    strip_debug_in_place "$out_path"
    echo "Wrote: $out_path"
  done
}

latest_mtime() {
  local latest=0
  local path=""
  local mtime=""

  for path in "$@"; do
    if [[ -f "$path" ]]; then
      mtime=$(stat -c %Y "$path" 2>/dev/null || true)
      if [[ -n "$mtime" && "$mtime" -gt "$latest" ]]; then
        latest="$mtime"
      fi
    elif [[ -d "$path" ]]; then
      mtime=$(find "$path" -type f -printf '%T@\n' 2>/dev/null | sort -nr | head -1 | cut -d. -f1)
      if [[ -n "$mtime" && "$mtime" -gt "$latest" ]]; then
        latest="$mtime"
      fi
    fi
  done

  echo "$latest"
}

needs_binary_build() {
  local package="$1"
  local profile_flag="$2"
  local target_triple="$3"
  local bin_name="${4:-$package}"

  local bin_path
  bin_path=$(binary_output_path "$package" "$profile_flag" "$target_triple" "$bin_name")
  if [[ ! -f "$bin_path" ]]; then
    return 0
  fi

  local -a watch_paths=(
    "$ROOT_DIR/backend/Cargo.toml"
    "$ROOT_DIR/backend/Cargo.lock"
    "$ROOT_DIR/backend/src/libs"
    "$ROOT_DIR/backend/src/$package"
  )
  if [[ -n "${DAEDALUS_HOST_PATH// }" ]]; then
    watch_paths+=("$DAEDALUS_HOST_PATH")
  fi
  if [[ -n "${STYX_HOST_PATH// }" ]]; then
    watch_paths+=("$STYX_HOST_PATH")
  fi
  if [[ -n "${LIBCAMERA_RS_HOST_PATH// }" ]]; then
    watch_paths+=("$LIBCAMERA_RS_HOST_PATH")
  fi

  # `helios-api` links `helios-engine` as a library; rebuild API when engine sources change.
  # (This avoids stale deployments when only the shared engine crate changes.)
  case "$package" in
    helios-api)
      watch_paths+=("$ROOT_DIR/backend/src/helios-engine")
      ;;
  esac

  local latest
  latest=$(latest_mtime "${watch_paths[@]}")

  if [[ -z "$latest" ]]; then
    return 0
  fi

  local bin_mtime
  bin_mtime=$(stat -c %Y "$bin_path" 2>/dev/null || true)
  [[ -z "$bin_mtime" ]] && return 0

  if [[ "$latest" -gt "$bin_mtime" ]]; then
    return 0
  fi

  return 1
}

plugin_output_path() {
  local package="$1"
  local profile_flag="$2"
  local target_triple="$3"

  local profile_dir
  profile_dir=$(profile_dir_from_flag "$profile_flag")

  local crate_name="${package//-/_}"
  echo "$TARGET_BUILD_DIR/$target_triple/$profile_dir/lib${crate_name}.so"
}

needs_plugin_build() {
  local package="$1"
  local profile_flag="$2"
  local target_triple="$3"
  local output_path
  output_path=$(plugin_output_path "$package" "$profile_flag" "$target_triple")

  if [[ ! -f "$output_path" ]]; then
    return 0
  fi

  local -a watch_paths=(
    "$ROOT_DIR/backend/Cargo.toml"
    "$ROOT_DIR/backend/Cargo.lock"
    "$ROOT_DIR/backend/src/plugins/$package"
  )
  if [[ -n "${DAEDALUS_HOST_PATH// }" ]]; then
    watch_paths+=("$DAEDALUS_HOST_PATH")
  fi
  if [[ -n "${STYX_HOST_PATH// }" ]]; then
    watch_paths+=("$STYX_HOST_PATH")
  fi
  if [[ -n "${LIBCAMERA_RS_HOST_PATH// }" ]]; then
    watch_paths+=("$LIBCAMERA_RS_HOST_PATH")
  fi

  case "$package" in
    helios-daedalus-cv-plugin)
      watch_paths+=("$ROOT_DIR/backend/src/libs/lib-cv")
      ;;
    helios-daedalus-ai-plugin)
      watch_paths+=("$ROOT_DIR/backend/src/libs/lib-ai")
      ;;
    helios-daedalus-nt4-plugin)
      watch_paths+=("$ROOT_DIR/backend/src/libs/lib-nt4-plugin")
      ;;
    helios-daedalus-led-plugin)
      watch_paths+=("$ROOT_DIR/backend/src/libs/lib-led-plugin" "$ROOT_DIR/backend/src/libs/lib-led-animations")
      ;;
  esac

  local latest
  latest=$(latest_mtime "${watch_paths[@]}")
  if [[ -z "$latest" ]]; then
    return 0
  fi

  local out_mtime
  out_mtime=$(stat -c %Y "$output_path" 2>/dev/null || true)
  [[ -z "$out_mtime" ]] && return 0

  if [[ "$latest" -gt "$out_mtime" ]]; then
    return 0
  fi

  return 1
}

if [[ "$BUILD" == "1" ]]; then
  run mkdir -p "$TARGET_BUILD_DIR" "$CROSS_CARGO_HOME" "$CROSS_SCCACHE_DIR"
  ensure_docker_image

  if [[ "$do_plugins" == "1" ]]; then
    echo "Building Daedalus plugins ($TARGET_TRIPLE) $(profile_label)..."
    packages=(
      "helios-daedalus-cv-plugin"
      "helios-daedalus-ai-plugin"
      "helios-daedalus-nt4-plugin"
      "helios-daedalus-led-plugin"
    )
    pkg=""
    for pkg in "${packages[@]}"; do
      if needs_plugin_build "$pkg" "$PROFILE_FLAG" "$TARGET_TRIPLE"; then
        echo "- $pkg"
        docker_run_cargo_build "$pkg" "$TARGET_TRIPLE" "$PROFILE_FLAG" "$ROOT_DIR"
      else
        echo "- $pkg (up to date)"
      fi
    done
    copy_plugins_out "$PROFILE_FLAG" "$TARGET_TRIPLE" "$PLUGINS_DIR"
  fi

  if [[ "$do_binaries" == "1" ]]; then
    bin_specs=(
      "helios-engine:helios-engine"
      "helios-api:helios-api"
      "helios-api:helios-api-tools"
      "helios-peripherals:helios-peripherals"
      "helios-updater:helios-updater"
    )
    spec=""
    for spec in "${bin_specs[@]}"; do
      IFS=: read -r pkg bin_name <<<"$spec"
      if needs_binary_build "$pkg" "$PROFILE_FLAG" "$TARGET_TRIPLE" "$bin_name"; then
        echo "Building $bin_name ($TARGET_TRIPLE) $(profile_label)..."
        pkg_features=""
        if [[ "$pkg" == "helios-engine" ]]; then
          pkg_features="$ENGINE_FEATURES"
        elif [[ "$pkg" == "helios-api" ]]; then
          pkg_features="$API_FEATURES"
        fi
        docker_run_cargo_build "$pkg" "$TARGET_TRIPLE" "$PROFILE_FLAG" "$ROOT_DIR" "$pkg_features" "bin" "$bin_name"
      else
        echo "Skipping $bin_name (up to date)"
      fi
      copy_binary_out "$pkg" "$PROFILE_FLAG" "$TARGET_TRIPLE" "$BINS_DIR" "$bin_name"
    done
  fi
fi

if [[ "$UPLOAD" == "1" ]]; then
  if [[ "$do_plugins" == "1" ]]; then
    [[ -d "$PLUGINS_DIR" ]] || die "local plugin dir not found: $PLUGINS_DIR"
  fi

  if [[ "$do_binaries" == "1" ]]; then
    [[ -d "$BINS_DIR" ]] || die "local binaries dir not found: $BINS_DIR"
  fi

  ssh_opts=(
    -T
    -o StrictHostKeyChecking=no
    -o UserKnownHostsFile=/dev/null
    -o GlobalKnownHostsFile=/dev/null
  )
  control_path="/tmp/helios-cm5-%C"
  ssh_opts+=(
    -o ControlMaster=auto
    -o ControlPersist=60s
    -o ControlPath="$control_path"
    -o Compression=yes
  )

  ssh_exec() {
    if [[ -n "${SSH_PASS// }" ]]; then
      run sshpass -p "$SSH_PASS" ssh "${ssh_opts[@]}" "$SSH_TARGET" "$@"
    else
      run ssh "${ssh_opts[@]}" "$SSH_TARGET" "$@"
    fi
  }

  ssh_control_cleanup() {
    if [[ "$DRY_RUN" == "1" ]]; then
      return 0
    fi
    if [[ -n "${SSH_PASS// }" ]]; then
      sshpass -p "$SSH_PASS" ssh "${ssh_opts[@]}" -O exit "$SSH_TARGET" >/dev/null 2>&1 || true
    else
      ssh "${ssh_opts[@]}" -O exit "$SSH_TARGET" >/dev/null 2>&1 || true
    fi
  }
  trap ssh_control_cleanup EXIT

  ssh_cat_upload() {
    local local_path="$1"
    local remote_path="$2"
    if [[ "$DRY_RUN" == "1" ]]; then
      printf '+ upload %q -> %q:%q\n' "$local_path" "$SSH_TARGET" "$remote_path"
      return 0
    fi
    if [[ -n "${SSH_PASS// }" ]]; then
      sshpass -p "$SSH_PASS" ssh "${ssh_opts[@]}" "$SSH_TARGET" "sh -lc 'cat > \"$remote_path\"'" <"$local_path"
    else
      ssh "${ssh_opts[@]}" "$SSH_TARGET" "sh -lc 'cat > \"$remote_path\"'" <"$local_path"
    fi
  }

  ssh_upload_tar() {
    local base_dir="$1"
    local remote_dir="$2"
    shift 2
    local -a files=( "$@" )
    if [[ "${#files[@]}" -eq 0 ]]; then
      return 0
    fi
    if [[ "$DRY_RUN" == "1" ]]; then
      printf '+ tar -C %q -cf -' "$base_dir"
      local f
      for f in "${files[@]}"; do
        printf ' %q' "$f"
      done
      printf ' | ssh %q %q\n' "$SSH_TARGET" "tar -x -o -f - -C \"$remote_dir\""
      return 0
    fi
    if [[ -n "${SSH_PASS// }" ]]; then
      tar -C "$base_dir" -cf - "${files[@]}" | \
        sshpass -p "$SSH_PASS" ssh "${ssh_opts[@]}" "$SSH_TARGET" "sh -lc 'tar -x -o -f - -C \"$remote_dir\"'"
    else
      tar -C "$base_dir" -cf - "${files[@]}" | \
        ssh "${ssh_opts[@]}" "$SSH_TARGET" "sh -lc 'tar -x -o -f - -C \"$remote_dir\"'"
    fi
  }

  # Validate that a remote path looks like a real executable (non-empty, ELF magic).
  # Busybox/coreutils availability varies on target; avoid relying on `stat`.
  remote_validate_elf() {
    local remote_path="$1"
    ssh_exec "sh -lc 'set -e; \
      [ -s \"$remote_path\" ]; \
      head -c 4 \"$remote_path\" | hexdump -C | head -n 1 | grep -q \"7f 45 4c 46\"'"
  }

  upload_and_install_bins() {
    local remote_dir="$1"
    shift
    local -a bins=( "$@" )
    if [[ "${#bins[@]}" -eq 0 ]]; then
      return 0
    fi

    # Extract into a temp dir first so a partial transfer can't brick /usr/bin.
    local ts tmpdir backup_dir staging_root
    ts="$(date +%s)"
    staging_root="/var/lib/helios/deploy-staging"
    tmpdir="$staging_root/bins.$ts"
    backup_dir="/var/lib/helios/deploy-backups/bins-$ts"

    ssh_exec "sh -lc 'set -e; install -d -m0755 \"$staging_root\"; rm -rf \"$tmpdir\"; install -d -m0755 \"$tmpdir\"'"
    ssh_upload_tar "$BINS_DIR" "$tmpdir" "${bins[@]}"

    # Validate all uploads before touching the live paths.
    local b
    for b in "${bins[@]}"; do
      remote_validate_elf "$tmpdir/$b"
    done

    # Keep a copy of previous binaries for quick rollback if needed.
    ssh_exec "sh -lc 'set -e; install -d -m0755 \"${backup_dir%/*}\" \"$backup_dir\"'"
    for b in "${bins[@]}"; do
      ssh_exec "sh -lc 'set -e; \
        if [ -f \"$remote_dir/$b\" ]; then cp -f \"$remote_dir/$b\" \"$backup_dir/$b\" || true; fi; \
        install -m0755 \"$tmpdir/$b\" \"$remote_dir/$b\"'"
    done
    ssh_exec "sh -lc 'rm -rf \"$tmpdir\"'"
  }

  ensure_remote_plugin_env() {
    # Keep engine/api pointed at both the writable deploy plugin dir and the system plugin dir.
    # Do not set HELIOS_DAEDALUS_PLUGIN_DIR here: that single-dir override masks system plugins.
    local plugin_dirs="${PLUGIN_DIR_REMOTE}:/usr/lib/helios/plugins/daedalus"
    ssh_exec "sh -lc 'install -d -m0755 \"$PLUGIN_DIR_REMOTE\"; \
      for f in /etc/default/helios-engine /etc/default/helios-api; do \
        touch \"\$f\"; \
        sed -i \"/^HELIOS_DAEDALUS_PLUGIN_DIR=/d\" \"\$f\"; \
        if grep -q \"^HELIOS_DAEDALUS_PLUGIN_DIRS=\" \"\$f\"; then \
          sed -i \"s|^HELIOS_DAEDALUS_PLUGIN_DIRS=.*|HELIOS_DAEDALUS_PLUGIN_DIRS=$plugin_dirs|\" \"\$f\"; \
        else \
          echo \"HELIOS_DAEDALUS_PLUGIN_DIRS=$plugin_dirs\" >> \"\$f\"; \
        fi; \
      done'"
  }

  local_sha256() {
    sha256sum "$1" | awk '{print $1}'
  }

  remote_sha256() {
    local remote_path="$1"
    ssh_exec "sh -lc 'sha256sum \"$remote_path\" 2>/dev/null | awk \"{print \\$1}\"'"
  }

  should_upload() {
    if [[ "$FAST_UPLOAD" == "1" ]]; then
      return 0
    fi
    local local_path="$1"
    local remote_path="$2"
    local local_hash=""
    local remote_hash=""

    if [[ "$DRY_RUN" == "1" ]]; then
      return 0
    fi

    local_hash="$(local_sha256 "$local_path")"
    remote_hash="$(remote_sha256 "$remote_path")"

    if [[ -z "${remote_hash// }" ]]; then
      return 0
    fi

    [[ "$local_hash" != "$remote_hash" ]]
  }

  if [[ "$do_binaries" == "1" ]]; then
    bins=("helios-engine" "helios-api" "helios-api-tools" "helios-peripherals" "helios-updater")
    b=""
    for b in "${bins[@]}"; do
      [[ -f "$BINS_DIR/$b" ]] || die "missing binary: $BINS_DIR/$b"
    done

    bins_to_upload=()
    if [[ "$FAST_UPLOAD" == "1" ]]; then
      bins_to_upload=( "${bins[@]}" )
    else
      for b in "${bins[@]}"; do
        if should_upload "$BINS_DIR/$b" "$BIN_DIR_REMOTE/$b"; then
          bins_to_upload+=("$b")
        fi
      done
    fi

    if [[ "${#bins_to_upload[@]}" -gt 0 ]]; then
      echo "Stopping services on $SSH_TARGET..."
      # Do not stop peripherals/updater here; on some devices peripherals owns the USB gadget/network.
      # Stopping it can drop the SSH link mid-deploy.
      ssh_exec "systemctl stop helios-api.service helios-engine.service || true"
    else
      echo "Binaries unchanged; skipping binary upload."
    fi
  fi

  if [[ "$UPLOAD_TEMPLATES" == "1" ]]; then
    if [[ ! -d "$TEMPLATES_DIR_LOCAL" ]]; then
      die "local templates dir not found: $TEMPLATES_DIR_LOCAL"
    fi
    shopt -s nullglob
    templates=( "$TEMPLATES_DIR_LOCAL"/*.json )
    shopt -u nullglob
    if [[ "${#templates[@]}" -gt 0 ]]; then
      echo "Uploading ${#templates[@]} template(s) -> $SSH_TARGET:$TEMPLATES_DIR_REMOTE"
      ssh_exec "install -d -m0755 '$TEMPLATES_DIR_REMOTE'"
      tmpl_names=()
      for t in "${templates[@]}"; do
        tmpl_names+=( "$(basename "$t")" )
      done
      ssh_upload_tar "$TEMPLATES_DIR_LOCAL" "$TEMPLATES_DIR_REMOTE" "${tmpl_names[@]}"
      ssh_exec "chmod 0644 $(
        for t in "${tmpl_names[@]}"; do
          printf '%q ' "$TEMPLATES_DIR_REMOTE/$t"
        done
      )"
    fi
  fi

  if [[ "$UPLOAD_FRONTEND" == "1" ]]; then
    if [[ ! -d "$FRONTEND_DIR_LOCAL" ]]; then
      die "local frontend dir not found: $FRONTEND_DIR_LOCAL"
    fi
    echo "Uploading frontend -> $SSH_TARGET:$FRONTEND_DIR_REMOTE"
    ssh_exec "install -d -m0755 '$FRONTEND_DIR_REMOTE'"
    # Clear old build artifacts so removed files don't linger.
    ssh_exec "sh -lc 'rm -rf \"$FRONTEND_DIR_REMOTE\"/*'"
    ssh_upload_tar "$FRONTEND_DIR_LOCAL" "$FRONTEND_DIR_REMOTE" "."
    ssh_exec "sh -lc 'chmod -R a+rX \"$FRONTEND_DIR_REMOTE\"'"
  fi

  if [[ "$do_plugins" == "1" ]]; then
    shopt -s nullglob
    plugins=( "$PLUGINS_DIR"/*.so )
    shopt -u nullglob
    [[ "${#plugins[@]}" -gt 0 ]] || die "no .so files found in: $PLUGINS_DIR"

    any_plugin_uploaded="0"
    plugins_to_upload=()

    so=""
    for so in "${plugins[@]}"; do
      name="$(basename "$so")"
      if [[ "$FAST_UPLOAD" == "1" ]]; then
        if [[ "$any_plugin_uploaded" == "0" ]]; then
          echo "Uploading ${#plugins[@]} plugin(s) -> $SSH_TARGET:$PLUGIN_DIR_REMOTE"
          ssh_exec "install -d -m0755 '$PLUGIN_DIR_REMOTE'"
        fi
        echo "- $name (changed)"
        plugins_to_upload+=( "$name" )
        any_plugin_uploaded="1"
      else
        if should_upload "$so" "$PLUGIN_DIR_REMOTE/$name"; then
          if [[ "$any_plugin_uploaded" == "0" ]]; then
            echo "Uploading ${#plugins[@]} plugin(s) -> $SSH_TARGET:$PLUGIN_DIR_REMOTE"
            ssh_exec "install -d -m0755 '$PLUGIN_DIR_REMOTE'"
          fi
          echo "- $name (changed)"
          plugins_to_upload+=( "$name" )
          any_plugin_uploaded="1"
        else
          echo "- $name (unchanged)"
        fi
      fi
    done
    if [[ "${#plugins_to_upload[@]}" -gt 0 ]]; then
      # Avoid updating .so files while services are running.
      # (Engine/API restarts happen below, but stopping up front prevents in-place file changes.)
      ssh_exec "systemctl stop helios-api.service helios-engine.service || true"
      ssh_upload_tar "$PLUGINS_DIR" "$PLUGIN_DIR_REMOTE" "${plugins_to_upload[@]}"
      ssh_exec "chmod 0644 $(
        for so in "${plugins_to_upload[@]}"; do
          printf '%q ' "$PLUGIN_DIR_REMOTE/$so"
        done
      )"
      ensure_remote_plugin_env
    fi

    if [[ "$do_binaries" != "1" && "$RESTART_SERVICES" != "0" ]]; then
      if [[ "$any_plugin_uploaded" == "1" ]]; then
        echo "Restarting services (engine/api)..."
        ssh_exec "systemctl restart helios-engine.service helios-api.service"
      else
        echo "No plugin changes; skipping restart."
      fi
    elif [[ "$do_binaries" != "1" ]]; then
      echo "Skipping restart (RESTART_SERVICES=0)"
    fi
  fi

  if [[ "$do_binaries" == "1" ]]; then
    if [[ "${#bins_to_upload[@]}" -gt 0 ]]; then
      echo "Uploading ${#bins_to_upload[@]} bin(s) -> $SSH_TARGET:$BIN_DIR_REMOTE"
      ssh_exec "install -d -m0755 '$BIN_DIR_REMOTE'"
      upload_and_install_bins "$BIN_DIR_REMOTE" "${bins_to_upload[@]}"

      echo "Ensuring plugin env in /etc/default..."
      ensure_remote_plugin_env
      if [[ "$PROFILE_FLAG" == "--release" ]]; then
        echo "Disabling debug logging for release..."
        ssh_exec "sh -lc 'f=\"/etc/default/helios-engine\"; \
          touch \"\$f\"; \
          sed -i \"/^DAEDALUS_HOST_BRIDGE_TRACE=/d\" \"\$f\"; \
          # Clear any leftover tracing/profiling knobs that can drastically impact performance. \
          sed -i \"/^DAEDALUS_TRACE_/d\" \"\$f\"; \
          sed -i \"/^HELIOS_PERF_COUNTERS=/d\" \"\$f\"; \
          sed -i \"/^HELIOS_PPROF=/d\" \"\$f\"; \
          sed -i \"/^HELIOS_PPROF_/d\" \"\$f\"; \
          sed -i \"/^HELIOS_HOST_OUTPUT_DEBUG=/d\" \"\$f\"; \
          sed -i \"/^HELIOS_DAEDALUS_HOST_OUTPUTS_IN_GRAPH=/d\" \"\$f\"; \
          sed -i \"/^HELIOS_DAEDALUS_DEMAND_DRIVEN=/d\" \"\$f\"; \
          if grep -q \"^RUST_LOG=\" \"\$f\"; then \
            sed -i \"s/^RUST_LOG=.*/RUST_LOG=info/\" \"\$f\"; \
          else \
            echo \"RUST_LOG=info\" >> \"\$f\"; \
          fi'"
      fi

      if [[ "$RESTART_SERVICES" != "0" ]]; then
        echo "Restarting services..."
        # Restart engine+api first.
        ssh_exec "systemctl daemon-reload || true; systemctl restart helios-engine.service helios-api.service"
        # Restart peripherals in a best-effort way as well so updated IMU/power code is actually active.
        # On some setups this may briefly impact the USB gadget/network link; don't fail the deploy if so.
        if ! ssh_exec "systemctl restart helios-peripherals.service"; then
          echo "Warning: failed to restart helios-peripherals.service; binary was uploaded but service may still be running old code."
        fi
        if ! ssh_exec "systemctl restart helios-updater.service"; then
          echo "Warning: failed to restart helios-updater.service; binary was uploaded but service may still be running old code."
        fi
      else
        echo "Skipping restart (RESTART_SERVICES=0)"
      fi
    fi
  fi
fi

echo "Done."
