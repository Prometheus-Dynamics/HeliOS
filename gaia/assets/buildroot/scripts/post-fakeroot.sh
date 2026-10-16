#!/bin/sh
# Buildroot post-fakeroot script (BR2_ROOTFS_POST_FAKEROOT_SCRIPT): runs on the
# target tree right before the EROFS root is packed, after Gaia's image feed has
# staged /etc/default/slot.env (the device package's slot identity, full
# profile only). Writes /etc/slot.json: which slot this root is. No verity
# values: those are in the bundle's slot.json, outside the verity image.
set -eu

target=${1:-$TARGET_DIR}
env="$target/etc/default/slot.env"
[ -r "$env" ] || exit 0

json_escape() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }
(
	# shellcheck disable=SC1090
	. "$env"
	dirs=$(printf '%s' "${SLOT_DATA_DIRS:-}" | tr ',' '\n' | sed '/^$/d; s/.*/"&"/' | paste -sd, -)
	cat >"$target/etc/slot.json" <<JSON
{"schema": ${SLOT_SCHEMA:-1}, "id": "$(json_escape "${SLOT_ID:-}")", "family": "$(json_escape "${SLOT_FAMILY:-}")", "version": "$(json_escape "${SLOT_VERSION:-}")", "variant": "$(json_escape "${SLOT_VARIANT:-}")", "kernel": "$(json_escape "${SLOT_KERNEL:-}")", "built": "$(json_escape "${SLOT_BUILT:-}")", "source": "$(json_escape "${SLOT_SOURCE:-}")", "data_dirs": [${dirs}]}
JSON
)
