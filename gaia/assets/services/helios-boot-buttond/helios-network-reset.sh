#!/bin/sh
# Reset networking to the baked-in defaults. Intended to be invoked by the boot button daemon.
set -eu

log() {
  if command -v logger >/dev/null 2>&1; then
    logger -t helios-network-reset "$*"
  fi
  printf "%s\n" "$*"
}

log "Resetting network configuration to defaults"

# Stop any stray dhclient processes so networkd can take over cleanly.
if command -v pkill >/dev/null 2>&1; then
  pkill -f "dhclient" >/dev/null 2>&1 || true
fi

# Flush addresses on all non-loopback interfaces.
for iface in /sys/class/net/*; do
  name="$(basename "$iface")"
  [ "$name" = "lo" ] && continue
  ip addr flush dev "$name" >/dev/null 2>&1 || true
done

# Clear dhclient leases if present.
rm -f /var/lib/dhcp/dhclient*.lease* 2>/dev/null || true

# Remove persisted network overrides so networkd falls back to image defaults.
rm -f /var/lib/helios/networkd/00-helios-persisted*.network 2>/dev/null || true
rm -f /etc/systemd/network/00-helios-persisted*.network 2>/dev/null || true

# Restart networkd to reapply the packaged .network files.
systemctl restart systemd-networkd.service >/dev/null 2>&1 || true
systemctl restart systemd-networkd-wait-online.service >/dev/null 2>&1 || true

log "Network reset complete"
