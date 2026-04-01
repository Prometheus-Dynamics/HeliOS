#!/bin/sh
set -eu

status_file=/run/helios/provision-status.env
provision_status=unknown
booted_tmpfs=0
layout_manifest=/etc/helios/storage-layout.toml
layout_env=/etc/helios/storage-layout.env

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

seed_data_partition_layout() {
  data_root=/run/helios-provision/data
  slot_a_name="${HELIOS_LAYOUT_SLOT_A_NAME:-ROOT_A}"
  slot_b_name="${HELIOS_LAYOUT_SLOT_B_NAME:-ROOT_B}"

  mountpoint -q "$data_root" || return 0

  install -d -m0755 \
    "$data_root/state" \
    "$data_root/journal" \
    "$data_root/snapshots" \
    "$data_root/ota" \
    "$data_root/diagnostics" \
    "$data_root/usb-recovery" \
    "$data_root/networkd" \
    "$data_root/plugins/daedalus" \
    "$data_root/plugins/uploads" \
    "$data_root/ai-models" \
    "$data_root/api-data" \
    "$data_root/api-data/media" \
    "$data_root/api-data/pipelines" \
    "$data_root/api-data/streams" \
    "$data_root/api-data/localization/maps"

  if [ ! -s "$data_root/ota/active" ]; then
    printf '%s\n' "$slot_a_name" > "$data_root/ota/active"
  fi
  if [ ! -s "$data_root/ota/reserve" ]; then
    printf '%s\n' "$slot_b_name" > "$data_root/ota/reserve"
  fi

  seed_file_if_missing \
    /usr/share/helios/media/FRC2026_ANDYMARK.fmap \
    "$data_root/api-data/media/FRC2026_ANDYMARK.fmap"
}

if awk '$2 == "/.overlay-data" && $3 == "tmpfs" { found=1 } END { exit found ? 0 : 1 }' /proc/mounts; then
  booted_tmpfs=1
fi

install -d -m0755 /run/helios /run/helios-provision/data /var/lib/helios
rm -f "$status_file"

/usr/local/bin/helios-provision --layout-manifest "$layout_manifest" --status-file "$status_file"

if [ -r "$status_file" ]; then
  # shellcheck source=/dev/null
  . "$status_file"
  provision_status="${PROVISION_STATUS:-unknown}"
fi

seed_data_partition_layout
ensure_boot_ota_metadata

case "$booted_tmpfs:$provision_status" in
  1:applied)
    echo "[helios-provision] booted with tmpfs writable store and changed storage layout; rebooting once immediately"
    reboot_immediately
    ;;
  1:recovered|1:unchanged)
    echo "[helios-provision] booted with tmpfs writable store but provisioning made no layout changes; not rebooting"
    ;;
esac
