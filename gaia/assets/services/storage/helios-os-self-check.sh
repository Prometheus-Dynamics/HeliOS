#!/bin/sh
set -eu

RUN_DIR=/run/helios
STATE_DIR=/var/lib/helios/state
RUN_REPORT="${RUN_DIR}/os-self-check.env"
STATE_REPORT="${STATE_DIR}/os-self-check.env"

repairs=""
failures=""

append_code() {
  var_name="$1"
  code="$2"
  eval "current=\${$var_name:-}"
  case ",${current}," in
    *,"${code}",*) return 0 ;;
  esac
  if [ -n "${current}" ]; then
    eval "$var_name=\${current},${code}"
  else
    eval "$var_name=${code}"
  fi
}

mount_fs_type() {
  mount_point="$1"
  awk -v target="${mount_point}" '$2 == target { print $3; exit }' /proc/mounts
}

is_mountpoint() {
  mount_point="$1"
  awk -v target="${mount_point}" '$2 == target { found=1 } END { exit found ? 0 : 1 }' /proc/mounts
}

ensure_base_dirs() {
  mkdir -p "${RUN_DIR}" /var/log/helios /var/lib/helios || return 1
  return 0
}

attempt_mount_data() {
  [ -e /dev/disk/by-label/DATA ] || return 0
  is_mountpoint /var/lib/helios && return 0
  mkdir -p /var/lib/helios || return 1
  if mount /dev/disk/by-label/DATA /var/lib/helios 2>/dev/null; then
    append_code repairs data_partition_mounted
    return 0
  fi
  append_code failures data_partition_mount_failed
  return 1
}

attempt_restore_overlay_bind() {
  [ "$(mount_fs_type /)" = "overlay" ] || return 0
  is_mountpoint /var/lib/helios || return 0
  is_mountpoint /.overlay-data && return 0
  mkdir -p /.overlay-data || return 1
  if mount --bind /var/lib/helios /.overlay-data 2>/dev/null; then
    append_code repairs overlay_data_bound
    return 0
  fi
  append_code failures overlay_data_bind_failed
  return 1
}

ensure_state_dirs() {
  created=0
  for dir in \
    /var/lib/helios/state \
    /var/lib/helios/journal \
    /var/lib/helios/api-data \
    /var/lib/helios/ota \
    /var/lib/helios/diagnostics \
    /var/lib/helios/plugins/daedalus
  do
    [ -d "${dir}" ] || created=1
  done

  if mkdir -p \
    /var/lib/helios/state \
    /var/lib/helios/journal \
    /var/lib/helios/api-data \
    /var/lib/helios/ota \
    /var/lib/helios/diagnostics \
    /var/lib/helios/plugins/daedalus \
    /var/log/helios
  then
    [ "${created}" -eq 0 ] || append_code repairs helios_state_dirs_created
    return 0
  fi

  append_code failures helios_state_dirs_failed
  return 1
}

check_required_binaries() {
  [ -x /usr/bin/helios-api ] || append_code failures helios_api_missing
  [ -x /usr/bin/helios-api-tools ] || append_code failures helios_api_tools_missing
  [ -x /usr/bin/helios-engine ] || append_code failures helios_engine_missing
  [ -x /usr/bin/helios-peripherals ] || append_code failures helios_peripherals_missing
  [ -x /usr/bin/helios-updater ] || append_code failures helios_updater_missing
}

write_report() {
  checked_at_ms="$(date +%s%3N 2>/dev/null || true)"
  case "${checked_at_ms}" in
    ""|*%3N*)
      checked_at_ms="$(busybox date +%s000 2>/dev/null || date +%s000 2>/dev/null || printf '0')"
      ;;
  esac
  root_fs_type="$(mount_fs_type / || true)"
  data_fs_type="$(mount_fs_type /var/lib/helios || true)"
  overlay_data_fs_type="$(mount_fs_type /.overlay-data || true)"

  mkdir -p "${RUN_DIR}" "${STATE_DIR}" 2>/dev/null || true

  {
    printf 'CHECKED_AT_MS=%s\n' "${checked_at_ms}"
    printf 'ROOT_FS_TYPE=%s\n' "${root_fs_type}"
    printf 'DATA_FS_TYPE=%s\n' "${data_fs_type}"
    printf 'OVERLAY_DATA_FS_TYPE=%s\n' "${overlay_data_fs_type}"
    printf 'REPAIRS=%s\n' "${repairs}"
    printf 'FAILURES=%s\n' "${failures}"
  } > "${RUN_REPORT}"

  cp "${RUN_REPORT}" "${STATE_REPORT}" 2>/dev/null || true
}

ensure_base_dirs || append_code failures helios_state_dirs_failed
attempt_mount_data || true
attempt_restore_overlay_bind || true
ensure_state_dirs || true
check_required_binaries || true
write_report

[ -z "${failures}" ]
