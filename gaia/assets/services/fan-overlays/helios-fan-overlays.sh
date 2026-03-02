#!/bin/sh
set -eu
PATH="/usr/sbin:/usr/bin:/sbin:/bin"

OVERLAY_ROOT="/run/helios/fan-overlays"
CONFIGFS="/sys/kernel/config"
OVERLAYS_DIR="${CONFIGFS}/device-tree/overlays"

ensure_configfs() {
  if ! mountpoint -q "${CONFIGFS}"; then
    mount -t configfs configfs "${CONFIGFS}" 2>/dev/null || true
  fi
  mkdir -p "${OVERLAYS_DIR}"
}

write_dtbo() {
  name="$1"
  b64="$2"
  out="${OVERLAY_ROOT}/${name}.dtbo"
  if [ ! -s "${out}" ]; then
    printf '%s' "${b64}" | base64 -d > "${out}"
    chmod 644 "${out}"
  fi
}

apply_overlay() {
  name="$1"
  dtbo="$2"
  if [ ! -s "${dtbo}" ]; then
    return 0
  fi
  if [ -d "${OVERLAYS_DIR}/${name}" ]; then
    return 0
  fi
  mkdir -p "${OVERLAYS_DIR}/${name}"
  cat "${dtbo}" > "${OVERLAYS_DIR}/${name}/dtbo"
}

seed_fan_defaults() {
  default_cfg="/etc/helios/fan.toml"
  runtime_cfg="/var/lib/helios/fan.toml"
  if [ ! -s "${default_cfg}" ]; then
    return 0
  fi

  mkdir -p "/var/lib/helios" 2>/dev/null || true
  if [ ! -s "${runtime_cfg}" ]; then
    cp "${default_cfg}" "${runtime_cfg}" 2>/dev/null || true
    return 0
  fi

  # Migrate a known bad legacy seed (manual=100 + fixed hwmon0 path) to sane defaults.
  if grep -q 'manual_percent = 100' "${runtime_cfg}" \
    && grep -q 'pwm_path = "/sys/class/hwmon/hwmon0/pwm1"' "${runtime_cfg}"
  then
    cp "${default_cfg}" "${runtime_cfg}" 2>/dev/null || true
    return 0
  fi

  # Migrate the old CM5 seed that shipped with inverted fan behavior.
  # The old seed included this comment + invert=false; preserve user-edited configs.
  if grep -q 'CM5 pwm-fan is active-high; do not invert duty' "${runtime_cfg}" \
    && grep -q 'invert_pwm = false' "${runtime_cfg}"
  then
    cp "${default_cfg}" "${runtime_cfg}" 2>/dev/null || true
  fi
}

mkdir -p "${OVERLAY_ROOT}"
ensure_configfs

write_dtbo "cooling-fan-enable" "0A3+7QAAANoAAAA4AAAAvAAAACgAAAARAAAAEAAAAAAAAAAeAAAAhAAAAAAAAAAAAAAAAAAAAAAAAAABAAAAAAAAAAMAAAANAAAAAGJyY20sYmNtMjcxMgAAAAAAAAABZnJhZ21lbnRAMAAAAAAAAwAAAA0AAAALL2Nvb2xpbmdfZmFuAAAAAAAAAAFfX292ZXJsYXlfXwAAAAADAAAABQAAABdva2F5AAAAAAAAAAIAAAACAAAAAgAAAAljb21wYXRpYmxlAHRhcmdldC1wYXRoAHN0YXR1cwA="
write_dtbo "cooling-fan-levels" "0A3+7QAAAO4AAAA4AAAAyAAAACgAAAARAAAAEAAAAAAAAAAmAAAAkAAAAAAAAAAAAAAAAAAAAAAAAAABAAAAAAAAAAMAAAANAAAAAGJyY20sYmNtMjcxMgAAAAAAAAABZnJhZ21lbnRAMAAAAAAAAwAAAA0AAAALL2Nvb2xpbmdfZmFuAAAAAAAAAAFfX292ZXJsYXlfXwAAAAADAAAAFAAAABcAAAAAAAAASwAAAH0AAACvAAAA+gAAAAIAAAACAAAAAgAAAAljb21wYXRpYmxlAHRhcmdldC1wYXRoAGNvb2xpbmctbGV2ZWxzAA=="
write_dtbo "rp1-pwm1-enable" "0A3+7QAAAO4AAAA4AAAA0AAAACgAAAARAAAAEAAAAAAAAAAeAAAAmAAAAAAAAAAAAAAAAAAAAAAAAAABAAAAAAAAAAMAAAANAAAAAGJyY20sYmNtMjcxMgAAAAAAAAABZnJhZ21lbnRAMAAAAAAAAwAAACMAAAALL2F4aS9wY2llQDEwMDAxMjAwMDAvcnAxL3B3bUA5YzAwMAAAAAAAAV9fb3ZlcmxheV9fAAAAAAMAAAAFAAAAF29rYXkAAAAAAAAAAgAAAAIAAAACAAAACWNvbXBhdGlibGUAdGFyZ2V0LXBhdGgAc3RhdHVzAA=="

apply_overlay "cooling_fan_enable" "${OVERLAY_ROOT}/cooling-fan-enable.dtbo"
apply_overlay "cooling_fan_levels" "${OVERLAY_ROOT}/cooling-fan-levels.dtbo"
apply_overlay "rp1_pwm1_enable" "${OVERLAY_ROOT}/rp1-pwm1-enable.dtbo"
seed_fan_defaults
