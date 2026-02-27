#!/bin/sh

set -eu

BOARD_DIR="$(dirname "$0")"

resolve_repo_root() {
    candidate="${HELIOS_REPO_ROOT:-}"
    if [ -n "${candidate}" ] && [ -d "${candidate}/assets/buildroot" ]; then
        printf "%s\n" "${candidate}"
        return 0
    fi

    for rel in ../../../.. ../../../../.. ../../../../../..; do
        candidate="$(realpath -m "${BOARD_DIR}/${rel}")"
        if [ -d "${candidate}/assets/buildroot" ]; then
            printf "%s\n" "${candidate}"
            return 0
        fi
    done
    return 1
}

REPO_ROOT="$(resolve_repo_root || true)"

# Keep upstream board behavior: ensure a local tty1 console is available.
if [ -e "${TARGET_DIR}/etc/inittab" ]; then
    grep -qE '^tty1::' "${TARGET_DIR}/etc/inittab" || \
	sed -i '/GENERIC_SERIAL/a\
tty1::respawn:/sbin/getty -L  tty1 0 vt100 # HDMI console' "${TARGET_DIR}/etc/inittab"
elif [ -d "${TARGET_DIR}/etc/systemd" ]; then
    mkdir -p "${TARGET_DIR}/etc/systemd/system/getty.target.wants"
    ln -sf /lib/systemd/system/getty@.service \
       "${TARGET_DIR}/etc/systemd/system/getty.target.wants/getty@tty1.service"
fi

PRUNE_SCRIPT="${REPO_ROOT}/assets/buildroot/post-build/prune-rootfs.sh"
if [ -n "${REPO_ROOT}" ] && [ -x "${PRUNE_SCRIPT}" ]; then
    "${PRUNE_SCRIPT}"
else
    echo "post-build: prune-rootfs.sh not found/executable (repo_root='${REPO_ROOT}'); skipping prune" >&2
fi
