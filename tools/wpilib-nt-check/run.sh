#!/usr/bin/env bash
# Builds NtCheck, a WPILib robot program that only reads HeliOS's NetworkTables topics, and runs it
# on a SystemCore. It is also the board's NT server, as any robot program is.
#
#   tools/wpilib-nt-check/run.sh deploy    build, copy, and start it (robot.service)
#   tools/wpilib-nt-check/run.sh logs      follow what it logs
#   tools/wpilib-nt-check/run.sh stop      stop it and remove it from the board
#
# robot.service runs /home/systemcore/robotCommand at every boot while that file exists, so a
# deployed check stays until `stop`. SYSTEMCORE overrides the board (default systemcore@172.27.0.1).
set -euo pipefail

VERSION=2027.0.0-alpha-8
MAVEN=https://frcmaven.wpi.edu/artifactory/release/org/wpilib
BOARD=${SYSTEMCORE:-systemcore@172.27.0.1}
REMOTE=/home/systemcore/nt-check
HERE=$(cd "$(dirname "$0")" && pwd)
CACHE=${XDG_CACHE_HOME:-$HOME/.cache}/helios/wpilib-$VERSION
OUT=$CACHE/out
MODULES=(wpilibj hal ntcore wpiutil wpimath wpinet datalog wpiunits telemetry tunables)
NATIVES=(hal ntcore wpiutil wpimath wpinet datalog)
# What this WPILib release's module-info files require, at its gradle/libs.versions.toml versions.
CENTRAL=https://repo1.maven.org/maven2
THIRD_PARTY=(
  io/avaje/avaje-jsonb/3.14/avaje-jsonb-3.14.jar
  io/avaje/avaje-json-core/3.14/avaje-json-core-3.14.jar
  us/hebi/quickbuf/quickbuf-runtime/1.4/quickbuf-runtime-1.4.jar
)

fetch() {
  [ -s "$2" ] || curl -fsSL --retry 3 -o "$2.part" "$1"
  [ -s "$2" ] || mv "$2.part" "$2"
}

build() {
  mkdir -p "$CACHE/lib" "$OUT/lib" "$OUT/classes"
  for module in "${MODULES[@]}"; do
    fetch "$MAVEN/$module/$module-java/$VERSION/$module-java-$VERSION.jar" "$CACHE/lib/$module.jar"
    cp "$CACHE/lib/$module.jar" "$OUT/lib/"
  done
  for path in "${THIRD_PARTY[@]}"; do
    fetch "$CENTRAL/$path" "$CACHE/lib/${path##*/}"
    cp "$CACHE/lib/${path##*/}" "$OUT/lib/"
  done
  for module in "${NATIVES[@]}"; do
    zip=$CACHE/$module-native.zip
    fetch "$MAVEN/$module/$module-cpp/$VERSION/$module-cpp-$VERSION-linuxsystemcore.zip" "$zip"
    unzip -qoj "$zip" 'linux/systemcore/shared/*.so' -d "$OUT/lib"
  done
  javac -d "$OUT/classes" -cp "$OUT/lib/*" "$HERE/NtCheck.java"
}

case ${1:-} in
  deploy)
    build
    ssh "$BOARD" "mkdir -p $REMOTE/lib"
    scp -q "$OUT"/lib/* "$BOARD:$REMOTE/lib/"
    scp -qr "$OUT/classes" "$BOARD:$REMOTE/"
    ssh "$BOARD" "printf '%s\n' 'cd $REMOTE && LD_LIBRARY_PATH=$REMOTE/lib exec java -Djava.library.path=$REMOTE/lib -cp \"classes:lib/*\" NtCheck' > /home/systemcore/robotCommand && sudo -n systemctl restart robot"
    ;;
  logs)
    ssh "$BOARD" "journalctl -u robot -f -n 20 --no-pager"
    ;;
  stop)
    ssh "$BOARD" "rm -f /home/systemcore/robotCommand; sudo -n systemctl stop robot; rm -rf $REMOTE"
    ;;
  *)
    sed -n 2,9p "$0"
    exit 2
    ;;
esac
