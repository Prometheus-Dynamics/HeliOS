#!/bin/sh
set -eu

# USB gadget watchdog: if the gadget is enumerated but IP connectivity to the
# attached host dies, force a soft disconnect/reconnect of the UDC.
#
# This mitigates dwc2/configfs-gadget hangs that otherwise require a physical
# cable replug.

GADGET_ENV=/etc/helios/gadget.env
CONF=/etc/helios/gadget-watchdog.env

if [ -f "$GADGET_ENV" ]; then
  # shellcheck disable=SC1090
  . "$GADGET_ENV"
fi
if [ -f "$CONF" ]; then
  # shellcheck disable=SC1090
  . "$CONF"
fi

GADGET_NAME=${GADGET_NAME:-g1}
LOGF=${GADGET_WATCHDOG_LOGFILE:-/var/log/usb-gadget-watchdog.log}
CHECK_INTERVAL_SEC=${GADGET_WATCHDOG_CHECK_INTERVAL_SEC:-2}
FAIL_THRESHOLD=${GADGET_WATCHDOG_FAIL_THRESHOLD:-3}
MIN_RESET_INTERVAL_SEC=${GADGET_WATCHDOG_MIN_RESET_INTERVAL_SEC:-30}
PING_IFACE=${GADGET_WATCHDOG_PING_IFACE:-usbbr0}
PING_TIMEOUT_SEC=${GADGET_WATCHDOG_PING_TIMEOUT_SEC:-1}
PING_TARGET=${GADGET_WATCHDOG_PING_TARGET:-AUTO}

mkdir -p /var/log 2>/dev/null || true
touch "$LOGF" 2>/dev/null || true

log() {
  msg="[usb-gadget-watchdog] $*"
  echo "$msg"
  (echo "$msg" >> "$LOGF") 2>/dev/null || true
  command -v logger >/dev/null 2>&1 && logger -t usb-gadget-watchdog "$msg" 2>/dev/null || true
}

gadget_udc() {
  G="/sys/kernel/config/usb_gadget/$GADGET_NAME"
  [ -f "$G/UDC" ] || return 1
  cat "$G/UDC" 2>/dev/null || true
}

udc_state() {
  udc="$1"
  [ -n "$udc" ] || return 0
  f="/sys/class/udc/$udc/state"
  [ -r "$f" ] || return 0
  cat "$f" 2>/dev/null || true
}

leases_ip() {
  # Prefer dnsmasq leases if present; fall back to ARP neighbors on the bridge.
  for f in \
    /var/lib/helios/state/dnsmasq.leases \
    /var/lib/misc/dnsmasq.leases \
    /var/lib/dnsmasq/dnsmasq.leases \
    /var/lib/dnsmasq.leases \
    /tmp/dnsmasq.leases; do
    if [ -r "$f" ]; then
      ip="$(awk 'NF>=3 {last=$3} END{print last}' "$f" 2>/dev/null || true)"
      if [ -n "$ip" ]; then
        echo "$ip"
        return 0
      fi
    fi
  done

  if command -v ip >/dev/null 2>&1 && [ -d "/sys/class/net/$PING_IFACE" ]; then
    # Pick the first non-failed neighbor entry.
    ip neigh show dev "$PING_IFACE" 2>/dev/null | awk '
      $1 ~ /^[0-9]+\\./ && $0 !~ /FAILED/ {print $1; exit}
    ' || true
  fi
  return 0
}

neighbor_bad() {
  target="$1"
  [ -n "$target" ] || return 1
  command -v ip >/dev/null 2>&1 || return 1
  # INCOMPLETE/FAILED is the symptom we saw on the host side when the link wedges.
  ip neigh show dev "$PING_IFACE" 2>/dev/null | awk -v t="$target" '
    $1==t && ($0 ~ /INCOMPLETE/ || $0 ~ /FAILED/) { exit 0 }
    $1==t { exit 1 }
    END { exit 1 }
  '
}

ping_ok() {
  target="$1"
  [ -n "$target" ] || return 1
  # If ping isn't available, skip health enforcement entirely.
  command -v ping >/dev/null 2>&1 || return 2
  [ -d "/sys/class/net/$PING_IFACE" ] || return 2
  ping -I "$PING_IFACE" -c 1 -W "$PING_TIMEOUT_SEC" "$target" >/dev/null 2>&1
}

dump_debug() {
  # Best-effort snapshots right before a reset; useful for post-mortem.
  udc="$1"
  st="$2"
  log "debug: udc='$udc' state='${st:-unknown}' iface='$PING_IFACE' target='${PING_TARGET:-AUTO}'"
  if [ -d "/sys/class/net/$PING_IFACE" ]; then
    rx="$(cat "/sys/class/net/$PING_IFACE/statistics/rx_packets" 2>/dev/null || echo n/a)"
    tx="$(cat "/sys/class/net/$PING_IFACE/statistics/tx_packets" 2>/dev/null || echo n/a)"
    log "debug: $PING_IFACE rx_packets=$rx tx_packets=$tx"
  fi
  dmesg | tail -n 60 2>/dev/null | sed 's/^/[usb-gadget-watchdog][dmesg] /' >>"$LOGF" 2>/dev/null || true
}

reset_gadget() {
  # Rate-limit resets to avoid oscillation.
  now="$(date +%s 2>/dev/null || echo 0)"
  if [ -f /run/usb-gadget-watchdog.last_reset ]; then
    last="$(cat /run/usb-gadget-watchdog.last_reset 2>/dev/null || echo 0)"
  else
    last=0
  fi
  if [ "$now" -ne 0 ] && [ "$last" -ne 0 ] && [ $((now - last)) -lt "$MIN_RESET_INTERVAL_SEC" ]; then
    log "reset: suppressed by rate limit (${now}-${last} < ${MIN_RESET_INTERVAL_SEC}s)"
    return 0
  fi
  mkdir -p /run 2>/dev/null || true
  echo "$now" > /run/usb-gadget-watchdog.last_reset 2>/dev/null || true

  if [ -x /usr/local/bin/usb-gadget-reset.sh ]; then
    /bin/sh /usr/local/bin/usb-gadget-reset.sh || true
  else
    log "reset: missing /usr/local/bin/usb-gadget-reset.sh"
  fi
}

log "start: gadget=$GADGET_NAME iface=$PING_IFACE fail_threshold=$FAIL_THRESHOLD check_interval=${CHECK_INTERVAL_SEC}s"

fails=0

while :; do
  udc="$(gadget_udc || true)"
  st="$(udc_state "$udc")"

  # Only enforce connectivity when the gadget is enumerated/configured.
  case "${st:-}" in
    configured|addressed|default)
      : ;;
    *)
      fails=0
      sleep "$CHECK_INTERVAL_SEC"
      continue ;;
  esac

  target="$PING_TARGET"
  if [ "$target" = "AUTO" ] || [ -z "$target" ]; then
    target="$(leases_ip || true)"
  fi

  # If the bridge/interface isn't physically up, don't spam resets.
  if [ -r "/sys/class/net/$PING_IFACE/carrier" ]; then
    c="$(cat "/sys/class/net/$PING_IFACE/carrier" 2>/dev/null || echo 0)"
    if [ "$c" != "1" ]; then
      fails=0
      sleep "$CHECK_INTERVAL_SEC"
      continue
    fi
  fi

  if [ -z "$target" ]; then
    fails=0
    sleep "$CHECK_INTERVAL_SEC"
    continue
  fi

  # Treat bad neighbor resolution as a strong failure signal even if ICMP is filtered.
  if neighbor_bad "$target"; then
    fails=$((fails + 1))
    log "health: neigh unresolved (${fails}/${FAIL_THRESHOLD}) target=$target state=${st:-unknown}"
    if [ "$fails" -ge "$FAIL_THRESHOLD" ]; then
      dump_debug "$udc" "$st"
      log "health: triggering gadget reset"
      reset_gadget
      fails=0
    fi
    sleep "$CHECK_INTERVAL_SEC"
    continue
  fi

  if ping_ok "$target"; then
    fails=0
  else
    rc=$?
    if [ "$rc" -eq 2 ]; then
      fails=0
      sleep "$CHECK_INTERVAL_SEC"
      continue
    fi
    fails=$((fails + 1))
    log "health: ping failed (${fails}/${FAIL_THRESHOLD}) target=$target state=${st:-unknown}"
    if [ "$fails" -ge "$FAIL_THRESHOLD" ]; then
      dump_debug "$udc" "$st"
      log "health: triggering gadget reset"
      reset_gadget
      fails=0
    fi
  fi

  sleep "$CHECK_INTERVAL_SEC"
done
