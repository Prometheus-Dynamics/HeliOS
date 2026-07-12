#!/bin/sh
set -u

# Read OS info
NAME=$(sed -n 's/^NAME=\"\{0,1\}\(.*\)\"\{0,1\}$/\1/p' /etc/os-release | head -n1)
VER=$(sed -n 's/^VERSION_ID=\"\{0,1\}\(.*\)\"\{0,1\}$/\1/p' /etc/os-release | head -n1)

# Build banner (always write header)
echo "${NAME} OS - ${VER}" > /etc/issue

# Append IPv4 addresses if 'ip' is available
if command -v ip >/dev/null 2>&1; then
  i=1
  ip -o -4 addr show up scope global 2>/dev/null | while read _ IF _ IP _; do
    IP=${IP%/*}
    echo "IP${i}: ${IP} (${IF})" >> /etc/issue
    i=$((i+1))
  done
fi

exit 0
