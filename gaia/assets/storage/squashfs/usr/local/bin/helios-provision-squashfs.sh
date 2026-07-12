#!/bin/sh
set -eu

status_file=/run/helios/provision-status.env
provision_status=unknown
booted_tmpfs=0
layout_manifest=/etc/helios/storage-layout.toml
layout_env=/etc/helios/storage-layout.env
provision_manifest=/etc/helios/storage-layout.toml
repartition_request_present=0
repartition_update_id=
repartition_layout_copy=/run/helios-provision/repartition-layout.toml
data_check_mount=/run/helios-provision/check-data

if [ -r "$layout_env" ]; then
  # shellcheck source=/etc/helios/storage-layout.env
  . "$layout_env"
fi

canon() {
  readlink -f "$1" 2>/dev/null || printf '%s\n' "$1"
}

seed_file_if_missing() {
  src="$1"
  dst="$2"

  [ -s "$src" ] || return 0
  if [ ! -s "$dst" ]; then
    install -D -m0644 "$src" "$dst"
  fi
}

disk_from_part() {
  case "$1" in
    /dev/mmcblk*p[0-9]*|/dev/nvme*n*p[0-9]*)
      printf '%s\n' "${1%p[0-9]*}"
      ;;
    /dev/*[0-9]*)
      printf '%s\n' "${1%[0-9]*}"
      ;;
    *)
      printf '%s\n' "$1"
      ;;
  esac
}

part_dev() {
  base="$1"
  part="$2"
  case "$base" in
    *[0-9]) printf '%s\n' "${base}p${part}" ;;
    *) printf '%s\n' "${base}${part}" ;;
  esac
}

align_up_mib() {
  value="$1"
  align="${2:-4}"
  if [ "$align" -le 0 ]; then
    printf '%s\n' "$value"
    return 0
  fi
  printf '%s\n' $(( ((value + align - 1) / align) * align ))
}

read_block_size_mib() {
  dev="$(canon "$1")"
  [ -b "$dev" ] || return 1
  bn=$(basename "$dev")
  sectors=$(cat "/sys/class/block/$bn/size" 2>/dev/null || true)
  sector_bytes=$(cat "/sys/class/block/$bn/queue/logical_block_size" 2>/dev/null || printf '512\n')
  [ -n "$sectors" ] || return 1
  [ -n "$sector_bytes" ] || sector_bytes=512
  printf '%s\n' $(( (sectors * sector_bytes + 1048576 - 1) / 1048576 ))
}

boot_part_from_config() {
  [ -r /etc/helios/bootloader.conf ] || return 1
  awk -F= '$1=="boot_partition"{print $2}' /etc/helios/bootloader.conf | head -n 1
}

reboot_immediately() {
  echo "[helios-provision] forcing immediate reboot to complete first-boot storage setup"
  sync
  sleep 1

  if command -v systemctl >/dev/null 2>&1; then
    exec systemctl --force --job-mode=replace-irreversibly reboot
  fi

  if command -v reboot >/dev/null 2>&1; then
    exec reboot -f
  fi

  if [ -w /proc/sysrq-trigger ]; then
    printf '1\n' > /proc/sys/kernel/sysrq 2>/dev/null || true
    printf 'b\n' > /proc/sysrq-trigger
  fi

  exit 1
}

find_boot_dev() {
  boot_dev=""
  boot_label="${HELIOS_LAYOUT_BOOT_LABEL:-BOOT}"
  boot_part="${HELIOS_LAYOUT_BOOT_PARTITION:-1}"
  data_label="${HELIOS_LAYOUT_DATA_LABEL:-DATA}"
  if [ -n "$boot_label" ] && [ -e "/dev/disk/by-label/$boot_label" ]; then
    boot_dev=$(canon "/dev/disk/by-label/$boot_label")
  fi
  if [ -z "$boot_dev" ]; then
    cfg_boot="$(boot_part_from_config || true)"
    if [ -n "$cfg_boot" ] && [ -e "$cfg_boot" ]; then
      boot_dev=$(canon "$cfg_boot")
    fi
  fi
  if [ -z "$boot_dev" ] && [ -n "$data_label" ] && [ -e "/dev/disk/by-label/$data_label" ]; then
    data_dev=$(canon "/dev/disk/by-label/$data_label")
    boot_dev=$(part_dev "$(disk_from_part "$data_dev")" "$boot_part")
  fi
  [ -b "$boot_dev" ] || return 1
  printf '%s\n' "$boot_dev"
}

ensure_boot_ota_metadata() {
  boot_dev=""
  boot_mount=/run/helios-provision/boot
  mounted=0

  data_dev=""
  data_label="${HELIOS_LAYOUT_DATA_LABEL:-DATA}"
  slot_a_name="${HELIOS_LAYOUT_SLOT_A_NAME:-ROOT_A}"
  slot_b_name="${HELIOS_LAYOUT_SLOT_B_NAME:-ROOT_B}"
  reserve_part="${HELIOS_LAYOUT_SLOT_B_PARTITION:-3}"
  if [ -n "$data_label" ] && [ -e "/dev/disk/by-label/$data_label" ]; then
    data_dev=$(canon "/dev/disk/by-label/$data_label")
  fi
  [ -n "$data_dev" ] || return 0
  boot_dev="$(find_boot_dev || true)"

  disk=$(disk_from_part "$data_dev")
  reserve_dev=$(part_dev "$disk" "$reserve_part")
  [ -b "$reserve_dev" ] || return 0

  ota_state_dir=/var/lib/helios/ota
  install -d -m0755 "$ota_state_dir"
  if [ ! -s "$ota_state_dir/active" ]; then
    printf '%s\n' "$slot_a_name" > "$ota_state_dir/active"
  fi
  if [ ! -s "$ota_state_dir/reserve" ]; then
    printf '%s\n' "$slot_b_name" > "$ota_state_dir/reserve"
  fi

  [ -n "$boot_dev" ] || return 0

  if mountpoint -q /boot; then
    boot_mount=/boot
  else
    install -d -m0755 "$boot_mount"
  fi
  if ! mountpoint -q "$boot_mount"; then
    mount -o rw "$boot_dev" "$boot_mount" || return 0
    mounted=1
  fi

  ota_dir="$boot_mount/helios/ota"
  install -d -m0755 "$ota_dir"

  if [ ! -s "$ota_dir/active" ]; then
    printf '%s\n' "$slot_a_name" > "$ota_dir/active"
  fi
  if [ ! -s "$ota_dir/reserve" ]; then
    printf '%s\n' "$slot_b_name" > "$ota_dir/reserve"
  fi

  sync "$boot_mount" 2>/dev/null || true
  if [ "$mounted" -eq 1 ]; then
    umount "$boot_mount" || true
  fi
}

load_repartition_request() {
  boot_dev=
  boot_mount=/run/helios-provision/boot
  mounted=0
  request_env=
  layout_src=

  [ "$booted_tmpfs" -eq 1 ] || return 0

  boot_dev="$(find_boot_dev || true)"
  [ -n "$boot_dev" ] || return 0

  if mountpoint -q /boot; then
    boot_mount=/boot
  else
    install -d -m0755 "$boot_mount"
  fi
  if ! mountpoint -q "$boot_mount"; then
    mount -o rw "$boot_dev" "$boot_mount" || return 0
    mounted=1
  fi

  request_env="$boot_mount/helios/ota/repartition-request.env"
  layout_src="$boot_mount/helios/ota/repartition-layout.toml"
  if [ -r "$request_env" ] && [ -r "$layout_src" ]; then
    # shellcheck source=/dev/null
    . "$request_env"
    install -D -m0644 "$layout_src" "$repartition_layout_copy"
    repartition_request_present=1
    repartition_update_id="${HELIOS_REPARTITION_UPDATE_ID:-}"
    provision_manifest="$repartition_layout_copy"
  fi

  if [ "$mounted" -eq 1 ]; then
    umount "$boot_mount" || true
  fi
}

write_repartition_result() {
  status="$1"
  boot_dev=
  boot_mount=/run/helios-provision/boot
  mounted=0
  ota_dir=

  [ "$repartition_request_present" -eq 1 ] || return 0

  boot_dev="$(find_boot_dev || true)"
  [ -n "$boot_dev" ] || return 0

  if mountpoint -q /boot; then
    boot_mount=/boot
  else
    install -d -m0755 "$boot_mount"
  fi
  if ! mountpoint -q "$boot_mount"; then
    mount -o rw "$boot_dev" "$boot_mount" || return 0
    mounted=1
  fi

  ota_dir="$boot_mount/helios/ota"
  install -d -m0755 "$ota_dir"
  cat > "$ota_dir/repartition-result.env" <<EOF
HELIOS_REPARTITION_RESULT_VERSION=1
HELIOS_REPARTITION_UPDATE_ID=${repartition_update_id}
HELIOS_REPARTITION_STATUS=${status}
EOF
  rm -f \
    "$ota_dir/repartition-request.env" \
    "$ota_dir/repartition-layout.toml"

  sync "$boot_mount" 2>/dev/null || true
  if [ "$mounted" -eq 1 ]; then
    umount "$boot_mount" || true
  fi
}

seed_data_partition_layout() {
  data_root=/run/helios-provision/data
  slot_a_name="${HELIOS_LAYOUT_SLOT_A_NAME:-ROOT_A}"
  slot_b_name="${HELIOS_LAYOUT_SLOT_B_NAME:-ROOT_B}"

  if ! mountpoint -q "$data_root"; then
    if mountpoint -q /var/lib/helios; then
      data_root=/var/lib/helios
    else
      return 0
    fi
  fi

  install -d -m0755 \
    "$data_root/bin" \
    "$data_root/state" \
    "$data_root/journal" \
    "$data_root/identity/ssh" \
    "$data_root/ota" \
    "$data_root/diagnostics" \
    "$data_root/updater" \
    "$data_root/root-overlay/root-a/upper" \
    "$data_root/root-overlay/root-a/work" \
    "$data_root/root-overlay/root-b/upper" \
    "$data_root/root-overlay/root-b/work"

  for bin in orion-node orionctl helios-engine helios-peripherals helios-api helios-updater heliosctl; do
    if [ -x "/usr/bin/$bin" ] && [ ! -e "$data_root/bin/$bin" ]; then
      ln -s "/usr/bin/$bin" "$data_root/bin/$bin"
    fi
  done

  if [ ! -s "$data_root/ota/active" ]; then
    printf '%s\n' "$slot_a_name" > "$data_root/ota/active"
  fi
  if [ ! -s "$data_root/ota/reserve" ]; then
    printf '%s\n' "$slot_b_name" > "$data_root/ota/reserve"
  fi
}

data_partition_ready_for_persistent_boot() {
  data_dev=
  data_label="${HELIOS_LAYOUT_DATA_LABEL:-DATA}"
  secondary_marker="${HELIOS_LAYOUT_DATA_SECONDARY_MARKER:-/run/helios-provision/data/.provision.data_v1}"
  marker_relative="${secondary_marker#/run/helios-provision/data}"

  [ -n "$marker_relative" ] || marker_relative='/.provision.data_v1'
  case "$marker_relative" in
    /*) ;;
    *) marker_relative="/$marker_relative" ;;
  esac

  if [ -n "$data_label" ] && [ -e "/dev/disk/by-label/$data_label" ]; then
    data_dev=$(canon "/dev/disk/by-label/$data_label")
  fi
  [ -b "$data_dev" ] || return 1

  install -d -m0755 "$data_check_mount"
  mountpoint -q "$data_check_mount" && umount "$data_check_mount" 2>/dev/null || true

  if ! mount -o ro "$data_dev" "$data_check_mount" 2>/dev/null; then
    return 1
  fi

  if [ -f "$data_check_mount$marker_relative" ]; then
    umount "$data_check_mount" 2>/dev/null || true
    return 0
  fi

  umount "$data_check_mount" 2>/dev/null || true
  return 1
}

if awk '$2 == "/.overlay-data" && $3 == "tmpfs" { found=1 } END { exit found ? 0 : 1 }' /proc/mounts; then
  booted_tmpfs=1
fi

install -d -m0755 /run/helios /run/helios-provision/data /var/lib/helios
rm -f "$status_file"
load_repartition_request

if [ "$repartition_request_present" -eq 1 ]; then
  /usr/local/bin/helios-provision --layout-manifest "$provision_manifest" --status-file "$status_file" --force-apply
else
  /usr/local/bin/helios-provision --layout-manifest "$provision_manifest" --status-file "$status_file"
fi

if [ -r "$status_file" ]; then
  # shellcheck source=/dev/null
  . "$status_file"
  provision_status="${PROVISION_STATUS:-unknown}"
fi

write_repartition_result "$provision_status"

seed_data_partition_layout
ensure_boot_ota_metadata

if [ "$booted_tmpfs" -eq 1 ]; then
  if data_partition_ready_for_persistent_boot; then
    echo "[helios-provision] booted with tmpfs writable store but persistent DATA is now ready; rebooting once immediately"
    reboot_immediately
  fi

  case "$provision_status" in
    applied)
      echo "[helios-provision] booted with tmpfs writable store and changed storage layout; DATA still not ready after provisioning"
      ;;
    recovered|unchanged)
      echo "[helios-provision] booted with tmpfs writable store and persistent DATA is still unavailable; not rebooting"
      ;;
  esac
fi
