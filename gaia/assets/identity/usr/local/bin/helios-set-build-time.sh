#!/bin/sh
set -eu

EPOCH_FILE="/etc/helios/build.epoch"

[ -r "$EPOCH_FILE" ] || exit 0

EPOCH="$(awk 'NR==1 {print $1}' "$EPOCH_FILE" 2>/dev/null | tr -cd '0-9' || true)"
[ -n "${EPOCH:-}" ] || exit 0

current="$(date -u +%s 2>/dev/null || printf '0')"
if [ "$current" -lt "$EPOCH" ]; then
  date -u -s "@${EPOCH}" >/dev/null 2>&1 || date -s "@${EPOCH}" >/dev/null 2>&1 || true
fi

mkdir -p /var/lib/systemd/timesync
: > /var/lib/systemd/timesync/clock

exit 0
