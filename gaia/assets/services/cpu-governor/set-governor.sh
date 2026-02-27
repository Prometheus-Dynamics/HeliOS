#!/bin/sh
# Set CPU frequency governor for all CPUs. Default from build-time config,
# but allow a runtime override persisted by the Helios app.
GOVERNOR="performance"

# System default from image configuration.
IDENTITY="/etc/default/helios-identity"
if [ -f "$IDENTITY" ]; then
    # shellcheck disable=SC1090
    . "$IDENTITY"
    if [ -n "${CPU_GOVERNOR:-}" ]; then
        GOVERNOR="$CPU_GOVERNOR"
    fi
fi

# Runtime override path (on DATA partition)
OVERRIDE="/var/lib/helios/cpu-governor"
if [ -f "$OVERRIDE" ]; then
    OVR_VAL=$(tr -d ' \t\r\n' < "$OVERRIDE")
    if [ -n "$OVR_VAL" ]; then
        GOVERNOR="$OVR_VAL"
    fi
fi

for f in /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor; do
    if [ -w "$f" ]; then
        echo "$GOVERNOR" > "$f" 2>/dev/null || true
    fi
done
