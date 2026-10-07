#!/bin/sh
# Login banner: OS version and IP addresses. /etc/issue is a link to
# /run/helios/issue (the root filesystem is read-only).
set -u

ISSUE=/run/helios/issue
mkdir -p /run/helios

NAME=$(sed -n 's/^NAME=\"\{0,1\}\(.*\)\"\{0,1\}$/\1/p' /etc/os-release | head -n1)
VER=$(sed -n 's/^VERSION_ID=\"\{0,1\}\(.*\)\"\{0,1\}$/\1/p' /etc/os-release | head -n1)

{
  echo "${NAME} OS - ${VER} ($(hostname))"
  if command -v ip >/dev/null 2>&1; then
    i=1
    ip -o -4 addr show up scope global 2>/dev/null | while read -r _ IF _ IP _; do
      echo "IP${i}: ${IP%/*} (${IF})"
      i=$((i + 1))
    done
  fi
} > "$ISSUE.tmp"
mv -f "$ISSUE.tmp" "$ISSUE"

exit 0
