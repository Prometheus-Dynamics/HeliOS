#!/bin/sh
set -eu

size="${HELIOS_JOURNAL_MAX_USE:-16M}"
files="${HELIOS_JOURNAL_MAX_FILES:-4}"

if ! command -v journalctl >/dev/null 2>&1; then
  exit 0
fi

journalctl --rotate >/dev/null 2>&1 || true
journalctl --vacuum-size="$size" --vacuum-files="$files" >/dev/null 2>&1 || true
