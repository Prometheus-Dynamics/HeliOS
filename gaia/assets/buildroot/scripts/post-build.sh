#!/bin/sh
# Buildroot post-build script (BR2_ROOTFS_POST_BUILD_SCRIPT), run on the target
# tree before the read-only EROFS root is packed. Gaia's own post-build script
# (the image feed) runs after this one.
#
# Everything here exists so that nothing has to be created in / at runtime:
# the root filesystem is a read-only EROFS slot, and what HeliOS writes lives
# on /data (p7) or in /run (gaia/configs/README.md, "Read-only root").
set -eu

target=${1:-$TARGET_DIR}

# The root is mounted read-only from the kernel command line and has no fsck
# (EROFS); keep systemd from trying to check or remount it.
if [ -f "$target/etc/fstab" ]; then
	sed -i 's#^/dev/root[[:space:]].*#/dev/root / auto ro 0 0#' "$target/etc/fstab"
fi

# Mount points: /data (p7) is mounted by helios-data-setup.service.
mkdir -p "$target/data"

# systemd generates a transient machine ID over this empty file at boot, and
# helios-data-setup binds the one kept on /data over it.
[ -e "$target/etc/machine-id" ] || : >"$target/etc/machine-id"

# Files the system writes at runtime, as links into /run:
#   /etc/issue                  login banner (helios-update-issue.service)
#   /etc/board/manage-url       the device identity's manage_url
#                               (helios-manage-url.service; it holds the
#                               per-board hostname)
mkdir -p "$target/etc/board"
ln -sfn ../run/helios/issue "$target/etc/issue"
ln -sfn ../../run/helios/manage-url "$target/etc/board/manage-url"

# The board package's commit (BOARD_PACKAGE_COMMIT) comes from the image feed's
# /etc/default/board-package.env (configs/identity/base.toml); the package
# reads it as /etc/board/board-package.env.
ln -sfn ../default/board-package.env "$target/etc/board/board-package.env"
