#!/bin/sh
set -eu

# Soft-reset the USB gadget without reboot: emulate a cable unplug/replug by
# unbinding and rebinding the UDC. This is the fastest recovery path when the
# gadget stops communicating but the system is otherwise alive.

GADGET_ENV=/etc/helios/gadget.env
WATCHDOG_ENV=/etc/helios/gadget-watchdog.env

if [ -f "$GADGET_ENV" ]; then
  # shellcheck disable=SC1090
  . "$GADGET_ENV"
fi
if [ -f "$WATCHDOG_ENV" ]; then
  # shellcheck disable=SC1090
  . "$WATCHDOG_ENV"
fi

GADGET_NAME=${GADGET_NAME:-g1}
LOGF=${GADGET_RESET_LOGFILE:-/var/log/usb-gadget-watchdog.log}
DISCONNECT_DELAY_SEC=${GADGET_RESET_DISCONNECT_DELAY_SEC:-1}

mkdir -p /var/log 2>/dev/null || true
touch "$LOGF" 2>/dev/null || true

log() {
  msg="[usb-gadget-reset] $*"
  echo "$msg"
  (echo "$msg" >> "$LOGF") 2>/dev/null || true
  command -v logger >/dev/null 2>&1 && logger -t usb-gadget-reset "$msg" 2>/dev/null || true
}

mountpoint -q /sys/kernel/config 2>/dev/null || mount -t configfs configfs /sys/kernel/config 2>/dev/null || true

role_nudge() {
  # Best-effort role switch towards device to re-trigger peripheral connect.
  for role in /sys/bus/platform/devices/*usb*/role /sys/class/usb_role/*/role; do
    [ -w "$role" ] || continue
    echo device > "$role" 2>/dev/null || true
  done
}

reset_platform_driver() {
  udc="$1"
  [ -n "$udc" ] || return 1

  # Map UDC -> underlying device, then unbind/bind its driver if possible.
  devpath="/sys/class/udc/$udc/device"
  [ -e "$devpath" ] || return 1
  # Resolve to a concrete sysfs path
  real="$(readlink -f "$devpath" 2>/dev/null || true)"
  [ -n "$real" ] || return 1
  bn="$(basename "$real")"

  drvlink="$real/driver"
  if [ ! -L "$drvlink" ]; then
    log "platform reset: no driver symlink for udc=$udc (dev=$bn)"
    return 1
  fi
  drvreal="$(readlink -f "$drvlink" 2>/dev/null || true)"
  [ -n "$drvreal" ] || return 1
  drvname="$(basename "$drvreal")"

  unbind="$drvreal/unbind"
  bind="$drvreal/bind"
  if [ -w "$unbind" ] && [ -w "$bind" ]; then
    log "platform reset: unbind/bind driver=$drvname dev=$bn"
    echo "$bn" > "$unbind" 2>/dev/null || true
    sleep 0.4 2>/dev/null || true
    echo "$bn" > "$bind" 2>/dev/null || true
    sleep 0.6 2>/dev/null || true
    role_nudge
    return 0
  fi

  log "platform reset: driver $drvname does not support bind/unbind (or perms)"
  return 1
}

G="/sys/kernel/config/usb_gadget/$GADGET_NAME"
if [ ! -d "$G" ]; then
  log "no gadget at $G; attempting full reconfigure via usb-gadget-setup.sh"
  if [ -x /usr/local/bin/usb-gadget-setup.sh ]; then
    /bin/sh /usr/local/bin/usb-gadget-setup.sh || true
  fi
fi

if [ ! -d "$G" ] || [ ! -f "$G/UDC" ]; then
  log "cannot reset: missing $G/UDC"
  exit 1
fi

UDC="$(cat "$G/UDC" 2>/dev/null || true)"
if [ -z "$UDC" ]; then
  UDC="$(ls /sys/class/udc 2>/dev/null | head -n1 || true)"
fi
if [ -z "$UDC" ]; then
  log "cannot reset: no UDC found in /sys/class/udc"
  exit 1
fi

log "reset: unbinding gadget '$GADGET_NAME' from UDC '$UDC'"
echo "" > "$G/UDC" 2>/dev/null || true
sleep "$DISCONNECT_DELAY_SEC" 2>/dev/null || true
role_nudge

log "reset: rebinding gadget '$GADGET_NAME' to UDC '$UDC'"
if ! echo "$UDC" > "$G/UDC" 2>/dev/null; then
  log "reset: initial rebind failed; attempting platform driver reset"
  reset_platform_driver "$UDC" || true
  mountpoint -q /sys/kernel/config 2>/dev/null || mount -t configfs configfs /sys/kernel/config 2>/dev/null || true
  # Recreate gadget to ensure configfs state is consistent after controller reset.
  if [ -x /usr/local/bin/usb-gadget-setup.sh ]; then
    /bin/sh /usr/local/bin/usb-gadget-setup.sh || true
  fi
  # Final attempt
  echo "$UDC" > "$G/UDC" 2>/dev/null || true
fi

# Best-effort: bring gadget-facing interfaces up; do not fail reset if ip is missing.
for ifc in usbbr0 usb0 usb1; do
  ip link set dev "$ifc" up 2>/dev/null || true
done

# Best-effort: nudge dnsmasq in case it got stuck due to the link flap.
if command -v systemctl >/dev/null 2>&1; then
  systemctl restart helios-dnsmasq.service >/dev/null 2>&1 || true
fi

log "reset: complete"
