#!/bin/bash

set -euo pipefail

BOARD_DIR="$(dirname "$0")"
BOARD_NAME="$(basename "${BOARD_DIR}")"
GENIMAGE_CFG="${BOARD_DIR}/genimage-${BOARD_NAME}.cfg"
GENIMAGE_TMP="${BUILD_DIR}/genimage.tmp"

resolve_repo_root() {
	local candidate
	for candidate in \
		"${HELIOS_REPO_ROOT:-}" \
		"$(realpath -m "${BOARD_DIR}/../../../..")" \
		"$(realpath -m "${BOARD_DIR}/../../../../..")" \
		"$(realpath -m "${BOARD_DIR}/../../../../../..")"
	do
		[ -n "${candidate}" ] || continue
		if [ -d "${candidate}/assets/buildroot" ]; then
			echo "${candidate}"
			return 0
		fi
	done
	return 1
}

REPO_ROOT="$(resolve_repo_root || true)"
if [ -z "${REPO_ROOT}" ]; then
	echo "warning: unable to resolve repo root; custom boot config/overlays may be stale" >&2
fi

# Upstream Pi5/CM5 boot partition (contains firmware blobs and DTBs).
BOOT_IMG_URL="${BOOT_IMG_URL:-https://downloads.raspberrypi.com/raspios_arm64/images/raspios_arm64-2025-12-04/2025-12-04-raspios-trixie-arm64.img.xz}"
BOOT_IMG_SHA256="${BOOT_IMG_SHA256:-f7afb40e587746128538d84f217bf478a23af59484d4db77f2d06bf647f7c82e}"

fetch_boot_image() {
	local url="$1" sha="$2" cache="${3}"
	if [ ! -f "${cache}" ]; then
		mkdir -p "$(dirname "${cache}")"
		echo "Downloading boot image from ${url}"
		curl -L --fail -o "${cache}.tmp" "${url}"
		mv "${cache}.tmp" "${cache}"
	fi
	echo "${sha}  ${cache}" | sha256sum -c -
}

extract_boot_partition() {
	local img="$1" out_dir="$2"
	local offset
	offset=$(fdisk -l "${img}" | awk '/\.img1/ {print $2}')
	offset=$((offset * 512))
	rm -rf "${out_dir}"
	mkdir -p "${out_dir}"
	mcopy -s -i "${img}@@${offset}" ::* "${out_dir}"
}

# Prepare boot files from the official Pi5/CM5 image to ensure firmware alignment.
BOOT_CACHE="${HOST_DIR}/../.cache/helios/pi5-boot"
BOOT_IMG="${BOOT_CACHE}/boot.img"
fetch_boot_image "${BOOT_IMG_URL}" "${BOOT_IMG_SHA256}" "${BOOT_IMG}.xz"
if [ ! -f "${BOOT_IMG}" ]; then
	unxz -k "${BOOT_IMG}.xz"
fi
OFFICIAL_BOOT="${BOOT_CACHE}/bootfs"
extract_boot_partition "${BOOT_IMG}" "${OFFICIAL_BOOT}"
rm -rf "${BINARIES_DIR}/rpi-firmware"
mkdir -p "${BINARIES_DIR}/rpi-firmware"
cp -a "${OFFICIAL_BOOT}/." "${BINARIES_DIR}/rpi-firmware/"
# Prevent stale EEPROM update payloads from being re-included on incremental builds.
rm -f "${BINARIES_DIR}/pieeprom.upd" "${BINARIES_DIR}/pieeprom.sig"

# Optional: stage a bootloader (EEPROM) update into the boot filesystem.
#
# IMPORTANT: leaving `pieeprom.upd` on BOOT makes the firmware attempt an EEPROM
# update very early in the boot chain. If the target's current bootloader can't
# consume the update (or loops applying it), the OS may never reach userspace.
#
# Default is OFF; enable explicitly when you intend to ship a dedicated "bootfix"
# image or you are certain the update is safe for your fleet.
#
# Usage:
#   HELIOS_STAGE_PIEEPROM_UPDATE=1 make ...
STAGE_PIEEPROM_UPDATE=0
if [ "${HELIOS_STAGE_PIEEPROM_UPDATE:-0}" = "1" ]; then
	STAGE_PIEEPROM_UPDATE=1
	PIEEPROM_SRC_DIR="${BOARD_DIR}/bootloader"
	PIEEPROM_UPD="${PIEEPROM_SRC_DIR}/pieeprom-2025-12-08.upd"
	PIEEPROM_SIG="${PIEEPROM_SRC_DIR}/pieeprom-2025-12-08.sig"
	if [ -f "${PIEEPROM_UPD}" ]; then
		install -m 0644 "${PIEEPROM_UPD}" "${BINARIES_DIR}/rpi-firmware/pieeprom.upd"
		[ -f "${PIEEPROM_SIG}" ] && install -m 0644 "${PIEEPROM_SIG}" "${BINARIES_DIR}/rpi-firmware/pieeprom.sig"
	fi
fi

# Keep EEPROM update payloads opt-in only (avoid accidental early EEPROM writes).
if [ "${STAGE_PIEEPROM_UPDATE}" != "1" ]; then
	rm -f "${BINARIES_DIR}/rpi-firmware/pieeprom.upd" "${BINARIES_DIR}/rpi-firmware/pieeprom.sig"
fi

# Prefer repo-supplied boot configuration so kernel and overlays match our image.
REPO_CONFIG_TXT="${REPO_ROOT}/assets/buildroot/config.txt"
REPO_CMDLINE_TXT="${REPO_ROOT}/assets/buildroot/cmdline.txt"
if [ -f "${REPO_CONFIG_TXT}" ]; then
	install -m 0644 "${REPO_CONFIG_TXT}" "${BINARIES_DIR}/rpi-firmware/config.txt"
fi
if [ -f "${REPO_CMDLINE_TXT}" ]; then
	install -m 0644 "${REPO_CMDLINE_TXT}" "${BINARIES_DIR}/rpi-firmware/cmdline.txt"
fi

# Copy staged firmware into the root of BINARIES_DIR and keep Buildroot-generated
# DTBs authoritative: `--ignore-existing` prevents overwriting files we already
# built (e.g. patched bcm2712 CM5 DTBs), while still copying extra fallback DTBs.
rsync -a --ignore-existing "${BINARIES_DIR}/rpi-firmware/" "${BINARIES_DIR}/"
if [ -d "${BINARIES_DIR}/rpi-firmware/overlays" ]; then
	mkdir -p "${BINARIES_DIR}/overlays"
	rsync -a "${BINARIES_DIR}/rpi-firmware/overlays/" "${BINARIES_DIR}/overlays/"
fi

# Keep Buildroot-generated DTBs authoritative, but add upstream bcm2712 aliases
# not produced by our kernel build. Older CM5 EEPROM revisions may fall back to
# alternate DTB names before CM5 carrier auto-detection is available.
for dtb in "${BINARIES_DIR}"/rpi-firmware/bcm2712*.dtb; do
	[ -f "${dtb}" ] || continue
	dtb_name="$(basename "${dtb}")"
	if [ ! -f "${BINARIES_DIR}/${dtb_name}" ]; then
		install -m 0644 "${dtb}" "${BINARIES_DIR}/${dtb_name}"
	fi
done

# Older CM5 EEPROM revisions may ignore/partially parse config.txt and try the
# default Pi 5 kernel name. Provide a compatibility copy so both old/new EEPROM
# loaders can boot the same image.
KERNEL_FROM_CONFIG="$(sed -n 's/^kernel=//p' "${BINARIES_DIR}/rpi-firmware/config.txt" | head -n 1)"
if [ -n "${KERNEL_FROM_CONFIG}" ] && [ -f "${BINARIES_DIR}/${KERNEL_FROM_CONFIG}" ]; then
	if [ "${KERNEL_FROM_CONFIG}" != "kernel_2712.img" ] && [ ! -f "${BINARIES_DIR}/kernel_2712.img" ]; then
		install -m 0644 "${BINARIES_DIR}/${KERNEL_FROM_CONFIG}" "${BINARIES_DIR}/kernel_2712.img"
	fi
	if [ "${KERNEL_FROM_CONFIG}" != "kernel8.img" ] && [ ! -f "${BINARIES_DIR}/kernel8.img" ]; then
		install -m 0644 "${BINARIES_DIR}/${KERNEL_FROM_CONFIG}" "${BINARIES_DIR}/kernel8.img"
	fi
fi

if [ "${HELIOS_ROOT_BY_LABEL:-0}" = "1" ] && [ -f "${BINARIES_DIR}/rpi-firmware/cmdline.txt" ]; then
	sed -E -i 's#root=/dev/mmcblk0p2#root=LABEL=ACTIVE#g' "${BINARIES_DIR}/rpi-firmware/cmdline.txt"
fi

# Compile repo-owned DT overlays (e.g. OV9782) into rpi-firmware overlays right before genimage.
# Buildroot will regenerate the rpi-firmware overlay tree during the build; doing this here
# ensures the dtbo files exist when boot.vfat is constructed.
if [ -n "${REPO_ROOT}" ]; then
	DT_OVERLAY_SRC="${REPO_ROOT}/assets/buildroot/dt-overlays"
	if [ -d "${DT_OVERLAY_SRC}" ]; then
		DTC="${HOST_DIR}/bin/dtc"
		if [ ! -x "${DTC}" ]; then
			DTC="$(command -v dtc || true)"
		fi
		if [ -z "${DTC}" ]; then
			echo "dtc not found (needed to compile dt-overlays from ${DT_OVERLAY_SRC})" >&2
			exit 1
		fi

		mkdir -p "${BINARIES_DIR}/rpi-firmware/overlays"
		for dts in "${DT_OVERLAY_SRC}"/*.dts; do
			[ -f "${dts}" ] || continue
			name="$(basename "${dts}" .dts)"
			"${DTC}" -@ -I dts -O dtb -o "${BINARIES_DIR}/rpi-firmware/overlays/${name}.dtbo" "${dts}"
		done
		# Ensure custom overlays are included in the BOOT partition inputs.
		mkdir -p "${BINARIES_DIR}/overlays"
		rsync -a "${BINARIES_DIR}/rpi-firmware/overlays/" "${BINARIES_DIR}/overlays/"
	fi
fi

# generate genimage from template if a board specific variant doesn't exists
if [ ! -e "${GENIMAGE_CFG}" ]; then
	GENIMAGE_CFG="${BINARIES_DIR}/genimage.cfg"
	FILES=()

	for i in "${BINARIES_DIR}"/bcm*.dtb; do
		[ -f "${i}" ] && FILES+=( "$(basename "${i}")" )
	done
	for f in start.elf start_cd.elf start_db.elf start_x.elf start4.elf start4cd.elf start4db.elf start4x.elf fixup.dat fixup_cd.dat fixup_db.dat fixup_x.dat fixup4.dat fixup4cd.dat fixup4db.dat fixup4x.dat bootcode.bin config.txt cmdline.txt rp1.bin kernel_2712.img kernel8.img; do
		[ -e "${BINARIES_DIR}/${f}" ] && FILES+=( "${f}" )
	done
	if [ "${STAGE_PIEEPROM_UPDATE}" = "1" ]; then
		for f in pieeprom.upd pieeprom.sig; do
			[ -e "${BINARIES_DIR}/${f}" ] && FILES+=( "${f}" )
		done
	fi
	[ -d "${BINARIES_DIR}/overlays" ] && FILES+=( "overlays" )
	
	# Keep Pi 5 os_config metadata opt-in for compatibility with older CM5 EEPROMs.
	if [ "${HELIOS_INCLUDE_OS_CONFIG:-0}" = "1" ] && [ -f "${BOARD_DIR}/os_config.json" ]; then
		install -m 0644 "${BOARD_DIR}/os_config.json" "${BINARIES_DIR}/os_config.json"
		case " ${FILES[*]} " in
			*" os_config.json "*) ;; *) FILES+=( "os_config.json" ) ;;
		esac
	fi

	KERNEL=$(sed -n 's/^kernel=//p' "${BINARIES_DIR}/rpi-firmware/config.txt")
	if [ -n "${KERNEL}" ]; then
		case " ${FILES[*]} " in
			*" ${KERNEL} "*) ;; *) FILES+=( "${KERNEL}" ) ;;
		esac
	fi

	BOOT_FILES=$(printf '\\t\\t\\t"%s",\\n' "${FILES[@]}")
	sed "s|#BOOT_FILES#|${BOOT_FILES}|" "${BOARD_DIR}/genimage.cfg.in" \
		> "${GENIMAGE_CFG}"
fi

# Pass an empty rootpath. genimage makes a full copy of the given rootpath to
# ${GENIMAGE_TMP}/root so passing TARGET_DIR would be a waste of time and disk
# space. We don't rely on genimage to build the rootfs image, just to insert a
# pre-built one in the disk image.

trap 'rm -rf "${ROOTPATH_TMP}"' EXIT
ROOTPATH_TMP="$(mktemp -d)"

rm -rf "${GENIMAGE_TMP}"

genimage \
	--rootpath "${ROOTPATH_TMP}"   \
	--tmppath "${GENIMAGE_TMP}"    \
	--inputpath "${BINARIES_DIR}"  \
	--outputpath "${BINARIES_DIR}" \
	--config "${GENIMAGE_CFG}"

exit $?
