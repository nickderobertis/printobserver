#!/usr/bin/env bash
# SPIKE MOCK (throwaway): re-capture every screenshot of the visual spike.
#
#   SPIKE_FRAMES_DIR=<the 26 Nov capture folder> tools/spike-visual/capture.sh [out-dir]
#
# "before" shots run the CLI built from the merge base with main (the unchanged
# base); "after" shots run this branch's CLI, which only adds the planned
# `recheck-camera` declaration. Both talk to stub_server.py's canned answers.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
out=${1:-${TMPDIR:-/tmp}/printobserver-spike-visual}
frames=${SPIKE_FRAMES_DIR:?name the 26 Nov capture folder in SPIKE_FRAMES_DIR}
work=$(mktemp -d)
mkdir -p "$work/home"
state=/tmp/printobserver-mock/state
print=0193688a-5c00-7a1e-8b0c-5e1f0a2b3c4d
port=18471
mkdir -p "$out" "$work/before" "$work/after" "$state/check-images" "$state/images/5e"
stub=""
cleanup() { [ -n "$stub" ] && kill "$stub" 2>/dev/null; git -C "$repo" worktree remove --force "$work/base" 2>/dev/null || true; rm -rf "$work" /tmp/printobserver-mock; }
trap cleanup EXIT
zeros=$(printf '0%.0s' $(seq 56))
touch "$state/check-images/5e-5e1f${zeros}1212-1280x720.jpg" "$state/images/5e/5e1f${zeros}1931"

# The two CLIs and the composer.
git -C "$repo" worktree add --detach "$work/base" "$(git -C "$repo" merge-base HEAD origin/main)" >/dev/null
(cd "$work/base" && cargo build -q -p printobserver --bin printobserver --target-dir "$repo/target/spike-base")
cp "$repo/target/spike-base/debug/printobserver" "$work/before/printobserver"
(cd "$repo" && touch crates/printobserver-server/src/operations.rs && cargo build -q -p printobserver --bin printobserver \
  && cargo build -q -p printobserver-server --features test-responder --bin printobserver-server-compose)
ln -s "$repo/target/debug/printobserver" "$work/after/printobserver"

pillow() { uv run -q --with pillow python -I "$here/cards.py" "$@"; }

# 1. The 13:14 check image.
whole_note="the whole camera frame, scaled to fit 1280x720 and uncropped, because the print's first-layer alignment failed: the printed first layer lies 59.7 px from where the calibration projects it"
pillow card "$frames/agent-received/131416-turn-image1.jpg" "$whole_note" "$out/check-1314-before.png"
note=$(python3 -I "$here/check_image.py" "$repo/target/debug/printobserver-server-compose" "$frames" "$work/1314.jpg" -15 30 75 0 \
  | python3 -I -c 'import json,sys; print(json.load(sys.stdin)["note"])')
note=${note/the camera does not shift in regular operation and the drawing already accounts for the bed\'s travel, so /the drawing was re-fit to this frame, the camera having shifted 75 px since it was calibrated, and accounts for the bed\'s travel, so }
pillow card "$work/1314.jpg" "$note" "$out/check-1314-after.png"
pillow pad "$out/check-1314-before.png" "$out/check-1314-after.png"

# 2-5. The operator's commands, each against the stub in its mode.
shot() { # mode name shell-command-line
  local mode=$1 name=$2 line=$3
  printf 'echo %q\n%s\n' "\$ $line" "$line" > "$work/$name.sh"
  PATH="$work/$mode:$PATH" PRINTOBSERVER_SERVER="http://127.0.0.1:$port" PRINTOBSERVER_CREDENTIAL=mock \
    HOME="$work/home" XDG_CONFIG_HOME="$work/home/.config" \
    timeout 120 freeze --execute "bash $work/$name.sh" --wrap 120 --window --output "$out/$name.png" >/dev/null </dev/null
}
for mode in before after; do
  python3 -I "$here/stub_server.py" "$port" "$mode" "$state" & stub=$!
  sleep 1
  shot "$mode" "look-text-$mode" "printobserver look --print-id $print"
  shot "$mode" "look-json-$mode" "printobserver look --print-id $print --json"
  shot "$mode" "status-$mode" "printobserver status --print-id $print"
  shot "$mode" "history-$mode" "printobserver history --print-id $print --limit 9"
  if [ "$mode" = after ]; then
    shot after recheck-camera-after "printobserver recheck-camera --print-id $print --actor '\"operator\"' --reason 'the camera was knocked; re-judge it'"
  fi
  kill "$stub"; wait "$stub" 2>/dev/null || true; stub=""
done
for mode in before after; do
  pillow stack "$out/look-text-$mode.png" "$out/look-json-$mode.png" "$out/look-$mode.png"
  rm "$out/look-text-$mode.png" "$out/look-json-$mode.png"
done
for name in look status history; do pillow pad "$out/$name-before.png" "$out/$name-after.png"; done
ls "$out"
