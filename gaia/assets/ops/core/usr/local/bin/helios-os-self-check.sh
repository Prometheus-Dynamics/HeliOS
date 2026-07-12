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
    /var/lib/helios/identity/ssh \
    /var/lib/helios/bin \
    /var/lib/helios/ota \
    /var/lib/helios/diagnostics \
    /var/lib/helios/updater \
    /var/lib/helios/root-overlay/root-a/upper \
    /var/lib/helios/root-overlay/root-a/work \
    /var/lib/helios/root-overlay/root-b/upper \
    /var/lib/helios/root-overlay/root-b/work
  do
    [ -d "${dir}" ] || created=1
  done

  if mkdir -p \
    /var/lib/helios/state \
    /var/lib/helios/journal \
    /var/lib/helios/identity/ssh \
    /var/lib/helios/bin \
    /var/lib/helios/ota \
    /var/lib/helios/diagnostics \
    /var/lib/helios/updater \
    /var/lib/helios/root-overlay/root-a/upper \
    /var/lib/helios/root-overlay/root-a/work \
    /var/lib/helios/root-overlay/root-b/upper \
    /var/lib/helios/root-overlay/root-b/work \
    /var/log/helios
  then
    [ "${created}" -eq 0 ] || append_code repairs helios_state_dirs_created
    return 0
  fi

  append_code failures helios_state_dirs_failed
  return 1
}

ensure_persistent_journal() {
  is_mountpoint /var/lib/helios || return 0
  mkdir -p /var/lib/helios/journal /var/log/journal || return 1
  is_mountpoint /var/log/journal && return 0
  if mount --bind /var/lib/helios/journal /var/log/journal 2>/dev/null; then
    append_code repairs journal_bound_to_data
    return 0
  fi
  append_code failures journal_bind_failed
  return 1
}

migrate_identity_file() {
  src="$1"
  dst="$2"

  if [ ! -e "${dst}" ] && [ -e "${src}" ] && [ ! -L "${src}" ]; then
    mkdir -p "$(dirname "${dst}")" || return 1
    cp -a "${src}" "${dst}" || return 1
  fi

  rm -f "${src}" || return 1
  ln -s "${dst}" "${src}" || return 1
  return 0
}

ensure_persistent_identity() {
  is_mountpoint /var/lib/helios || return 0
  mkdir -p /var/lib/helios/identity/ssh /etc/ssh || return 1

  if migrate_identity_file /etc/machine-id /var/lib/helios/identity/machine-id; then
    append_code repairs identity_machine_id_persisted
  else
    append_code failures identity_machine_id_failed
  fi

  if command -v ssh-keygen >/dev/null 2>&1; then
    ssh-keygen -A >/dev/null 2>&1 || append_code failures identity_ssh_keygen_failed
  fi

  for key_name in \
    ssh_host_rsa_key \
    ssh_host_rsa_key.pub \
    ssh_host_ecdsa_key \
    ssh_host_ecdsa_key.pub \
    ssh_host_ed25519_key \
    ssh_host_ed25519_key.pub
  do
    if migrate_identity_file "/etc/ssh/${key_name}" "/var/lib/helios/identity/ssh/${key_name}"; then
      append_code repairs identity_ssh_keys_persisted
    else
      append_code failures identity_ssh_keys_failed
      break
    fi
  done
}

check_required_binaries() {
  [ -x /usr/bin/helios-api ] || append_code failures helios_api_missing
  [ -x /usr/bin/helios-engine ] || append_code failures helios_engine_missing
  [ -x /usr/bin/helios-peripherals ] || append_code failures helios_peripherals_missing
  [ -x /usr/bin/helios-updater ] || append_code failures helios_updater_missing
  [ -x /var/lib/helios/bin/helios-api ] || append_code failures helios_api_managed_missing
  [ -x /var/lib/helios/bin/helios-engine ] || append_code failures helios_engine_managed_missing
  [ -x /var/lib/helios/bin/helios-peripherals ] || append_code failures helios_peripherals_managed_missing
  [ -x /var/lib/helios/bin/helios-updater ] || append_code failures helios_updater_managed_missing
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
ensure_persistent_journal || true
ensure_persistent_identity || true
check_required_binaries || true
write_report

[ -z "${failures}" ]
