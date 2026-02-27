#!/bin/sh
set -eu

DATA_DIR="${HELIOS_API_DATA_DIR:-/var/lib/helios/api-data}"
MEDIA_DIR="${HELIOS_API_MEDIA_DIR:-$DATA_DIR/media}"
MEDIA_SEED_DIR="${HELIOS_API_MEDIA_SEED_DIR:-/usr/share/helios/media}"
MARKER="${HELIOS_STARTUP_PRESET_MARKER:-$DATA_DIR/.startup-preset-applied-v1.json}"
CORRUPT=0

remove_zero_json_files() {
    dir="$1"
    [ -d "$dir" ] || return 0

    for file in "$dir"/*.json; do
        [ -e "$file" ] || continue
        if [ ! -s "$file" ]; then
            echo "[helios-api] removing zero-byte persisted file: $file" >&2
            rm -f "$file"
            CORRUPT=1
        fi
    done
}

if [ -f "$MARKER" ] && [ ! -s "$MARKER" ]; then
    echo "[helios-api] removing zero-byte startup marker: $MARKER" >&2
    rm -f "$MARKER"
    CORRUPT=1
fi

remove_zero_json_files "$DATA_DIR/pipelines"
remove_zero_json_files "$DATA_DIR/streams"

if [ "$CORRUPT" -eq 1 ]; then
    # Allow startup.toml to re-seed after pruning corrupt state.
    rm -f "$MARKER"
fi

seed_fmap() {
    src="$1"
    dst="$2"

    [ -s "$src" ] || return 0
    install -d -m0755 "$MEDIA_DIR"
    if [ ! -s "$dst" ]; then
        echo "[helios-api] seeding media asset: $dst" >&2
        install -m0644 "$src" "$dst"
    fi
}

seed_fmap \
    "$MEDIA_SEED_DIR/FRC2026_ANDYMARK.fmap" \
    "$MEDIA_DIR/FRC2026_ANDYMARK.fmap"

exit 0
