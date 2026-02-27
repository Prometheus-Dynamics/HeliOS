#!/bin/sh
set -eu

# Determine current root device
canon() { readlink -f "$1" 2>/dev/null || echo "$1"; }
set_label() {
  dev="$1"
  label="$2"
  tune2fs -L "$label" "$dev" >/dev/null 2>&1 && return 0
  e2label "$dev" "$label" >/dev/null 2>&1 && return 0
  return 1
}
rootdev=""
root_mm=$(awk '$5=="/"{print $3}' /proc/self/mountinfo | head -n 1)
if [ -n "$root_mm" ]; then
  for devpath in /sys/class/block/*/dev; do
    [ -e "$devpath" ] || continue
    if [ "$(cat "$devpath" 2>/dev/null)" = "$root_mm" ]; then
      rootdev="/dev/$(basename "$(dirname "$devpath")")"
      break
    fi
  done
fi
if [ -z "$rootdev" ]; then
  rootdev=$(awk '$2=="/"{print $1}' /proc/mounts)
fi
rootcanon=$(canon "$rootdev")

# Find label of current root by matching /dev/disk/by-label symlinks
rootlabel=""
for l in /dev/disk/by-label/*; do
  [ -e "$l" ] || continue
  tgt=$(canon "$l")
  if [ "$tgt" = "$rootcanon" ]; then
    base=$(basename "$l")
    case "$base" in
      ACTIVE|RESERVE) rootlabel="$base" ;;
    esac
  fi
done

root_puuid=""
cmdline_root=$(awk 'BEGIN{RS=" "}/^root=/{print substr($0,6)}' /proc/cmdline | head -n 1)
case "$cmdline_root" in
  PARTUUID=*) root_puuid="${cmdline_root#PARTUUID=}" ;;
esac
if [ -z "$root_puuid" ] && [ -n "$rootdev" ]; then
  sys="/sys/class/block/$(basename "$rootdev")/uevent"
  root_puuid=$(awk -F= '$1=="PARTUUID"{print $2}' "$sys" 2>/dev/null || true)
fi

dev_partuuid() {
  dev="$1"
  sys="/sys/class/block/$dev/uevent"
  awk -F= '$1=="PARTUUID"{print $2}' "$sys" 2>/dev/null || true
}

slot_a_dev="/dev/mmcblk0p2"
slot_b_dev="/dev/mmcblk0p3"
slot_a_puuid=$(dev_partuuid "mmcblk0p2")
slot_b_puuid=$(dev_partuuid "mmcblk0p3")

# Mount BOOT to check pending marker
mkdir -p /mnt/boot
if mountpoint -q /mnt/boot; then
  :
else
  if [ -e /dev/disk/by-label/BOOT ]; then
    mount -o rw /dev/disk/by-label/BOOT /mnt/boot || true
  fi
fi

if [ -f /mnt/boot/helios/ota/pending ]; then
  target=$(cat /mnt/boot/helios/ota/pending || true)
  match=0
  active_dev=""
  reserve_dev=""
  case "$target" in
    PARTUUID=*)
      target_puuid="${target#PARTUUID=}"
      if [ -n "$root_puuid" ] && [ "$target_puuid" = "$root_puuid" ]; then
        match=1
        if [ "$rootdev" = "$slot_a_dev" ] || [ "$root_puuid" = "$slot_a_puuid" ]; then
          active_dev="$slot_a_dev"
          reserve_dev="$slot_b_dev"
        elif [ "$rootdev" = "$slot_b_dev" ] || [ "$root_puuid" = "$slot_b_puuid" ]; then
          active_dev="$slot_b_dev"
          reserve_dev="$slot_a_dev"
        fi
      fi
      ;;
    *)
      if [ "$target" = "$rootlabel" ] && [ -n "$rootlabel" ]; then
        match=1
      fi
      ;;
  esac

  if [ "$match" -eq 1 ]; then
    # Success: swap labels so we always update RESERVE next time.
    if [ -z "$active_dev" ] || [ -z "$reserve_dev" ]; then
      active_dev=$(canon /dev/disk/by-label/ACTIVE 2>/dev/null || true)
      reserve_dev=$(canon /dev/disk/by-label/RESERVE 2>/dev/null || true)
      if [ -z "$reserve_dev" ]; then
        if [ "$active_dev" = "$slot_a_dev" ]; then
          reserve_dev="$slot_b_dev"
        elif [ "$active_dev" = "$slot_b_dev" ]; then
          reserve_dev="$slot_a_dev"
        fi
      fi
    fi

    if [ -n "$active_dev" ] && [ -n "$reserve_dev" ]; then
      # Avoid label collision by using a temporary label during the swap.
      # The current root (active_dev) should become ACTIVE; the other slot should become RESERVE.
      set_label "$reserve_dev" "HELIOS-TMP" || true
      set_label "$active_dev" "ACTIVE" || true
      set_label "$reserve_dev" "RESERVE" || true
      if command -v udevadm >/dev/null 2>&1; then
        udevadm trigger --subsystem-match=block >/dev/null 2>&1 || true
        udevadm settle >/dev/null 2>&1 || true
      fi
    fi

    # Mark active and clear pending
    printf "%s\n" "ACTIVE" > /mnt/boot/helios/ota/active
    printf "%s\n" "RESERVE" > /mnt/boot/helios/ota/reserve
    rm -f /mnt/boot/helios/ota/pending
    rm -f /var/lib/helios/ota/pending /root/helios-updater/ota/pending
  fi
fi

if mountpoint -q /mnt/boot; then
  umount /mnt/boot || true
fi
exit 0
