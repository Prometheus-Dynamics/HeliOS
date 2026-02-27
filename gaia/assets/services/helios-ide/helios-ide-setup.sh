#!/bin/sh
set -eu

ENV_FILE="/etc/default/helios-ide"
if [ -f "$ENV_FILE" ]; then
  # shellcheck disable=SC1090
  . "$ENV_FILE"
fi

# Ensure basic utilities (mkdir/chown) are resolvable even if PATH is minimal.
PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin${PATH:+:$PATH}

IDE_SERVICE_USER="${IDE_SERVICE_USER:-pi}"
IDE_THEME="${IDE_THEME:-Default Dark Modern}"

for dir in "$IDE_ROOT" "$IDE_WORKSPACE_DIR" "$IDE_PROJECTS_DIR" "$IDE_ARTIFACTS_DIR" "$IDE_TOOLS_DIR" "$IDE_SDK_DIR" "$IDE_DATA_DIR" "$IDE_EXTENSIONS_DIR" "$IDE_SERVER_DATA_DIR"; do
  if [ -n "$dir" ]; then
    mkdir -p "$dir"
    chown "$IDE_SERVICE_USER:$IDE_SERVICE_USER" "$dir"
  fi
done

if [ -n "${IDE_DATA_DIR:-}" ]; then
  settings_dir="$IDE_DATA_DIR/User"
  settings_path="$settings_dir/settings.json"
  if [ ! -f "$settings_path" ]; then
    mkdir -p "$settings_dir"
    chown "$IDE_SERVICE_USER:$IDE_SERVICE_USER" "$settings_dir"
    cat >"$settings_path" <<EOF
{
  "workbench.colorTheme": "$IDE_THEME",
  "workbench.preferredDarkColorTheme": "$IDE_THEME",
  "github.copilot.enable": false,
  "github.copilot.inlineSuggest.enable": false
}
EOF
    chown "$IDE_SERVICE_USER:$IDE_SERVICE_USER" "$settings_path"
  fi
fi
