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
BASE_BOARD_DIR="$(realpath -m "${BOARD_DIR}/../raspberrypicm5io")"
LAYOUT_DIR="${REPO_ROOT}/assets/generated/storage-layouts"
SQUASHFS_LAYOUT_MANIFEST="${LAYOUT_DIR}/squashfs-ab.toml"
SQUASHFS_LAYOUT_ENV="${LAYOUT_DIR}/squashfs-ab.env"
if [ -f "${SQUASHFS_LAYOUT_ENV}" ]; then
	# shellcheck disable=SC1090
	. "${SQUASHFS_LAYOUT_ENV}"
fi

BOOT_IMG_URL="${BOOT_IMG_URL:-https://downloads.raspberrypi.com/raspios_arm64/images/raspios_arm64-2025-12-04/2025-12-04-raspios-trixie-arm64.img.xz}"
BOOT_IMG_SHA256="${BOOT_IMG_SHA256:-f7afb40e587746128538d84f217bf478a23af59484d4db77f2d06bf647f7c82e}"
INITRAMFS_NAME="helios-squashfs-initramfs.cpio.gz"
KERNEL_IMAGE_NAME="Image"
KERNEL_2712_NAME="kernel_2712.img"
KERNEL8_NAME="kernel8.img"
INITRAMFS_2712_NAME="initramfs_2712"
INITRAMFS8_NAME="initramfs8"
SINGLE_KERNEL_NAME="${HELIOS_BOOT_SINGLE_KERNEL_NAME:-kernel8.img}"
SINGLE_INITRAMFS_NAME="${HELIOS_BOOT_SINGLE_INITRAMFS_NAME:-initramfs8}"

boot_kernel_aliases() {
	if [ -n "${SINGLE_KERNEL_NAME}" ]; then
		printf '%s\n' "${SINGLE_KERNEL_NAME}"
		return 0
	fi

	printf '%s\n' "${KERNEL_2712_NAME}" "${KERNEL8_NAME}"
}

boot_initramfs_aliases() {
	if [ -n "${SINGLE_INITRAMFS_NAME}" ]; then
		printf '%s\n' "${SINGLE_INITRAMFS_NAME}"
		return 0
	fi

	printf '%s\n' "${INITRAMFS_2712_NAME}" "${INITRAMFS8_NAME}"
}

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

build_initramfs() {
	local init_root="${BUILD_DIR}/helios-squashfs-initramfs"
	local out="${BINARIES_DIR}/${INITRAMFS_NAME}"
	local target_bin="${TARGET_DIR}/usr/bin"
	local target_lib="${TARGET_DIR}/lib"

	rm -rf "${init_root}"
	mkdir -p "${init_root}/bin" "${init_root}/lib" "${init_root}/usr" \
		"${init_root}/proc" "${init_root}/sys" "${init_root}/dev" "${init_root}/etc/helios" \
		"${init_root}/mnt/lower" "${init_root}/mnt/data" "${init_root}/newroot"
	ln -sfn lib "${init_root}/lib64"
	ln -sfn ../lib "${init_root}/usr/lib64"

	install -m 0755 "${target_bin}/busybox" "${init_root}/bin/busybox"
	ln -sf busybox "${init_root}/bin/sh"
	ln -sf busybox "${init_root}/bin/mount"
	ln -sf busybox "${init_root}/bin/mkdir"
	ln -sf busybox "${init_root}/bin/sleep"
	ln -sf busybox "${init_root}/bin/sync"
	ln -sf busybox "${init_root}/bin/switch_root"
	ln -sf busybox "${init_root}/bin/cat"
	ln -sf busybox "${init_root}/bin/umount"

	install -m 0755 "${target_lib}/ld-linux-aarch64.so.1" "${init_root}/lib/ld-linux-aarch64.so.1"
	install -m 0644 "${target_lib}/libc.so.6" "${init_root}/lib/libc.so.6"
	install -m 0644 "${target_lib}/libresolv.so.2" "${init_root}/lib/libresolv.so.2"
	if [ -f "${SQUASHFS_LAYOUT_ENV}" ]; then
		install -m 0644 "${SQUASHFS_LAYOUT_ENV}" "${init_root}/etc/helios/storage-layout.env"
	fi

	cat > "${init_root}/init" <<'EOF'
#!/bin/sh
set -eu

panic_shell() {
	mark "panic:$*"
	echo "[helios-squashfs] $*"
	exec sh
}

LAYOUT_ENV="/etc/helios/storage-layout.env"
if [ -r "${LAYOUT_ENV}" ]; then
	# shellcheck source=/etc/helios/storage-layout.env
	. "${LAYOUT_ENV}"
fi

BOOT_PARTITION="${HELIOS_LAYOUT_BOOT_PARTITION:-1}"
SLOT_A_NAME="${HELIOS_LAYOUT_SLOT_A_NAME:-ROOT_A}"
SLOT_A_PARTITION="${HELIOS_LAYOUT_SLOT_A_PARTITION:-2}"
SLOT_B_NAME="${HELIOS_LAYOUT_SLOT_B_NAME:-ROOT_B}"
SLOT_B_PARTITION="${HELIOS_LAYOUT_SLOT_B_PARTITION:-3}"
DATA_PARTITION="${HELIOS_LAYOUT_DATA_PARTITION:-4}"

ROOT_DEV="/dev/mmcblk0p${SLOT_A_PARTITION}"
BOOT_DEV="/dev/mmcblk0p${BOOT_PARTITION}"
DATA_DEV=""
DEBUG_MOUNT="/mnt/boot-debug"
REQUEST_MOUNT="/mnt/boot-request"
DATA_MARKER="${HELIOS_LAYOUT_DATA_SECONDARY_MARKER:-/mnt/data/.provision.data_v1}"
REPARTITION_REQUEST_PATH="${REQUEST_MOUNT}/helios/ota/repartition-request.env"
ROOT_CANDIDATES=""
BOOT_CANDIDATES=""
DATA_CANDIDATES=""
DATA_DEV_EXPLICIT=0
BOOT_DEBUG=0
FORCE_TMPFS_WRITABLE=0
OVERLAY_SLOT="root-a"
ROOT_SLOT="${SLOT_A_NAME}"

is_mounted() {
	local target="$1"
	local dev mountpoint rest

	while read -r dev mountpoint rest; do
		[ "${mountpoint}" = "${target}" ] && return 0
	done < /proc/mounts

	return 1
}

partition_base() {
	local dev="$1"

	case "${dev}" in
		/dev/mmcblk*p[0-9]*|/dev/nvme*n*p[0-9]*)
			echo "${dev%p[0-9]*}"
			;;
		/dev/*[0-9]*)
			echo "${dev%[0-9]*}"
			;;
		*)
			echo "${dev}"
			;;
	esac
}

partition_dev() {
	local base="$1"
	local part="$2"

	case "${base}" in
		*[0-9])
			echo "${base}p${part}"
			;;
		*)
			echo "${base}${part}"
			;;
	esac
}

build_candidate_devices() {
	local part="$1"
	printf '/dev/mmcblk0p%s /dev/mmcblk1p%s /dev/sda%s /dev/sdb%s /dev/nvme0n1p%s' \
		"${part}" "${part}" "${part}" "${part}" "${part}"
}

ROOT_CANDIDATES="$(build_candidate_devices "${SLOT_A_PARTITION}") $(build_candidate_devices "${SLOT_B_PARTITION}")"
BOOT_CANDIDATES="$(build_candidate_devices "${BOOT_PARTITION}")"
DATA_CANDIDATES="$(build_candidate_devices "${DATA_PARTITION}") $(build_candidate_devices "${SLOT_B_PARTITION}")"

wait_for_block() {
	local dev="$1"
	local root_wait=0

	case "${dev}" in
		/dev/*) ;;
		*) return 1 ;;
	esac

	while [ ! -b "${dev}" ] && [ "${root_wait}" -lt 200 ]; do
		sleep 0.1
		root_wait=$((root_wait + 1))
	done

	[ -b "${dev}" ]
}

mount_boot_debug() {
	local candidate

	mkdir -p "${DEBUG_MOUNT}" 2>/dev/null || true
	if is_mounted "${DEBUG_MOUNT}"; then
		return 0
	fi

	for candidate in "${BOOT_DEV}" ${BOOT_CANDIDATES}; do
		[ -n "${candidate}" ] || continue
		wait_for_block "${candidate}" || continue
		if mount -t vfat -o rw "${candidate}" "${DEBUG_MOUNT}" 2>/dev/null; then
			BOOT_DEV="${candidate}"
			return 0
		fi
	done

	return 1
}

mount_boot_request() {
	local candidate

	mkdir -p "${REQUEST_MOUNT}" 2>/dev/null || true
	if is_mounted "${REQUEST_MOUNT}"; then
		return 0
	fi

	for candidate in "${BOOT_DEV}" ${BOOT_CANDIDATES}; do
		[ -n "${candidate}" ] || continue
		wait_for_block "${candidate}" || continue
		if mount -t vfat -o ro "${candidate}" "${REQUEST_MOUNT}" 2>/dev/null; then
			BOOT_DEV="${candidate}"
			return 0
		fi
	done

	return 1
}

offline_data_borrow_requested() {
	local found=1

	if mount_boot_request; then
		if [ -f "${REPARTITION_REQUEST_PATH}" ]; then
			found=0
		fi
		umount "${REQUEST_MOUNT}" 2>/dev/null || true
	fi

	return "${found}"
}

mark() {
	local stage="$1"
	[ "${BOOT_DEBUG}" -eq 1 ] || return 0
	if mount_boot_debug; then
		mkdir -p "${DEBUG_MOUNT}/helios/squashfs-debug" 2>/dev/null || true
		printf '%s\n' "${stage}" > "${DEBUG_MOUNT}/helios/squashfs-debug/last-stage.txt" 2>/dev/null || true
		sync 2>/dev/null || true
	fi
}

record_selection() {
	[ "${BOOT_DEBUG}" -eq 1 ] || return 0
	if mount_boot_debug; then
		mkdir -p "${DEBUG_MOUNT}/helios/squashfs-debug" 2>/dev/null || true
		{
			printf 'root_cmd=%s\n' "${ROOT_DEV}"
			printf 'root_slot=%s\n' "${ROOT_SLOT}"
			printf 'overlay_slot=%s\n' "${OVERLAY_SLOT}"
			printf 'data_dev=%s\n' "${DATA_DEV}"
		} > "${DEBUG_MOUNT}/helios/squashfs-debug/selection.txt" 2>/dev/null || true
		sync 2>/dev/null || true
	fi
	if is_mounted /mnt/data; then
		mkdir -p /mnt/data/helios/squashfs-debug 2>/dev/null || true
		{
			printf 'root_cmd=%s\n' "${ROOT_DEV}"
			printf 'root_slot=%s\n' "${ROOT_SLOT}"
			printf 'overlay_slot=%s\n' "${OVERLAY_SLOT}"
			printf 'data_dev=%s\n' "${DATA_DEV}"
		} > /mnt/data/helios/squashfs-debug/selection.txt 2>/dev/null || true
		sync 2>/dev/null || true
	fi
}

resolve_root_dev() {
	local candidate slot

	mkdir -p /mnt/probe
	case "${ROOT_DEV}" in
		/dev/*)
			wait_for_block "${ROOT_DEV}" || return 1
			[ -b "${ROOT_DEV}" ] || return 1
			if mount -t squashfs -o ro "${ROOT_DEV}" /mnt/probe 2>/dev/null; then
				umount /mnt/probe 2>/dev/null || true
				echo "${ROOT_DEV}"
				return 0
			fi
			return 1
			;;
	esac

	if slot="$(resolve_requested_root_slot)"; then
		ROOT_SLOT="${slot}"
		candidate="$(root_device_for_slot "${slot}")" || return 1
		wait_for_block "${candidate}" || return 1
		[ -b "${candidate}" ] || return 1
		if mount -t squashfs -o ro "${candidate}" /mnt/probe 2>/dev/null; then
			umount /mnt/probe 2>/dev/null || true
			echo "${candidate}"
			return 0
		fi
		return 1
	fi

	for candidate in "${ROOT_DEV}" ${ROOT_CANDIDATES}; do
		[ -n "${candidate}" ] || continue
		wait_for_block "${candidate}" || continue
		[ -b "${candidate}" ] || continue
		if mount -t squashfs -o ro "${candidate}" /mnt/probe 2>/dev/null; then
			umount /mnt/probe 2>/dev/null || true
			ROOT_SLOT="$(slot_for_root_dev "${candidate}")"
			echo "${candidate}"
			return 0
		fi
	done

	return 1
}

resolve_requested_root_slot() {
	if [ "${ROOT_DEV}" = "/dev/helios-rootfs" ]; then
		read_active_slot_marker || printf '%s\n' "${SLOT_A_NAME}"
		return 0
	fi
	slot_for_root_dev "${ROOT_DEV}"
}

resolve_base_disk() {
	local candidate

	case "${ROOT_DEV}" in
		/dev/helios-rootfs) ;;
		/dev/*)
			partition_base "${ROOT_DEV}"
			return 0
			;;
	esac

	for candidate in "${BOOT_DEV}" ${BOOT_CANDIDATES} ${DATA_CANDIDATES}; do
		[ -n "${candidate}" ] || continue
		wait_for_block "${candidate}" || continue
		[ -b "${candidate}" ] || continue
		partition_base "${candidate}"
		return 0
	done

	return 1
}

root_device_for_slot() {
	local slot="$1"
	local base

	base="$(resolve_base_disk)" || return 1
	case "${slot}" in
		"${SLOT_A_NAME}")
			partition_dev "${base}" "${SLOT_A_PARTITION}"
			;;
		"${SLOT_B_NAME}")
			partition_dev "${base}" "${SLOT_B_PARTITION}"
			;;
		*)
			return 1
			;;
	esac
}

read_active_slot_marker() {
	local candidate mountpoint active

	mountpoint="/mnt/slot-marker"
	mkdir -p "${mountpoint}"

	for candidate in ${DATA_CANDIDATES}; do
		[ -n "${candidate}" ] || continue
		wait_for_block "${candidate}" || continue
		if mount -t ext4 -o ro "${candidate}" "${mountpoint}" 2>/dev/null; then
			active="$(cat "${mountpoint}/ota/active" 2>/dev/null || true)"
			umount "${mountpoint}" 2>/dev/null || true
			if [ "${active}" = "${SLOT_A_NAME}" ] || [ "${active}" = "${SLOT_B_NAME}" ]; then
				printf '%s\n' "${active}"
				return 0
			fi
		fi
	done

	for candidate in "${BOOT_DEV}" ${BOOT_CANDIDATES}; do
		[ -n "${candidate}" ] || continue
		wait_for_block "${candidate}" || continue
		if mount -t vfat -o ro "${candidate}" "${mountpoint}" 2>/dev/null; then
			active="$(cat "${mountpoint}/helios/ota/active" 2>/dev/null || true)"
			umount "${mountpoint}" 2>/dev/null || true
			if [ "${active}" = "${SLOT_A_NAME}" ] || [ "${active}" = "${SLOT_B_NAME}" ]; then
				printf '%s\n' "${active}"
				return 0
			fi
		fi
	done

	return 1
}

derive_related_devices() {
	local base

	base="$(partition_base "${ROOT_DEV}")"
	[ -n "${base}" ] || return 1

	BOOT_DEV="$(partition_dev "${base}" "${BOOT_PARTITION}")"
	if [ "${DATA_DEV_EXPLICIT}" -ne 1 ]; then
		DATA_DEV="$(partition_dev "${base}" "${DATA_PARTITION}")"
	fi
	return 0
}

set_overlay_slot() {
	case "${ROOT_SLOT}" in
		"${SLOT_B_NAME}")
			OVERLAY_SLOT="root-b"
			;;
		*)
			OVERLAY_SLOT="root-a"
			;;
	esac
}

slot_for_root_dev() {
	local root_dev="$1"
	local slot_a_dev
	local slot_b_dev

	slot_a_dev="$(root_device_for_slot "${SLOT_A_NAME}" 2>/dev/null || true)"
	slot_b_dev="$(root_device_for_slot "${SLOT_B_NAME}" 2>/dev/null || true)"
	if [ -n "${slot_b_dev}" ] && [ "${root_dev}" = "${slot_b_dev}" ]; then
		printf '%s\n' "${SLOT_B_NAME}"
		return 0
	fi
	if [ -n "${slot_a_dev}" ] && [ "${root_dev}" = "${slot_a_dev}" ]; then
		printf '%s\n' "${SLOT_A_NAME}"
		return 0
	fi
	return 1
}

ensure_data_dev() {
	local candidate

	if [ "${DATA_DEV_EXPLICIT}" -eq 1 ]; then
		wait_for_block "${DATA_DEV}" || return 1
		return 0
	fi

	[ -b "${DATA_DEV}" ] && return 0
	for candidate in ${DATA_CANDIDATES}; do
		[ -b "${candidate}" ] || continue
		DATA_DEV="${candidate}"
		return 0
	done

	return 1
}

for arg in $(cat /proc/cmdline); do
	case "${arg}" in
		root=*)
			ROOT_DEV="${arg#root=}"
			;;
		helios_data=*)
			DATA_DEV="${arg#helios_data=}"
			DATA_DEV_EXPLICIT=1
			;;
		helios_squashfs_debug=1)
			BOOT_DEBUG=1
			;;
	esac
done

mount -t devtmpfs devtmpfs /dev 2>/dev/null || true
mount -t proc proc /proc 2>/dev/null || true
mount -t sysfs sysfs /sys 2>/dev/null || true
mark "10-dev-proc-sys-mounted"

ROOT_DEV="$(resolve_root_dev)" || panic_shell "root device not found"
ROOT_SLOT="$(slot_for_root_dev "${ROOT_DEV}")"
derive_related_devices || panic_shell "failed to derive related devices"
set_overlay_slot
record_selection
mark "20-root-device-found"

mount -t squashfs -o ro "${ROOT_DEV}" /mnt/lower || panic_shell "failed to mount lower squashfs"
mark "30-lower-mounted"

if offline_data_borrow_requested; then
	FORCE_TMPFS_WRITABLE=1
	mark "35-repartition-request-detected"
fi

if [ "${FORCE_TMPFS_WRITABLE}" -eq 1 ]; then
	mount -t tmpfs -o mode=0755 tmpfs /mnt/data || panic_shell "failed to mount tmpfs writable store"
elif ensure_data_dev && [ -b "${DATA_DEV}" ]; then
	if mount -t ext4 -o rw,noatime "${DATA_DEV}" /mnt/data; then
		if [ ! -f "${DATA_MARKER}" ]; then
			umount /mnt/data 2>/dev/null || true
			mount -t tmpfs -o mode=0755 tmpfs /mnt/data || \
				panic_shell "failed to mount tmpfs fallback writable store"
		fi
	else
		mount -t tmpfs -o mode=0755 tmpfs /mnt/data || \
			panic_shell "failed to mount writable store"
	fi
else
	mount -t tmpfs -o mode=0755 tmpfs /mnt/data || panic_shell "failed to mount tmpfs writable store"
fi
mark "40-data-mounted"
record_selection

mkdir -p "/mnt/data/root-overlay/${OVERLAY_SLOT}/upper" "/mnt/data/root-overlay/${OVERLAY_SLOT}/work"
mkdir -p /newroot
mount -t overlay overlay \
	-o lowerdir=/mnt/lower,upperdir=/mnt/data/root-overlay/${OVERLAY_SLOT}/upper,workdir=/mnt/data/root-overlay/${OVERLAY_SLOT}/work \
	/newroot || panic_shell "failed to mount overlay root"
mark "50-overlay-mounted"

mkdir -p /newroot/proc /newroot/sys /newroot/dev /newroot/run
mkdir -p /newroot/var/lib/helios /newroot/.overlay-data

mount --move /proc /newroot/proc || panic_shell "failed to move /proc"
mount --move /sys /newroot/sys || panic_shell "failed to move /sys"
mount --move /dev /newroot/dev || panic_shell "failed to move /dev"
mount --move /mnt/data /newroot/.overlay-data || panic_shell "failed to move writable store"
mount --bind /newroot/.overlay-data /newroot/var/lib/helios || panic_shell "failed to bind writable store"
mark "60-ready-for-switch-root"

exec switch_root /newroot /sbin/init || panic_shell "switch_root failed"
EOF

	chmod 0755 "${init_root}/init"

	(
		cd "${init_root}"
		find . -print0 | cpio --null -o -H newc --quiet | gzip -n -9 > "${out}"
	)
}

build_boot_kernel_aliases() {
	local src="${BINARIES_DIR}/${KERNEL_IMAGE_NAME}"
	local dst

	[ -f "${src}" ] || { echo "missing kernel image: ${src}" >&2; exit 1; }

	while IFS= read -r dst; do
		[ -n "${dst}" ] || continue
		gzip -n -c -9 "${src}" > "${BINARIES_DIR}/${dst}"
	done < <(boot_kernel_aliases)
}

install_initramfs_aliases() {
	local src="${BINARIES_DIR}/${INITRAMFS_NAME}"
	local alias

	[ -f "${src}" ] || { echo "missing initramfs: ${src}" >&2; exit 1; }

	while IFS= read -r alias; do
		[ -n "${alias}" ] || continue
		install -m 0644 "${src}" "${BINARIES_DIR}/${alias}"
	done < <(boot_initramfs_aliases)
}

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
rsync -a --ignore-existing "${BINARIES_DIR}/rpi-firmware/" "${BINARIES_DIR}/"
rm -rf "${BINARIES_DIR}/overlays"
mkdir -p "${BINARIES_DIR}/overlays"
if [ -d "${BINARIES_DIR}/rpi-firmware/overlays" ]; then
		for overlay in \
			bcm2712d0.dtbo \
			disable-wifi.dtbo \
			disable-wifi-pi5.dtbo \
			disable-bt.dtbo \
		disable-bt-pi5.dtbo \
		dwc2.dtbo \
		i2c1-pi5.dtbo \
		i2c-gpio.dtbo \
		ws2812-pio.dtbo \
		vc4-kms-v3d-pi5.dtbo \
		overlay_map.dtb \
		hat_map.dtb
	do
		if [ -f "${BINARIES_DIR}/rpi-firmware/overlays/${overlay}" ]; then
			install -m 0644 "${BINARIES_DIR}/rpi-firmware/overlays/${overlay}" "${BINARIES_DIR}/overlays/${overlay}"
		fi
	done
fi

install -m 0644 "${BOARD_DIR}/config_cm5io.txt" "${BINARIES_DIR}/config.txt"
install -m 0644 "${BOARD_DIR}/cmdline.txt" "${BINARIES_DIR}/cmdline.txt"

# Keep our CM5 DTBs authoritative, but add the upstream bcm2712 aliases used by
# older EEPROM revisions before full CM5 carrier auto-detection is available.
for dtb in "${BINARIES_DIR}"/rpi-firmware/bcm2712*.dtb; do
	[ -f "${dtb}" ] || continue
	dtb_name="$(basename "${dtb}")"
	if [ ! -f "${BINARIES_DIR}/${dtb_name}" ]; then
		install -m 0644 "${dtb}" "${BINARIES_DIR}/${dtb_name}"
	fi
done

if [ -n "${REPO_ROOT}" ]; then
	DT_OVERLAY_SRC="${REPO_ROOT}/assets/buildroot/dt-overlays"
	if [ -d "${DT_OVERLAY_SRC}" ]; then
		DTC="${HOST_DIR}/bin/dtc"
		if [ ! -x "${DTC}" ]; then
			DTC="$(command -v dtc || true)"
		fi
		[ -n "${DTC}" ] || { echo "dtc not found" >&2; exit 1; }

		mkdir -p "${BINARIES_DIR}/rpi-firmware/overlays" "${BINARIES_DIR}/overlays"
		for dts in "${DT_OVERLAY_SRC}"/*.dts; do
			[ -f "${dts}" ] || continue
			name="$(basename "${dts}" .dts)"
			"${DTC}" -@ -I dts -O dtb -o "${BINARIES_DIR}/rpi-firmware/overlays/${name}.dtbo" "${dts}"
			install -m 0644 "${BINARIES_DIR}/rpi-firmware/overlays/${name}.dtbo" "${BINARIES_DIR}/overlays/${name}.dtbo"
		done
	fi
fi

build_initramfs
install_initramfs_aliases
build_boot_kernel_aliases

KERNEL_FROM_CONFIG="$(sed -n 's/^kernel=//p' "${BINARIES_DIR}/config.txt" | head -n 1)"

	if [ ! -e "${GENIMAGE_CFG}" ]; then
		GENIMAGE_CFG="${BINARIES_DIR}/genimage.cfg"
		BOOT_FILES_LIST="${BINARIES_DIR}/genimage.boot-files"
		FILES=()

	for f in "${BINARIES_DIR}"/bcm2712*.dtb; do
		[ -e "${f}" ] && FILES+=( "$(basename "${f}")" )
	done
	for f in start4.elf fixup4.dat config.txt cmdline.txt; do
		[ -e "${BINARIES_DIR}/${f}" ] && FILES+=( "${f}" )
	done
	while IFS= read -r f; do
		[ -n "${f}" ] || continue
		[ -e "${BINARIES_DIR}/${f}" ] && FILES+=( "${f}" )
	done < <(boot_kernel_aliases)
	while IFS= read -r f; do
		[ -n "${f}" ] || continue
		[ -e "${BINARIES_DIR}/${f}" ] && FILES+=( "${f}" )
	done < <(boot_initramfs_aliases)
	[ -d "${BINARIES_DIR}/overlays" ] && FILES+=( "overlays" )

	if [ -f "${BASE_BOARD_DIR}/os_config.json" ]; then
		install -m 0644 "${BASE_BOARD_DIR}/os_config.json" "${BINARIES_DIR}/os_config.json"
		case " ${FILES[*]} " in
			*" os_config.json "*) ;;
			*) FILES+=( "os_config.json" ) ;;
		esac
	fi

		printf '\t\t\t"%s",\n' "${FILES[@]}" > "${BOOT_FILES_LIST}"
		sed "/#BOOT_FILES#/r ${BOOT_FILES_LIST}" "${BOARD_DIR}/genimage.cfg.in" | \
			sed "/#BOOT_FILES#/d" > "${GENIMAGE_CFG}"
	fi

trap 'rm -rf "${ROOTPATH_TMP}"' EXIT
ROOTPATH_TMP="$(mktemp -d)"

rm -rf "${GENIMAGE_TMP}"

genimage \
	--rootpath "${ROOTPATH_TMP}" \
	--tmppath "${GENIMAGE_TMP}" \
	--inputpath "${BINARIES_DIR}" \
	--outputpath "${BINARIES_DIR}" \
	--config "${GENIMAGE_CFG}"

if [ -f "${SQUASHFS_LAYOUT_MANIFEST}" ]; then
	python3 "${REPO_ROOT}/tools/storage-layout/render_layout_assets.py" \
		validate-image \
		--layout "${SQUASHFS_LAYOUT_MANIFEST}" \
		--image "${BINARIES_DIR}/sdcard.img"
fi

exit $?
