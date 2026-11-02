#!/bin/sh
# Buildroot post-image script (BR2_ROOTFS_POST_IMAGE_SCRIPT). Nothing to do: the Raze
# image is the dynamic slots, which Gaia assembles from rootfs.erofs (targets/raze.toml,
# Atlas slots.toml). This hook replaces Buildroot's default board post-image, which runs
# genimage for the removed A/B disk and would fail on the slot build.
set -eu
