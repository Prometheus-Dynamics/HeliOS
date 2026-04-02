#!/bin/sh
set -eu

if [ -r /etc/helios/storage-layout.env ]; then
  # shellcheck source=/etc/helios/storage-layout.env
  . /etc/helios/storage-layout.env
fi

BOOT_LABEL="${HELIOS_LAYOUT_BOOT_LABEL:-BOOT}"
BOOT_PARTITION="${HELIOS_LAYOUT_BOOT_PARTITION:-1}"
DATA_LABEL="${HELIOS_LAYOUT_DATA_LABEL:-DATA}"
SLOT_A_NAME="${HELIOS_LAYOUT_SLOT_A_NAME:-ACTIVE}"
SLOT_A_LABEL="${HELIOS_LAYOUT_SLOT_A_LABEL:-ACTIVE}"
SLOT_A_PARTITION="${HELIOS_LAYOUT_SLOT_A_PARTITION:-2}"
SLOT_B_NAME="${HELIOS_LAYOUT_SLOT_B_NAME:-RESERVE}"
SLOT_B_LABEL="${HELIOS_LAYOUT_SLOT_B_LABEL:-RESERVE}"
SLOT_B_PARTITION="${HELIOS_LAYOUT_SLOT_B_PARTITION:-3}"
SLOT_SCHEME="${HELIOS_LAYOUT_SLOT_SCHEME:-ext4_labels}"

canon() { readlink -f "$1" 2>/dev/null || echo "$1"; }

set_label() {
  dev="$1"
  label="$2"
  tune2fs -L "$label" "$dev" >/dev/null 2>&1 && return 0
  e2label "$dev" "$label" >/dev/null 2>&1 && return 0
  return 1
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

find_boot_dev() {
  boot_dev=""
  if [ -n "$BOOT_LABEL" ] && [ -e "/dev/disk/by-label/$BOOT_LABEL" ]; then
    boot_dev=$(canon "/dev/disk/by-label/$BOOT_LABEL")
  fi
  if [ -z "$boot_dev" ]; then
    cfg_boot="$(boot_part_from_config || true)"
    if [ -n "$cfg_boot" ] && [ -e "$cfg_boot" ]; then
      boot_dev=$(canon "$cfg_boot")
    fi
  fi
  if [ -z "$boot_dev" ] && [ -n "$DATA_LABEL" ] && [ -e "/dev/disk/by-label/$DATA_LABEL" ]; then
    data_dev=$(canon "/dev/disk/by-label/$DATA_LABEL")
    boot_dev=$(part_dev "$(disk_from_part "$data_dev")" "$BOOT_PARTITION")
  fi
  if [ -z "$boot_dev" ] && [ -n "$rootdev" ]; then
    boot_dev=$(part_dev "$(disk_from_part "$rootdev")" "$BOOT_PARTITION")
  fi
  [ -b "$boot_dev" ] || return 1
  printf '%s\n' "$boot_dev"
}

dev_partuuid() {
  dev="$1"
  sys="/sys/class/block/$(basename "$dev")/uevent"
  awk -F= '$1=="PARTUUID"{print $2}' "$sys" 2>/dev/null || true
}

slot_from_value() {
  value="$1"
  slot_a_name="$2"
  slot_a_dev="$3"
  slot_b_name="$4"
  slot_b_dev="$5"

  case "$value" in
    "$slot_a_name"|"$slot_b_name")
      printf '%s\n' "$value"
      return 0
      ;;
    PARTUUID=*)
      target_puuid="${value#PARTUUID=}"
      [ -n "$target_puuid" ] || return 1
      if [ "$target_puuid" = "$(dev_partuuid "$slot_a_dev")" ]; then
        printf '%s\n' "$slot_a_name"
        return 0
      fi
      if [ "$target_puuid" = "$(dev_partuuid "$slot_b_dev")" ]; then
        printf '%s\n' "$slot_b_name"
        return 0
      fi
      ;;
    /dev/*)
      target_dev=$(canon "$value")
      if [ "$target_dev" = "$(canon "$slot_a_dev")" ]; then
        printf '%s\n' "$slot_a_name"
        return 0
      fi
      if [ "$target_dev" = "$(canon "$slot_b_dev")" ]; then
        printf '%s\n' "$slot_b_name"
        return 0
      fi
      ;;
  esac

  return 1
}

find_rootdev() {
  root_mm=$(awk '$5=="/"{print $3}' /proc/self/mountinfo | head -n 1)
  if [ -n "$root_mm" ]; then
    for devpath in /sys/class/block/*/dev; do
      [ -e "$devpath" ] || continue
      if [ "$(cat "$devpath" 2>/dev/null)" = "$root_mm" ]; then
        printf '/dev/%s\n' "$(basename "$(dirname "$devpath")")"
        return 0
      fi
    done
  fi
  awk '$2=="/"{print $1}' /proc/mounts | head -n 1
}

rootdev="$(find_rootdev)"
rootcanon="$(canon "$rootdev")"
cmdline_root="$(awk 'BEGIN{RS=" "}/^root=/{print substr($0,6)}' /proc/cmdline | head -n 1)"

rootlabel=""
for l in /dev/disk/by-label/*; do
  [ -e "$l" ] || continue
  tgt=$(canon "$l")
  if [ "$tgt" = "$rootcanon" ]; then
    base=$(basename "$l")
    if [ "$base" = "$SLOT_A_LABEL" ] || [ "$base" = "$SLOT_B_LABEL" ]; then
      rootlabel="$base"
    fi
  fi
done

boot_mount="/mnt/boot"
mounted_boot=0
if mountpoint -q /boot; then
  boot_mount="/boot"
else
  mkdir -p "$boot_mount"
  if ! mountpoint -q "$boot_mount"; then
    if boot_dev=$(find_boot_dev 2>/dev/null); then
      mount -o rw "$boot_dev" "$boot_mount" || true
      if mountpoint -q "$boot_mount"; then
        mounted_boot=1
      fi
    fi
  fi
fi

ota_dir="${boot_mount}/helios/ota"
ota_state_dir="/var/lib/helios/ota"
mkdir -p "$ota_state_dir"
pending=""
if [ -f "$ota_dir/pending" ]; then
  pending=$(cat "$ota_dir/pending" || true)
fi

if [ -n "$pending" ]; then
  if [ -n "$SLOT_A_LABEL" ] && [ -n "$SLOT_B_LABEL" ] && { [ -e "/dev/disk/by-label/$SLOT_A_LABEL" ] || [ -e "/dev/disk/by-label/$SLOT_B_LABEL" ]; }; then
    slot_a_dev="$(canon "/dev/disk/by-label/$SLOT_A_LABEL" 2>/dev/null || true)"
    slot_b_dev="$(canon "/dev/disk/by-label/$SLOT_B_LABEL" 2>/dev/null || true)"
    current_slot=""
    if [ -n "$rootlabel" ]; then
      current_slot="$rootlabel"
    elif current_slot=$(slot_from_value "$cmdline_root" "$SLOT_A_LABEL" "$slot_a_dev" "$SLOT_B_LABEL" "$slot_b_dev" 2>/dev/null); then
      :
    fi
    target_slot=""
    if target_slot=$(slot_from_value "$pending" "$SLOT_A_LABEL" "$slot_a_dev" "$SLOT_B_LABEL" "$slot_b_dev" 2>/dev/null); then
      :
    fi

    if [ -n "$current_slot" ] && [ "$current_slot" = "$target_slot" ]; then
      active_dev=""
      reserve_dev=""
      if [ "$current_slot" = "$SLOT_A_LABEL" ]; then
        active_dev="$slot_a_dev"
        reserve_dev="$slot_b_dev"
      elif [ "$current_slot" = "$SLOT_B_LABEL" ]; then
        active_dev="$slot_b_dev"
        reserve_dev="$slot_a_dev"
      fi

      if [ -n "$active_dev" ] && [ -n "$reserve_dev" ]; then
        set_label "$reserve_dev" "HELIOS-TMP" || true
        set_label "$active_dev" "$SLOT_A_LABEL" || true
        set_label "$reserve_dev" "$SLOT_B_LABEL" || true
        if command -v udevadm >/dev/null 2>&1; then
          udevadm trigger --subsystem-match=block >/dev/null 2>&1 || true
          udevadm settle >/dev/null 2>&1 || true
        fi
      fi

      printf '%s\n' "$SLOT_A_NAME" > "$ota_dir/active"
      printf '%s\n' "$SLOT_B_NAME" > "$ota_dir/reserve"
      printf '%s\n' "$SLOT_A_NAME" > "$ota_state_dir/active"
      printf '%s\n' "$SLOT_B_NAME" > "$ota_state_dir/reserve"
      rm -f "$ota_dir/pending"
      rm -f "$ota_state_dir/pending" /root/helios-updater/ota/pending
    fi
  else
    data_dev=""
    if [ -n "$DATA_LABEL" ] && [ -e "/dev/disk/by-label/$DATA_LABEL" ]; then
      data_dev="$(canon "/dev/disk/by-label/$DATA_LABEL")"
    fi
    if [ -n "$data_dev" ]; then
      disk="$(disk_from_part "$data_dev")"
      slot_a_dev="$(part_dev "$disk" "$SLOT_A_PARTITION")"
      slot_b_dev="$(part_dev "$disk" "$SLOT_B_PARTITION")"
      current_slot=""
      if current_slot=$(slot_from_value "$cmdline_root" "$SLOT_A_NAME" "$slot_a_dev" "$SLOT_B_NAME" "$slot_b_dev" 2>/dev/null); then
        :
      elif [ -f "$ota_dir/active" ]; then
        current_slot=$(cat "$ota_dir/active" || true)
      fi
      target_slot=""
      if target_slot=$(slot_from_value "$pending" "$SLOT_A_NAME" "$slot_a_dev" "$SLOT_B_NAME" "$slot_b_dev" 2>/dev/null); then
        :
      fi

      if [ -n "$current_slot" ] && [ "$current_slot" = "$target_slot" ]; then
        if [ "$current_slot" = "$SLOT_A_NAME" ]; then
          reserve_slot="$SLOT_B_NAME"
        else
          reserve_slot="$SLOT_A_NAME"
        fi
        printf '%s\n' "$current_slot" > "$ota_dir/active"
        printf '%s\n' "$reserve_slot" > "$ota_dir/reserve"
        printf '%s\n' "$current_slot" > "$ota_state_dir/active"
        printf '%s\n' "$reserve_slot" > "$ota_state_dir/reserve"
        rm -f "$ota_dir/pending"
        rm -f "$ota_state_dir/pending" /root/helios-updater/ota/pending
      fi
    fi
  fi
fi

if [ "$mounted_boot" -eq 1 ] && mountpoint -q "$boot_mount"; then
  umount "$boot_mount" || true
fi
exit 0
