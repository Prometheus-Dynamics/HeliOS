#!/bin/sh
set -eu

status_file=/run/helios/provision-status.env
provision_status=unknown
booted_tmpfs=0

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
  if [ -e /dev/disk/by-label/BOOT ]; then
    boot_dev=$(canon /dev/disk/by-label/BOOT)
  fi
  if [ -z "$boot_dev" ]; then
    cfg_boot="$(boot_part_from_config || true)"
    if [ -n "$cfg_boot" ] && [ -e "$cfg_boot" ]; then
      boot_dev=$(canon "$cfg_boot")
    fi
  fi
  if [ -z "$boot_dev" ] && [ -e /dev/disk/by-label/DATA ]; then
    data_dev=$(canon /dev/disk/by-label/DATA)
    boot_dev=$(part_dev "$(disk_from_part "$data_dev")" 1)
  fi
  [ -b "$boot_dev" ] || return 1
  printf '%s\n' "$boot_dev"
}

ensure_boot_ota_metadata() {
  boot_dev=""
  boot_mount=/run/helios-provision/boot
  mounted=0

  data_dev=""
  if [ -e /dev/disk/by-label/DATA ]; then
    data_dev=$(canon /dev/disk/by-label/DATA)
  fi
  [ -n "$data_dev" ] || return 0
  boot_dev="$(find_boot_dev || true)"

  disk=$(disk_from_part "$data_dev")
  reserve_dev=$(part_dev "$disk" 3)
  [ -b "$reserve_dev" ] || return 0

  ota_state_dir=/var/lib/helios/ota
  install -d -m0755 "$ota_state_dir"
  if [ ! -s "$ota_state_dir/active" ]; then
    printf '%s\n' "ROOT_A" > "$ota_state_dir/active"
  fi
  if [ ! -s "$ota_state_dir/reserve" ]; then
    printf '%s\n' "ROOT_B" > "$ota_state_dir/reserve"
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
    printf '%s\n' "ROOT_A" > "$ota_dir/active"
  fi
  if [ ! -s "$ota_dir/reserve" ]; then
    printf '%s\n' "ROOT_B" > "$ota_dir/reserve"
  fi

  sync "$boot_mount" 2>/dev/null || true
  if [ "$mounted" -eq 1 ]; then
    umount "$boot_mount" || true
  fi
}

seed_data_partition_layout() {
  data_root=/run/helios-provision/data

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
    printf '%s\n' "ROOT_A" > "$data_root/ota/active"
  fi
  if [ ! -s "$data_root/ota/reserve" ]; then
    printf '%s\n' "ROOT_B" > "$data_root/ota/reserve"
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

/usr/local/bin/helios-provision --config /etc/helios/provisions.toml --status-file "$status_file"

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
