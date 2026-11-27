"""SPIKE MOCK (throwaway): compose the 13:14 check image through the real
composer, with the calibration's projection translated by a hard-coded shift
standing in for the planned re-fit.

usage: check_image.py <compose-binary> <frames-dir> <out.jpg> <dx> <dy> <landmark-dx> <landmark-dy>

The outline is moved by (dx, dy) and the landmarks by their own shift, so
the drift check accepts the frame: two hard-coded stand-ins for one re-fit.
"""

import json
import pathlib
import subprocess
import sys
import tempfile

compose, frames, out, dx, dy, ldx, ldy = sys.argv[1:8]
frames = pathlib.Path(frames)
repo = pathlib.Path(__file__).resolve().parents[2]
calibration = json.loads((repo / "tests/real-prints/cameras/core-one-2026-09-27.json").read_text())
calibration["intrinsics"]["cx"] += float(dx)
calibration["intrinsics"]["cy"] += float(dy)
for landmark in calibration["landmarks"]:
    landmark["center_px"] = [landmark["center_px"][0] + float(ldx), landmark["center_px"][1] + float(ldy)]
template = (repo / "scripts/install.sh").read_text()
body = template.split("<<CONFIG || die", 1)[1].split("\n", 1)[1].split("\nCONFIG\n", 1)[0]
with tempfile.TemporaryDirectory() as work:
    work = pathlib.Path(work)
    (work / "calibration.json").write_text(json.dumps(calibration))
    (work / "state/skills/printobserver").mkdir(parents=True)
    (work / "state/skills/printobserver/SKILL.md").write_text("mock")
    config = (
        body.replace("$RUNTIME_STATE", (work / "state").as_posix())
        .replace("$INTERVAL_LINE", "observation_interval_s = 120")
        .replace('api_key = ""', 'api_key = "mock"')
        .replace('shared_secret = ""', 'shared_secret = "mock"')
        + f'\n[camera]\nsnapshot_url = "{calibration["camera_snapshot_url"]}"\n'
        + f'calibration = "{(work / "calibration.json").as_posix()}"\n'
    )
    (work / "config.toml").write_text(config)
    done = subprocess.run(
        [
            compose, "--config", str(work / "config.toml"),
            "--gcode", str(frames / "composed-if-aligned/octoprint-copy.gcode"),
            "--frame", str(frames / "agent-images/131408-periodic_observation.jpg"),
            "--position", "24738", "--out", out,
            "--at", "2026-11-26T19:14:08Z",
            "--previous", str(frames / "agent-images/131207-periodic_observation.jpg"),
            "--previous-at", "2026-11-26T19:12:07Z",
        ],
        capture_output=True, text=True, check=False,
    )
    print(done.stdout or done.stderr, end="")
    sys.exit(done.returncode)
