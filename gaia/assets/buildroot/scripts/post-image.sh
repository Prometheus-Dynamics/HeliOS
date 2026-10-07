#!/bin/sh
# Buildroot post-image script (BR2_ROOTFS_POST_IMAGE_SCRIPT): the parts of the
# A/B disk that Buildroot does not make. Gaia's assembly (targets/raze.toml)
# then builds sdcard.img from them.
#
#   rootfs.erofs  checked against the root slot size: p5/p6 are fixed at
#                 ROOT_SLOT_MIB, and the device package's writer refuses an
#                 image whose root does not fit the board's slot.
#   data.ext4     an empty /data for p7 (label "data"), grown to fill the
#                 eMMC on first boot by helios-data-setup.
set -eu

ROOT_SLOT_MIB=512
DATA_IMAGE_MIB=64

images=${BINARIES_DIR:-$1}
PATH="${HOST_DIR:+$HOST_DIR/sbin:$HOST_DIR/bin:}$PATH"

log() { echo "post-image: $*"; }
die() {
	echo "post-image: error: $*" >&2
	exit 1
}

root="$images/rootfs.erofs"
[ -f "$root" ] || die "$root is missing (BR2_TARGET_ROOTFS_EROFS)"
root_bytes=$(stat -c %s "$root")
root_mib=$(((root_bytes + 1048575) / 1048576))
[ "$root_mib" -le "$ROOT_SLOT_MIB" ] ||
	die "rootfs.erofs is ${root_mib} MiB and does not fit a ${ROOT_SLOT_MIB} MiB root slot"
log "rootfs.erofs is ${root_mib} MiB of the ${ROOT_SLOT_MIB} MiB root slot"

data="$images/data.ext4"
rm -f "$data"
truncate -s "${DATA_IMAGE_MIB}M" "$data"
mkfs.ext4 -q -F -b 4096 -O ^64bit -L data "$data"
log "data.ext4 is ${DATA_IMAGE_MIB} MiB (grown to fill the eMMC on first boot)"
