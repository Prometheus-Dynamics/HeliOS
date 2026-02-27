#!/bin/sh
set -eu

EPOCH_FILE="/etc/helios/build.epoch"

[ -r "$EPOCH_FILE" ] || exit 0

# Read first token as epoch seconds and set unconditionally
EPOCH=$(awk 'NR==1 {print $1}' "$EPOCH_FILE" 2>/dev/null | tr -cd '0-9' || true)
[ -n "${EPOCH:-}" ] || exit 0

# Use epoch form to avoid format differences across date implementations
date -u -s "@${EPOCH}" >/dev/null 2>&1 || date -s "@${EPOCH}" >/dev/null 2>&1 || true

# Seed timesyncd clock file so any later logic will not regress the clock
mkdir -p /var/lib/systemd/timesync
: > /var/lib/systemd/timesync/clock

exit 0
