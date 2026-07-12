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
DATA_LAYOUT_MOUNT="${HELIOS_LAYOUT_DATA_MOUNT_POINT:-/run/helios-provision/data}"
DATA_MARKER_SOURCE="${HELIOS_LAYOUT_DATA_SECONDARY_MARKER:-/mnt/data/.provision.data_v1}"
case "${DATA_MARKER_SOURCE}" in
	/mnt/data|/mnt/data/*)
		DATA_MARKER="${DATA_MARKER_SOURCE}"
		;;
	"${DATA_LAYOUT_MOUNT}"/*)
		DATA_MARKER="/mnt/data${DATA_MARKER_SOURCE#"${DATA_LAYOUT_MOUNT}"}"
		;;
	/*)
		DATA_MARKER="/mnt/data${DATA_MARKER_SOURCE}"
		;;
	*)
		DATA_MARKER="/mnt/data/${DATA_MARKER_SOURCE}"
		;;
esac
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
		/dev/helios-rootfs)
			;;
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

mkdir -p /dev /proc /sys /mnt/data /mnt/lower /newroot

mount -t devtmpfs devtmpfs /dev 2>/dev/null || true
mount -t proc proc /proc 2>/dev/null || true
mount -t sysfs sysfs /sys 2>/dev/null || true
mark "10-dev-proc-sys-mounted"

/bin/insmod /lib/modules/squashfs.ko 2>/dev/null || true
/bin/insmod /lib/modules/overlay.ko 2>/dev/null || true
mark "15-fs-modules-loaded"

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
