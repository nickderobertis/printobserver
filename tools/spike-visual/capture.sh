#!/bin/sh
# SPIKE (spike-visual): re-capture all ten screenshots.
#   before-*.png: the `printobserver` command built from BASE (unchanged base)
#   after-*.png:  the command built from this branch's mock
# Both against tools/spike-visual/stub.py, in the same 160-column terminal.
# Usage: tools/spike-visual/capture.sh [OUT_DIR]   (BASE defaults to 75a7ca4)
set -eu
ROOT=$(git rev-parse --show-toplevel)
BASE=${BASE:-75a7ca4}
OUT=$(mkdir -p "${1:-$ROOT/tools/spike-visual/shots}" && cd "${1:-$ROOT/tools/spike-visual/shots}" && pwd)
WORK=$(mktemp -d)
cd "$ROOT"

git worktree add --detach "$WORK/base" "$BASE" >/dev/null
(cd "$WORK/base" && CARGO_TARGET_DIR="$ROOT/target/spike-base" cargo build -q -p printobserver)
git worktree remove --force "$WORK/base"
cargo build -q -p printobserver
mkdir -p "$WORK/before" "$WORK/after"
cp "$ROOT/target/spike-base/debug/printobserver" "$WORK/before/printobserver"
cp "$ROOT/target/debug/printobserver" "$WORK/after/printobserver"

PORT=${PORT:-18765}
export PRINTOBSERVER_SERVER="127.0.0.1:$PORT" PRINTOBSERVER_CREDENTIAL=spike-credential
PRINT=0199d4c2-7a10-7c3e-9f4b-2d1e6a8b0c11
FILE=benchy_0.2mm_PLA.gcode
MANIFEST='{"file_name":"'$FILE'","material":"PLA","nozzle_diameter_mm":0.4,"slicer_profile":"draft","allowed":{},"metadata":{}}'

shoot() { # side name command...
  side=$1; name=$2; shift 2
  STUB_MODE=$side /usr/bin/python3 "$ROOT/tools/spike-visual/stub.py" "$PORT" & STUB=$!
  sleep 1
  PATH="$WORK/$side:$PATH" freeze --execute "$ROOT/tools/spike-visual/show.sh $*" \
    --wrap 160 --width 1500 --font.size 14 --window --output "$OUT/$side-$name.png" >/dev/null
  kill "$STUB"; wait "$STUB" 2>/dev/null || true
}

for side in before after; do
  shoot "$side" help printobserver --help
  shoot "$side" spools printobserver spools
  shoot "$side" printer-history printobserver printer-history
  shoot "$side" start-print printobserver start-print --print-id "$PRINT" --actor operator \
    --file-name "$FILE" --manifest "'$MANIFEST'" --reason "'printing a benchy'"
  shoot "$side" preflight printobserver preflight --file-name "$FILE"
done
rm -rf "$WORK"
ls "$OUT"
