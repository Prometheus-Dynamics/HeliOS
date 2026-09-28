#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OTA_RELEASE_PUBLISHER="$ROOT_DIR/tools/ota_release_publisher.py"

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
# In a clean/public clone, leave unset and the script will use the pinned upstream revisions.
DAEDALUS_HOST_PATH="${DAEDALUS_HOST_PATH:-}"
STYX_HOST_PATH="${STYX_HOST_PATH:-}"
LIBCAMERA_RS_HOST_PATH="${LIBCAMERA_RS_HOST_PATH:-}"
HELIOS_CARGO_CONFIG="${HELIOS_CARGO_CONFIG:-}"

BINS_DIR_DEFAULT="$ROOT_DIR/output/cm5/binaries"
BINS_DIR="$BINS_DIR_DEFAULT"

FRONTEND_DIR_LOCAL="${FRONTEND_DIR_LOCAL:-$ROOT_DIR/frontend/build}"
OTA_BASE_URL="${OTA_BASE_URL:-}"
OTA_REQUESTED_BY="${OTA_REQUESTED_BY:-deploy-live}"
BINARY_REVISION="${BINARY_REVISION:-}"

ONLY="all" # all|binaries|frontend
BUILD="1"
UPLOAD="1"
RESTART_SERVICES="1"
DRY_RUN="0"
UPLOAD_FRONTEND="1"
STRIP_DEBUG="1"
REQUIRES_SSH="0"

usage() {
  cat <<EOF
Build + deploy Helios CM5 binaries and frontend assets to a device.
Binary deploys upload over SSH and activate through helios-updater-managed service revisions.

Usage:
  ./tools/deploy-live.sh [options]

Options:
  --ssh <user@host>     SSH target (default: $SSH_TARGET_DEFAULT)
  --pass <password>     Optional SSH password (uses sshpass)
  --release|--debug|--dev-release
                        Build profile (default: dev-release)
  --engine-features <f> Cargo features for helios-engine (default: $ENGINE_FEATURES)
  --api-features <f>   Cargo features for helios-api (default: $API_FEATURES)
  --only <what>         all|binaries|frontend (default: all)
  --no-build            Skip build; only upload/restart
  --no-upload           Only build; skip upload/restart
  --no-restart          Upload but do not restart services
  --bins-dir <dir>      Local binaries dir (default: $BINS_DIR_DEFAULT)
  --no-frontend         Skip uploading frontend assets
  --frontend-dir <dir>  Local frontend build dir (default: $FRONTEND_DIR_LOCAL)
  --ota-base-url <url>  OTA API base URL (default: derived from --ssh as http://host/v1)
  --binary-revision <r> Explicit revision string for binary activation (default: generated)
  --no-strip            Do not strip debug sections from built artifacts before upload
  --target <triple>     Rust target triple (default: $TARGET_TRIPLE)
  --dockerfile <path>   Cross-builder dockerfile (default: $DOCKERFILE)
  --docker-context <p>  Docker build context for cross image (default: $DOCKER_CONTEXT)
  --image-tag <tag>     Docker image tag (default: $IMAGE_TAG)
  --rebuild-image       Force rebuild of docker image (once)
  --dry-run             Print actions without executing
  -h, --help            Show this help

Env vars (optional):
  RUSTFLAGS             Passed through to build scripts
  OTA_BASE_URL           OTA API base URL override
  BINARY_REVISION        Explicit revision string for binary activation
  DAEDALUS_HOST_PATH     Host path to a Daedalus checkout (optional dev override)
  STYX_HOST_PATH         Host path to a Styx checkout (optional dev override)
  LIBCAMERA_RS_HOST_PATH Host path to a libcamera-rs checkout (optional dev override)
  HELIOS_CARGO_CONFIG    Host path to an explicit cargo config override passed as `cargo --config`
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

ssh_target_host() {
  local target="$1"
  target="${target##*@}"
  printf '%s\n' "$target"
}

default_ota_base_url() {
  local host
  host="$(ssh_target_host "$SSH_TARGET")"
  printf 'http://%s/v1\n' "$host"
}

if [[ -z "${OTA_BASE_URL// }" ]]; then
  OTA_BASE_URL="$(default_ota_base_url)"
fi

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
    --no-build) BUILD="0"; shift ;;
    --no-upload) UPLOAD="0"; shift ;;
    --no-restart) RESTART_SERVICES="0"; shift ;;
    --bins-dir) BINS_DIR="${2:-}"; shift 2 ;;
    --no-frontend) UPLOAD_FRONTEND="0"; shift ;;
    --frontend-dir) FRONTEND_DIR_LOCAL="${2:-}"; shift 2 ;;
    --bin-dir|--frontend-remote)
      shift 2 || die "$1 requires a value"
      ;;
    --binaries-via-ota|--frontend-via-ota)
      shift
      ;;
    --binaries-via-ssh|--frontend-via-ssh)
      die "$1 has been removed; binaries and frontend now publish only through OTA"
      ;;
    --ota-base-url) OTA_BASE_URL="${2:-}"; shift 2 ;;
    --binary-revision) BINARY_REVISION="${2:-}"; shift 2 ;;
    --no-strip) STRIP_DEBUG="0"; shift ;;
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
  all|binaries|frontend) ;;
  *) die "--only must be one of: all, binaries, frontend" ;;
esac

do_binaries="0"
do_frontend="0"
case "$ONLY" in
  all) do_binaries="1"; do_frontend="1" ;;
  binaries) do_binaries="1" ;;
  frontend) do_frontend="1" ;;
esac

docker_image_built="0"

ensure_deps() {
  if [[ "$BUILD" == "1" ]]; then
    command -v docker >/dev/null 2>&1 || die "docker is required for build"
    if [[ "$UPLOAD_FRONTEND" == "1" && "$do_frontend" == "1" ]]; then
      command -v bun >/dev/null 2>&1 || die "bun is required to build the frontend"
    fi
    if [[ -n "${DAEDALUS_HOST_PATH// }" ]]; then
      [[ -d "$DAEDALUS_HOST_PATH" ]] || die "missing DAEDALUS_HOST_PATH dir: $DAEDALUS_HOST_PATH"
    fi
    if [[ -n "${STYX_HOST_PATH// }" ]]; then
      [[ -d "$STYX_HOST_PATH" ]] || die "missing STYX_HOST_PATH dir: $STYX_HOST_PATH"
    fi
    if [[ -n "${LIBCAMERA_RS_HOST_PATH// }" ]]; then
      [[ -d "$LIBCAMERA_RS_HOST_PATH" ]] || die "missing LIBCAMERA_RS_HOST_PATH dir: $LIBCAMERA_RS_HOST_PATH"
    fi
    if [[ -n "${HELIOS_CARGO_CONFIG// }" ]]; then
      HELIOS_CARGO_CONFIG="$(readlink -f "$HELIOS_CARGO_CONFIG")"
      [[ -f "$HELIOS_CARGO_CONFIG" ]] || die "missing HELIOS_CARGO_CONFIG file: $HELIOS_CARGO_CONFIG"
    fi
  fi
  if [[ "$UPLOAD" == "1" ]]; then
    needs_ssh="0"
    if [[ "$do_binaries" == "1" ]]; then
      needs_ssh="1"
    fi
    if [[ "$needs_ssh" == "1" ]]; then
      REQUIRES_SSH="1"
      command -v ssh >/dev/null 2>&1 || die "ssh is required for the selected upload paths"
      if [[ -n "${SSH_PASS// }" ]]; then
        command -v sshpass >/dev/null 2>&1 || die "sshpass is required when using --pass"
      fi
    fi
    if [[ "$UPLOAD_FRONTEND" == "1" && "$do_frontend" == "1" ]]; then
      command -v curl >/dev/null 2>&1 || die "curl is required for frontend OTA deploys"
      command -v python3 >/dev/null 2>&1 || die "python3 is required for frontend OTA deploys"
      [[ -f "$OTA_RELEASE_PUBLISHER" ]] || die "missing OTA release publisher: $OTA_RELEASE_PUBLISHER"
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

  local cargo_prefix="cargo"
  if [[ -n "${HELIOS_CARGO_CONFIG// }" ]]; then
    docker_args+=(-v "$HELIOS_CARGO_CONFIG:/tmp/helios-local-cargo-config.toml:ro")
    cargo_prefix="cargo --config '/tmp/helios-local-cargo-config.toml'"
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
     $cargo_prefix build --target '$target_triple' $profile_flag ${features_arg:+$features_arg }$target_arg"
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
      watch_paths+=("$ROOT_DIR/backend/src/helios/engine")
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

if [[ "$BUILD" == "1" ]]; then
  if [[ "$do_binaries" == "1" ]]; then
    run mkdir -p "$TARGET_BUILD_DIR" "$CROSS_CARGO_HOME" "$CROSS_SCCACHE_DIR"
    ensure_docker_image
  fi

  if [[ "$do_binaries" == "1" ]]; then
    bin_specs=(
      "helios-engine:helios-engine"
      "helios-api:helios-api"
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

  if [[ "$UPLOAD_FRONTEND" == "1" && "$do_frontend" == "1" ]]; then
    echo "Building frontend bundle..."
    run_with_env SKIP_DOCS_SYNC=1 -- bash -lc "cd '$ROOT_DIR/frontend' && bun run build"
  fi
fi

if [[ "$UPLOAD" == "1" ]]; then
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
    if [[ "$REQUIRES_SSH" != "1" ]]; then
      return 0
    fi
    if [[ -n "${SSH_PASS// }" ]]; then
      sshpass -p "$SSH_PASS" ssh "${ssh_opts[@]}" -O exit "$SSH_TARGET" >/dev/null 2>&1 || true
    else
      ssh "${ssh_opts[@]}" -O exit "$SSH_TARGET" >/dev/null 2>&1 || true
    fi
  }
  trap ssh_control_cleanup EXIT

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

  ota_publish_release() {
    local artifact_kind="$1"
    local source_dir="$2"
    local post_url="$3"
    shift 3
    local -a bundle_entries=( "$@" )
    local -a cmd=(
      python3
      "$OTA_RELEASE_PUBLISHER"
      publish
      --artifact-kind "$artifact_kind"
      --base-url "$OTA_BASE_URL"
      --requested-by "$OTA_REQUESTED_BY"
      --source-dir "$source_dir"
      --post-url "$post_url"
    )
    local entry
    for entry in "${bundle_entries[@]}"; do
      cmd+=( --entry "$entry" )
    done
    if [[ "$DRY_RUN" == "1" ]]; then
      cmd+=( --dry-run )
    fi
    run "${cmd[@]}"
  }

  if [[ "$do_binaries" == "1" ]]; then
    bins=("helios-engine" "helios-api" "helios-peripherals" "helios-updater")
    b=""
    for b in "${bins[@]}"; do
      [[ -f "$BINS_DIR/$b" ]] || die "missing binary: $BINS_DIR/$b"
    done

    bins_to_upload=( "${bins[@]}" )

    if [[ "${#bins_to_upload[@]}" -gt 0 ]]; then
      echo "Preparing ${#bins_to_upload[@]} binary artifact(s) for live updater activation..."
    else
      echo "Binaries unchanged; skipping binary upload."
    fi
  fi

  if [[ "$UPLOAD_FRONTEND" == "1" && "$do_frontend" == "1" ]]; then
    if [[ ! -d "$FRONTEND_DIR_LOCAL" ]]; then
      die "local frontend dir not found: $FRONTEND_DIR_LOCAL"
    fi
    echo "Publishing frontend bundle via OTA -> $OTA_BASE_URL"
    frontend_root_url=""
    frontend_root_url="${OTA_BASE_URL%/v1}/"
    if [[ "$frontend_root_url" == "$OTA_BASE_URL/" ]]; then
      frontend_root_url="${OTA_BASE_URL%/}/"
    fi
    ota_publish_release "frontend_bundle" "$FRONTEND_DIR_LOCAL" "$frontend_root_url" "."
  fi

  if [[ "$do_binaries" == "1" ]]; then
    if [[ "${#bins_to_upload[@]}" -gt 0 ]]; then
      revision="$BINARY_REVISION"
      if [[ -z "${revision// }" ]]; then
        revision="dev-$(date +%Y%m%d-%H%M%S)"
      fi
      remote_incoming="/var/lib/helios/releases/incoming/$revision"
      echo "Uploading ${#bins_to_upload[@]} binary artifact(s) -> $SSH_TARGET:$remote_incoming"
      ssh_exec "install -d -m0755 '$remote_incoming'"
      ssh_upload_tar "$BINS_DIR" "$remote_incoming" "${bins_to_upload[@]}"
      ssh_exec "chmod 0755 $(
        for b in "${bins_to_upload[@]}"; do
          printf '%q ' "$remote_incoming/$b"
        done
      )"

      for b in "${bins_to_upload[@]}"; do
        echo "Activating $b -> revision $revision"
        activate_cmd="helios-updater service activate --name \"$b\" --revision \"$revision\""
        if [[ "$RESTART_SERVICES" == "0" ]]; then
          activate_cmd="$activate_cmd --restart false"
        fi
        ssh_exec "sh -lc 'helios-updater service stage --name \"$b\" --revision \"$revision\" --binary \"$remote_incoming/$b\" && $activate_cmd'"
      done
    fi
  fi
fi

echo "Done."
