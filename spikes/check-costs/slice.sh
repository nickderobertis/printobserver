#!/usr/bin/env bash
# Slice the spike's two plates with PrusaSlicer 2.9.6 and the CORE One system
# profiles, into spikes/check-costs/.work (gitignored).
#
#   typical: 3 specimens at height factor 1.12  (~2.5 h estimated)
#   long:    10 specimens at height factor 1.6  (~8.8 h estimated)
#
# PrusaSlicer 2.9.6 publishes no Linux AppImage on GitHub (only .exe, .dmg and
# a Windows .zip), so the slicer is Prusa Research's own Flathub build of 2.9.6,
# installed once into a local Docker image (po-spike-prusaslicer:2.9.6).
# Profiles are the PrusaResearch vendor bundle 2.5.12 from Prusa's preset
# repository, the newest bundle whose resolved CORE One / BALANCED / Prusament
# PLA config is identical to the one tests/real-prints was sliced with.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
work="$here/.work"
image=po-spike-prusaslicer:2.9.6
bundle_version=2.5.12
bundle_sha256=60769af8affc66ec7d74a597318a1f9815e91984ccce28d44f13a1c2c88a4e59
mkdir -p "$work"

if ! docker image inspect "$image" >/dev/null 2>&1; then
  name="po-spike-prusaslicer-build-$$"
  docker run --name "$name" --privileged ubuntu:24.04 bash -c '
    apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq flatpak dbus >/dev/null &&
    flatpak remote-add --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo &&
    flatpak install -y --noninteractive flathub com.prusa3d.PrusaSlicer//stable &&
    flatpak info com.prusa3d.PrusaSlicer | grep -q "Version: 2.9.6"' >&2
  docker commit "$name" "$image" >/dev/null
  docker rm "$name" >/dev/null
fi

if [ ! -f "$work/PrusaResearch.ini" ]; then
  curl -fsSL -o "$work/PrusaResearch.ini" \
    "https://preset-repo-api.prusa3d.com/v1/repos/prusa-fff/PrusaResearch/$bundle_version.ini"
fi
echo "$bundle_sha256  $work/PrusaResearch.ini" | sha256sum -c --quiet

python3 "$here/resolve_printer_profile.py" "$work/PrusaResearch.ini" \
  "Prusa CORE One HF0.4 nozzle" "$work/printer.ini"

cat > "$work/ps.sh" <<'INNER'
mkdir -p /run/dbus; dbus-daemon --system --fork >/dev/null 2>&1
rm -rf /work/datadir; mkdir -p /work/datadir/vendor
cp /work/PrusaResearch.ini /work/datadir/vendor/PrusaResearch.ini
printf '[vendor:PrusaResearch]\nCOREONE = HF0.4\n\n[filaments]\nPrusament PLA @COREONE HF0.4 = 1\n' > /work/datadir/PrusaSlicer.ini
exec dbus-run-session -- flatpak run --filesystem=/work --command=prusa-slicer \
  com.prusa3d.PrusaSlicer --datadir /work/datadir "$@" 2>&1 | grep -v dbus-daemon
INNER

slice() { # plate copies height_factor
  python3 "$here/geometry.py" "$work/$1.stl" "$3"
  docker run --rm --privileged -v "$work:/work" "$image" bash /work/ps.sh \
    --load /work/printer.ini \
    --printer-profile "Prusa CORE One HF0.4 nozzle" \
    --print-profile "0.20mm BALANCED @COREONE HF0.4" \
    --material-profile "Prusament PLA @COREONE HF0.4" \
    --support-material --support-material-auto \
    --duplicate "$2" \
    --export-gcode --no-binary-gcode \
    --output "/work/$1.gcode" "/work/$1.stl" | grep -E 'exported|rror' >&2
}
slice typical 3 1.12
slice long 10 1.6
ls -l "$work"/typical.gcode "$work"/long.gcode >&2
# The container wrote as root; hand the work directory back.
docker run --rm -v "$work:/work" ubuntu:24.04 chown -R "$(id -u):$(id -g)" /work
