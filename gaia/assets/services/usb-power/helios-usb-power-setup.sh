#!/bin/sh
set -eu

LOG_PREFIX="[usb-power]"
USB_POWER_ENV_FILE=${USB_POWER_ENV_FILE:-/etc/helios/usb-power.env}

log() {
  echo "${LOG_PREFIX} $*"
}

USB_POWER_USB_A_GPIO_OVERRIDE="${USB_POWER_USB_A_GPIO:-}"
USB_POWER_USB_C_GPIO_OVERRIDE="${USB_POWER_USB_C_GPIO:-}"
USB_POWER_USB_A_ACTIVE_HIGH_OVERRIDE="${USB_POWER_USB_A_ACTIVE_HIGH:-}"
USB_POWER_USB_C_ACTIVE_HIGH_OVERRIDE="${USB_POWER_USB_C_ACTIVE_HIGH:-}"
USB_POWER_USB_A_ENABLED_OVERRIDE="${USB_POWER_USB_A_ENABLED:-}"
USB_POWER_USB_C_ENABLED_OVERRIDE="${USB_POWER_USB_C_ENABLED:-}"
USB_POWER_ENABLED_OVERRIDE="${USB_POWER_ENABLED:-}"
USB_POWER_TUNING_ENABLED_OVERRIDE="${USB_POWER_TUNING_ENABLED:-}"
USB_POWER_DISABLE_AUTOSUSPEND_OVERRIDE="${USB_POWER_DISABLE_AUTOSUSPEND:-}"
USB_POWER_DISABLE_USB2_LPM_OVERRIDE="${USB_POWER_DISABLE_USB2_LPM:-}"
USB_POWER_FORCE_POWER_CONTROL_ON_OVERRIDE="${USB_POWER_FORCE_POWER_CONTROL_ON:-}"

if [ -f "${USB_POWER_ENV_FILE}" ]; then
  # shellcheck disable=SC1090
  . "${USB_POWER_ENV_FILE}"
fi

if [ -n "${USB_POWER_USB_A_GPIO_OVERRIDE}" ]; then
  USB_POWER_USB_A_GPIO="${USB_POWER_USB_A_GPIO_OVERRIDE}"
fi
if [ -n "${USB_POWER_USB_C_GPIO_OVERRIDE}" ]; then
  USB_POWER_USB_C_GPIO="${USB_POWER_USB_C_GPIO_OVERRIDE}"
fi
if [ -n "${USB_POWER_USB_A_ACTIVE_HIGH_OVERRIDE}" ]; then
  USB_POWER_USB_A_ACTIVE_HIGH="${USB_POWER_USB_A_ACTIVE_HIGH_OVERRIDE}"
fi
if [ -n "${USB_POWER_USB_C_ACTIVE_HIGH_OVERRIDE}" ]; then
  USB_POWER_USB_C_ACTIVE_HIGH="${USB_POWER_USB_C_ACTIVE_HIGH_OVERRIDE}"
fi
if [ -n "${USB_POWER_USB_A_ENABLED_OVERRIDE}" ]; then
  USB_POWER_USB_A_ENABLED="${USB_POWER_USB_A_ENABLED_OVERRIDE}"
fi
if [ -n "${USB_POWER_USB_C_ENABLED_OVERRIDE}" ]; then
  USB_POWER_USB_C_ENABLED="${USB_POWER_USB_C_ENABLED_OVERRIDE}"
fi
if [ -n "${USB_POWER_ENABLED_OVERRIDE}" ]; then
  USB_POWER_ENABLED="${USB_POWER_ENABLED_OVERRIDE}"
fi
if [ -n "${USB_POWER_TUNING_ENABLED_OVERRIDE}" ]; then
  USB_POWER_TUNING_ENABLED="${USB_POWER_TUNING_ENABLED_OVERRIDE}"
fi
if [ -n "${USB_POWER_DISABLE_AUTOSUSPEND_OVERRIDE}" ]; then
  USB_POWER_DISABLE_AUTOSUSPEND="${USB_POWER_DISABLE_AUTOSUSPEND_OVERRIDE}"
fi
if [ -n "${USB_POWER_DISABLE_USB2_LPM_OVERRIDE}" ]; then
  USB_POWER_DISABLE_USB2_LPM="${USB_POWER_DISABLE_USB2_LPM_OVERRIDE}"
fi
if [ -n "${USB_POWER_FORCE_POWER_CONTROL_ON_OVERRIDE}" ]; then
  USB_POWER_FORCE_POWER_CONTROL_ON="${USB_POWER_FORCE_POWER_CONTROL_ON_OVERRIDE}"
fi

USB_POWER_ENABLED=${USB_POWER_ENABLED:-1}
USB_POWER_USB_A_GPIO=${USB_POWER_USB_A_GPIO:-}
USB_POWER_USB_C_GPIO=${USB_POWER_USB_C_GPIO:-}
USB_POWER_USB_A_ACTIVE_HIGH=${USB_POWER_USB_A_ACTIVE_HIGH:-1}
USB_POWER_USB_C_ACTIVE_HIGH=${USB_POWER_USB_C_ACTIVE_HIGH:-1}
USB_POWER_USB_A_ENABLED=${USB_POWER_USB_A_ENABLED:-1}
USB_POWER_USB_C_ENABLED=${USB_POWER_USB_C_ENABLED:-1}
USB_POWER_TUNING_ENABLED=${USB_POWER_TUNING_ENABLED:-1}
USB_POWER_DISABLE_AUTOSUSPEND=${USB_POWER_DISABLE_AUTOSUSPEND:-1}
USB_POWER_DISABLE_USB2_LPM=${USB_POWER_DISABLE_USB2_LPM:-1}
USB_POWER_FORCE_POWER_CONTROL_ON=${USB_POWER_FORCE_POWER_CONTROL_ON:-1}
USB_POWER_DEVICE_PATH=""
USB_POWER_APPLY_GPIO=1

if [ "${1:-}" = "--device" ] && [ -n "${2:-}" ]; then
  USB_POWER_DEVICE_PATH="/sys${2}"
  USB_POWER_APPLY_GPIO=0
fi

if [ "${USB_POWER_ENABLED}" -eq 0 ]; then
  log "USB power control disabled"
  USB_POWER_APPLY_GPIO=0
fi

resolve_gpio() {
  gpio="$1"
  if [ -d "/sys/class/gpio/gpio${gpio}" ]; then
    echo "${gpio}"
    return 0
  fi

  for chip in /sys/class/gpio/gpiochip*; do
    if [ -f "${chip}/label" ] && [ "$(cat "${chip}/label")" = "pinctrl-rp1" ]; then
      base="$(cat "${chip}/base" 2>/dev/null || true)"
      if [ -n "${base}" ]; then
        echo "$((base + gpio))"
        return 0
      fi
    fi
  done

  echo "${gpio}"
}

set_gpio() {
  name="$1"
  gpio="$2"
  active_high="$3"
  enabled="$4"

  if [ -z "${gpio}" ]; then
    log "no ${name} GPIO configured; skipping"
    return 0
  fi

  orig_gpio="${gpio}"
  resolved_gpio="$(resolve_gpio "${gpio}")"
  if [ "${resolved_gpio}" != "${gpio}" ]; then
    log "remapped ${name} gpio${gpio} -> gpio${resolved_gpio}"
  fi

  gpio_path="/sys/class/gpio/gpio${resolved_gpio}"
  if [ ! -d "${gpio_path}" ]; then
    if ! echo "${resolved_gpio}" > /sys/class/gpio/export 2>/dev/null; then
      log "${name} gpio${resolved_gpio} export failed; using RP1 RIO"
      set_gpio_rio "${name}" "${orig_gpio}" "${active_high}" "${enabled}"
      return 0
    fi
  fi

  active_low_supported=0
  if [ -f "${gpio_path}/active_low" ]; then
    active_low_supported=1
    if [ "${active_high}" -eq 1 ]; then
      echo 0 > "${gpio_path}/active_low" 2>/dev/null || true
    else
      echo 1 > "${gpio_path}/active_low" 2>/dev/null || true
    fi
  fi

  if ! echo "out" > "${gpio_path}/direction" 2>/dev/null; then
    log "${name} gpio${resolved_gpio} direction failed; using RP1 RIO"
    set_gpio_rio "${name}" "${orig_gpio}" "${active_high}" "${enabled}"
    return 0
  fi
  if [ "${active_low_supported}" -eq 1 ]; then
    value_on=1
    value_off=0
  else
    if [ "${active_high}" -eq 1 ]; then
      value_on=1
      value_off=0
    else
      value_on=0
      value_off=1
    fi
  fi

  if [ "${enabled}" -eq 1 ]; then
    if ! echo "${value_on}" > "${gpio_path}/value" 2>/dev/null; then
      log "${name} gpio${resolved_gpio} value set failed; using RP1 RIO"
      set_gpio_rio "${name}" "${orig_gpio}" "${active_high}" "${enabled}"
      return 0
    fi
    log "${name} enabled on gpio${resolved_gpio}"
  else
    if ! echo "${value_off}" > "${gpio_path}/value" 2>/dev/null; then
      log "${name} gpio${resolved_gpio} value set failed; using RP1 RIO"
      set_gpio_rio "${name}" "${orig_gpio}" "${active_high}" "${enabled}"
      return 0
    fi
    log "${name} disabled on gpio${resolved_gpio}"
  fi
}

set_gpio_rio() {
  name="$1"
  gpio="$2"
  active_high="$3"
  enabled="$4"

  if [ -z "${gpio}" ]; then
    log "no ${name} GPIO configured; skipping"
    return 0
  fi

  if [ ! -x "/sbin/devmem" ] || [ ! -f "/sys/firmware/devicetree/base/axi/pcie@1000120000/rp1/gpio@d0000/reg" ]; then
    log "${name} gpio${gpio} RP1 RIO unavailable; skipping"
    return 0
  fi

  python3 - "$gpio" "$active_high" "$enabled" <<'PY'
import struct
import subprocess
import sys

gpio = int(sys.argv[1])
active_high = int(sys.argv[2])
enabled = int(sys.argv[3])

def read_u32(path):
    return struct.unpack(">I", open(path, "rb").read())[0]

def parse_ranges(data, child_ac, parent_ac, size_cells):
    entries = []
    entry_cells = child_ac + parent_ac + size_cells
    entry_size = entry_cells * 4
    for i in range(0, len(data), entry_size):
        chunk = data[i:i + entry_size]
        if len(chunk) < entry_size:
            break
        off = 0
        def read_n(n):
            nonlocal off
            val = 0
            for _ in range(n):
                val = (val << 32) | struct.unpack(">I", chunk[off:off + 4])[0]
                off += 4
            return val
        child = read_n(child_ac)
        parent = read_n(parent_ac)
        size = read_n(size_cells)
        entries.append((child, parent, size))
    return entries

reg_path = "/sys/firmware/devicetree/base/axi/pcie@1000120000/rp1/gpio@d0000/reg"
data = open(reg_path, "rb").read()
if len(data) < 32:
    sys.exit(0)

def be64(offset):
    return struct.unpack(">Q", data[offset:offset + 8])[0]

# reg entries are (addr,size) pairs; index 1 is RIO base (bus address)
rio_bus = be64(16)

pcie_path = "/sys/firmware/devicetree/base/axi/pcie@1000120000"
rp1_path = pcie_path + "/rp1"

pcie_bus_ac = read_u32(pcie_path + "/#address-cells")
pcie_size_cells = read_u32(pcie_path + "/#size-cells")
root_ac = read_u32("/sys/firmware/devicetree/base/#address-cells")

rp1_child_ac = read_u32(rp1_path + "/#address-cells")
rp1_size_cells = read_u32(rp1_path + "/#size-cells")

pcie_ranges = open(pcie_path + "/ranges", "rb").read()
rp1_ranges = open(rp1_path + "/ranges", "rb").read()

pcie_entries = parse_ranges(pcie_ranges, pcie_bus_ac, root_ac, pcie_size_cells)
rp1_entries = parse_ranges(rp1_ranges, rp1_child_ac, pcie_bus_ac, rp1_size_cells)

pcie_bus = None
for child, parent, size in rp1_entries:
    if child <= rio_bus < child + size:
        pcie_bus = parent + (rio_bus - child)
        break
if pcie_bus is None:
    sys.exit(0)

flags = (pcie_bus >> 64) & 0xffffffff
addr64 = pcie_bus & ((1 << 64) - 1)

rio_base = None
for child, parent, size in pcie_entries:
    c_flags = (child >> 64) & 0xffffffff
    c_addr = child & ((1 << 64) - 1)
    if c_flags == flags and c_addr <= addr64 < c_addr + size:
        rio_base = parent + (addr64 - c_addr)
        break

if rio_base is None:
    sys.exit(0)

RIO_OUT = 0x00
RIO_OE = 0x04
SET_OFFSET = 0x2000
CLR_OFFSET = 0x3000

bit = 1 << gpio

def devmem_write(addr, value):
    subprocess.run(["/sbin/devmem", hex(addr), "32", hex(value)], check=True)

# ensure output enabled
devmem_write(rio_base + RIO_OE + SET_OFFSET, bit)

value = 1 if enabled else 0
if active_high == 0:
    value = 0 if enabled else 1

if value:
    devmem_write(rio_base + RIO_OUT + SET_OFFSET, bit)
else:
    devmem_write(rio_base + RIO_OUT + CLR_OFFSET, bit)
PY

  log "${name} set via RP1 RIO on gpio${gpio}"
}

disable_autosuspend() {
  if [ "${USB_POWER_DISABLE_AUTOSUSPEND}" -ne 1 ]; then
    return 0
  fi
  if [ -w "/sys/module/usbcore/parameters/autosuspend" ]; then
    echo -1 > /sys/module/usbcore/parameters/autosuspend 2>/dev/null || true
  fi
}

apply_usb_device_tuning() {
  device="$1"
  [ -d "${device}" ] || return 0

  if [ "${USB_POWER_FORCE_POWER_CONTROL_ON}" -eq 1 ] && [ -w "${device}/power/control" ]; then
    echo on > "${device}/power/control" 2>/dev/null || true
  fi

  if [ "${USB_POWER_DISABLE_USB2_LPM}" -eq 1 ] && [ -w "${device}/power/usb2_hardware_lpm" ]; then
    echo disabled > "${device}/power/usb2_hardware_lpm" 2>/dev/null || true
  fi
}

apply_usb_tuning_all() {
  for device in /sys/bus/usb/devices/*; do
    [ -d "${device}" ] || continue
    apply_usb_device_tuning "${device}"
  done
}

if [ "${USB_POWER_APPLY_GPIO}" -eq 1 ]; then
  set_gpio "usb_a" "${USB_POWER_USB_A_GPIO}" "${USB_POWER_USB_A_ACTIVE_HIGH}" "${USB_POWER_USB_A_ENABLED}"
  set_gpio "usb_c" "${USB_POWER_USB_C_GPIO}" "${USB_POWER_USB_C_ACTIVE_HIGH}" "${USB_POWER_USB_C_ENABLED}"
fi

if [ "${USB_POWER_TUNING_ENABLED}" -eq 1 ]; then
  disable_autosuspend
  if [ -n "${USB_POWER_DEVICE_PATH}" ]; then
    apply_usb_device_tuning "${USB_POWER_DEVICE_PATH}"
  else
    apply_usb_tuning_all
  fi
fi
