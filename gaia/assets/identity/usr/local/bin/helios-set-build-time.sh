#!/bin/sh
# Clock floor at boot: a board without an RTC battery starts in 1970 until NTP or the
# robot's time arrives, so the clock is set to no earlier than the image's build time.
set -eu

DATE_FILE="/etc/helios/build.date"

[ -r "$DATE_FILE" ] || exit 0

# RFC 3339 UTC (2026-11-06T17:30:00Z) as "2026-11-06 17:30:00", which BusyBox and GNU date parse.
STAMP="$(head -n 1 "$DATE_FILE" | sed -n 's/^\([0-9-]\{10\}\)T\([0-9:]\{8\}\).*$/\1 \2/p')"
[ -n "${STAMP:-}" ] || exit 0
EPOCH="$(date -u -d "$STAMP" +%s 2>/dev/null || true)"
[ -n "${EPOCH:-}" ] || exit 0

current="$(date -u +%s 2>/dev/null || printf '0')"
if [ "$current" -lt "$EPOCH" ]; then
  date -u -s "@${EPOCH}" >/dev/null 2>&1 || date -s "@${EPOCH}" >/dev/null 2>&1 || true
fi

mkdir -p /var/lib/systemd/timesync
: > /var/lib/systemd/timesync/clock

exit 0
