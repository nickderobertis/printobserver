#!/usr/bin/env python3
"""The check-costs spike: every measurement, one JSON summary on stdout.

    python3 spikes/check-costs/run.py [--turns [N]] [--obico] [--out PATH]

Run from the repository root. Progress goes to stderr.

- G-code: slices the two plates (`slice.sh`) unless `.work/` already holds
  them, and takes the shape statistics of both plates and of every G-code
  under `tests/real-prints/` (`shape_stats.py`, also written to
  `shape-stats.json`).
- `gcode-index-*`: builds the index prototype (`index/`) and runs it 5 times
  per plate reading the local file, and 5 times per plate through the
  OctoPrint adapter's own download path against the scripted OctoPrint
  (`just octoprint-up`, started if no instance is recorded).
- `agent-context-added-tokens`: reads the real server's `context` answer
  (the `spike_context` journey) at the long plate's layer with the most
  types, builds the variant carrying the index's facts and the added
  instruction, and reports both byte lengths. With `--turns N` (at most and
  by default 6), runs N real turns, alternating the variants, through
  oneharness's `claude-code` harness with no model set, and reads each
  turn's input-token usage.
- `scheduled-check-worst-delay-seconds`: go2rtc serving an ffmpeg 1920x1080
  test source (`camera_bench.py`; the binaries are fetched outside the tree),
  and the real scheduled-check path with a camera answering `500 exit status
  69` on every attempt (the `spike_check_delay` journey).
- `obico-reads-per-print-hour`: the reads per print hour at the default
  observation interval; with `--obico`, brings up the self-hosted Obico
  (`tools/obico-env`), times the status reads (`obico_reads.py`) and brings
  it down again.
"""

import argparse
import json
import math
import os
import platform
import socket
import statistics
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
WORK = HERE / ".work"
TOOLS = Path(os.environ.get("SPIKE_TOOLS_DIR", Path.home() / ".cache" / "printobserver-check-costs-spike"))
SCRATCH = Path(os.environ.get("SPIKE_SCRATCH_DIR") or (WORK / "scratch"))
RUNS = 5
PLATES = ("typical", "long")

# What the change under measurement assumes (see the task).
CAMERA_ATTEMPTS = 4  # 3 retries of a 5xx
CAMERA_RETRY_GAP_S = 0.5
TODAY_TIMEOUT_S = 10.0  # crates/printobserver-obico/src/vision.rs
OBSERVATION_INTERVAL_S = 120  # crates/printobserver-core/src/config.rs
DELAY_BUDGET_S = 10.0

INSTRUCTION = "Read the printobserver context answer below. Reply with the single word OK."
ADDED_SENTENCE = (
    "Features the G-code index lists at the current height are intended: "
    "do not report them as a failure."
)


def note(message):
    print(f"check-costs: {message}", file=sys.stderr, flush=True)


def run(argv, **kwargs):
    kwargs.setdefault("check", True)
    kwargs.setdefault("text", True)
    return subprocess.run(argv, cwd=kwargs.pop("cwd", ROOT), **kwargs)


def python(script, *args, uv=False):
    prefix = ["uv", "run", "-q", "python"] if uv else [sys.executable, "-I"]
    return run([*prefix, str(script), *map(str, args)], capture_output=True).stdout


def host_sample():
    load = os.getloadavg()
    return {"loadavg_1_5_15": [round(v, 2) for v in load], "at": time.strftime("%Y-%m-%dT%H:%M:%S%z")}


def host():
    model = next(
        (l.split(":", 1)[1].strip() for l in open("/proc/cpuinfo") if l.startswith("model name")),
        platform.processor(),
    )
    mem = next(int(l.split()[1]) for l in open("/proc/meminfo") if l.startswith("MemTotal"))
    return {
        "hostname": socket.gethostname(),
        "cpu": model,
        "logical_cpus": os.cpu_count(),
        "memory_gib": round(mem / 1024 / 1024, 1),
        "kernel": platform.release(),
    }


def summary(values):
    return {"median": statistics.median(values), "max": max(values), "runs": values}


# ~~ G-code


def gcode():
    if not all((WORK / f"{p}.gcode").is_file() for p in PLATES):
        note("slicing the two plates (PrusaSlicer 2.9.6 in Docker)")
        run(["bash", str(HERE / "slice.sh")])
    real = sorted((ROOT / "tests" / "real-prints").rglob("*.gcode"))
    named = [f"{p}={WORK / p}.gcode" for p in PLATES]
    named += [f"{g.relative_to(ROOT / 'tests')}={g}" for g in real]
    stats = json.loads(python(HERE / "shape_stats.py", *named))
    (HERE / "shape-stats.json").write_text(json.dumps(stats, indent=2) + "\n")
    return stats


# ~~ the index


def octoprint():
    record = ROOT / ".octoprint-env" / "instance.json"
    def alive():
        if not record.is_file():
            return None
        instance = json.loads(record.read_text())
        try:
            urllib.request.urlopen(f"{instance['url']}/api/version", timeout=5)
        except urllib.error.HTTPError:
            return instance  # it answers (403 without a key)
        except OSError:
            return None
        return instance
    instance = alive()
    if instance is None:
        note("starting the scripted OctoPrint (just octoprint-up)")
        run(["just", "octoprint-up"], capture_output=True)
        instance = alive()
    key = Path(instance["api_key_file"]).read_text().strip()
    url = instance["url"]
    held = json.load(urllib.request.urlopen(urllib.request.Request(
        f"{url}/api/files/local", headers={"X-Api-Key": key}), timeout=30))
    names = {f["name"]: f.get("size") for f in held["files"]}
    for plate in PLATES:
        name, path = f"spike-{plate}.gcode", WORK / f"{plate}.gcode"
        if names.get(name) != path.stat().st_size:
            note(f"uploading {name} to OctoPrint")
            run(["curl", "-fsS", "-H", f"X-Api-Key: {key}", "-F", f"file=@{path};filename={name}",
                 f"{url}/api/files/local"], capture_output=True)
    return url, key, instance


def index_measurements():
    note("building the index prototype")
    run(["cargo", "build", "-q", "--release", "--manifest-path", str(HERE / "index" / "Cargo.toml")])
    meta = json.loads(run(["cargo", "metadata", "-q", "--no-deps", "--format-version", "1",
                           "--manifest-path", str(HERE / "index" / "Cargo.toml")],
                          capture_output=True).stdout)
    binary = Path(meta["target_directory"]) / "release" / "gcode-index-spike"
    url, key, instance = octoprint()
    out = {"octoprint": {"url_kind": "scripted OctoPrint (tools/octoprint-env), loopback",
                         "mode": instance.get("mode"), "printing_during_run": instance.get("printing")}}
    for plate in PLATES:
        cases = {
            "local": [str(binary), "local", str(WORK / f"{plate}.gcode")],
            "octoprint": [str(binary), "octoprint", url, key, f"spike-{plate}.gcode"],
        }
        out[plate] = {}
        for case, argv in cases.items():
            note(f"index: {plate} via {case} x{RUNS}")
            load_before = os.getloadavg()[0]
            runs = [json.loads(run(argv, capture_output=True).stdout) for _ in range(RUNS)]
            last = runs[-1]
            out[plate][case] = {
                "wall_s": summary([round(r["wall_s"], 4) for r in runs]),
                "peak_heap_mb": summary([round(r["peak_heap_bytes"] / 1e6, 3) for r in runs]),
                "peak_rss_mb": summary([round(r["peak_rss_kib"] * 1024 / 1e6, 2) for r in runs]),
                "bytes_read": last["bytes_read"],
                "index_entries": last["index_entries"],
                "index_layers": last["index_layers"],
                "index_m73_markers": last["index_m73_markers"],
                "index_resident_bytes": last["index_resident_bytes"],
                "final_z_mm": round(last["final_z_mm"], 3),
                "layer_with_most_types": last["layer_with_most_types"],
                "loadavg_1min_before": round(load_before, 2),
            }
    worst_s = max(out[p][c]["wall_s"]["max"] for p in PLATES for c in ("local", "octoprint"))
    worst_mb = max(max(out[p][c]["peak_rss_mb"]["max"], out[p][c]["peak_heap_mb"]["max"])
                   for p in PLATES for c in ("local", "octoprint"))
    out["gcode-index-seconds"] = worst_s
    out["gcode-index-peak-memory-mb"] = worst_mb
    return out


# ~~ tokens


def cargo_journey(name, env=None):
    output = run(
        ["cargo", "test", "-q", "-p", "printobserver-server", "--test", "api", name,
         "--", "--ignored", "--nocapture", "--exact", f"{name}::{name}"],
        capture_output=True, env={**os.environ, **(env or {})},
    ).stdout
    marker = f"{name.upper()} "
    line = next(l for l in output.splitlines() if l.startswith(marker))
    return json.loads(line[len(marker):])


def context_variants(stats, index):
    long = index["long"]["local"]
    most = long["layer_with_most_types"]
    plate = stats["long"]
    estimated = plate["estimated_s"]["normal"]
    left_s = most["slicer_minutes_left"] * 60
    elapsed = estimated - left_s
    note("reading the real server's context answer (spike_context journey)")
    answer = cargo_journey("spike_context", {
        "SPIKE_FILE": "spike-long.gcode",
        "SPIKE_SIZE": str(plate["bytes"]),
        "SPIKE_POSITION": str(most["start_byte"]),
        "SPIKE_PRINT_TIME_S": str(elapsed),
    })
    # The stand-in's job record keeps its own size and estimate; put the
    # long plate's own in, so the answer is the one this print would read.
    job = answer["context"]["job"]
    job.update({
        "size_bytes": plate["bytes"],
        "estimated_print_time_s": estimated,
        "print_time_left_s": left_s,
        "completion": {"out_of_range": False, "value": round(most["start_byte"] / plate["bytes"], 4)},
    })
    answer["context"]["print"]["file_name"] = "spike-long.gcode"
    answer["context"]["manifest"]["file_name"] = "spike-long.gcode"
    today = json.dumps(answer, separators=(",", ":"), sort_keys=True)
    added = json.loads(today)
    added["context"]["gcode"] = {
        "layer": most["layer_number"],
        "layer_count": most["layer_count"],
        "z_mm": round(most["z_mm"], 2),
        "final_z_mm": round(long["final_z_mm"], 2),
        "types_in_layer": most["types"],
        "slicer_time_left_s": left_s,
    }
    new = json.dumps(added, separators=(",", ":"), sort_keys=True)
    return {
        "today": {"prompt": f"{INSTRUCTION}\n\n{today}", "context_bytes": len(today.encode())},
        "with_index": {"prompt": f"{INSTRUCTION}\n{ADDED_SENTENCE}\n\n{new}",
                       "context_bytes": len(new.encode())},
        "added_facts": added["context"]["gcode"],
        "added_sentence": ADDED_SENTENCE,
        "position": {"plate": "long", "layer": most["layer_number"], "start_byte": most["start_byte"]},
    }


def turn_env():
    """This host's environment without the variables of the session running
    the harness: the operator's server starts its turns with none of them, and
    Claude Code refuses to start inside another Claude Code session. The
    identity (`CLAUDE_CONFIG_DIR`) is kept."""
    return {
        k: v for k, v in os.environ.items()
        if k == "CLAUDE_CONFIG_DIR" or not (k == "CLAUDECODE" or k.startswith(("CLAUDE_CODE_", "CLAUDE_PID", "CLAUDE_EFFORT")))
    }


def turn(prompt, label):
    work = SCRATCH / "turns"
    work.mkdir(parents=True, exist_ok=True)
    prompt_file = work / f"{label}.prompt.txt"
    prompt_file.write_text(prompt)
    harness = ROOT / ".venv" / "bin" / "oneharness"
    answered = subprocess.run(
        [str(harness if harness.exists() else "oneharness"), "run", "--harness", "claude-code",
         "--no-config", "--mode", "read-only", "--format", "json",
         "--system-file", str(ROOT / "skills" / "printobserver" / "SKILL.md"),
         "--prompt-file", str(prompt_file), "--cwd", str(work), "--timeout", "300"],
        capture_output=True, text=True, cwd=work, env=turn_env(),
    )
    (work / f"{label}.report.json").write_text(answered.stdout)
    (work / f"{label}.stderr.txt").write_text(answered.stderr)
    report = json.loads(answered.stdout)
    result = report["results"][0]
    usage = result.get("usage") or {}
    raw = {}
    try:
        raw = json.loads(result.get("stdout") or "{}")
    except ValueError:
        pass
    models = sorted((raw.get("modelUsage") or {}).keys())
    parts = [usage.get(k) or 0 for k in ("input_tokens", "cache_read_tokens", "cache_write_tokens")]
    return {
        "variant": label.split("-")[0],
        "status": result.get("status"),
        "failure_kind": result.get("failure_kind"),
        "observed_model": result.get("observed_model"),
        "models_in_usage": models,
        "input_tokens": usage.get("input_tokens"),
        "cache_read_tokens": usage.get("cache_read_tokens"),
        "cache_write_tokens": usage.get("cache_write_tokens"),
        "total_input_tokens": sum(parts),
        "output_tokens": usage.get("output_tokens"),
        "text": (result.get("text") or "")[:80],
        "stderr_tail": (result.get("stderr") or answered.stderr or "")[-300:] if result.get("status") != "ok" else "",
    }


def tokens(stats, index, real_turns):
    variants = context_variants(stats, index)
    out = {
        "today_context_bytes": variants["today"]["context_bytes"],
        "with_index_context_bytes": variants["with_index"]["context_bytes"],
        "today_prompt_bytes": len(variants["today"]["prompt"].encode()),
        "with_index_prompt_bytes": len(variants["with_index"]["prompt"].encode()),
        "added_facts": variants["added_facts"],
        "added_sentence": variants["added_sentence"],
        "position": variants["position"],
        "images_added": 0,
        "harness": "oneharness claude-code, no model set, --no-config, read-only, SKILL.md as system",
    }
    out["added_prompt_bytes"] = out["with_index_prompt_bytes"] - out["today_prompt_bytes"]
    if not real_turns:
        out["turns"] = []
        out["agent-context-added-tokens"] = None
        return out
    turns = []
    for i in range(real_turns):
        label = ("today" if i % 2 == 0 else "with_index") + f"-{i // 2 + 1}"
        note(f"real turn {i + 1}/{real_turns} ({label})")
        answer = turn(variants[label.split("-")[0]]["prompt"], label)
        turns.append(answer)
        if answer["status"] != "ok":
            note(f"turn refused or failed ({answer['failure_kind']}); stopping")
            break
    out["turns"] = turns
    token_figures(out)
    return out


TOTALS_NOTE = (
    "Each variant's ratio is its median total input tokens per turn (input + cache-read + "
    "cache-write) over its whole prompt's bytes. Those totals include the harness's own "
    "system prompt and tools and the skill (the same ~64k tokens in both variants), so they "
    "are not the tokens of the context answer alone and the ratio is no per-byte rate for it; "
    "use tokens_per_added_byte to convert bytes of added context to tokens."
)


def token_figures(out):
    """The token figures, from the recorded turns and prompt byte lengths only."""
    ok = [t for t in out["turns"] if t["status"] == "ok"]
    by = {v: [t["total_input_tokens"] for t in ok if t["variant"] == v] for v in ("today", "with_index")}
    if not (by["today"] and by["with_index"]):
        out["agent-context-added-tokens"] = None
        return out
    medians = {v: statistics.median(n) for v, n in by.items()}
    added = medians["with_index"] - medians["today"]
    out["median_total_input_tokens"] = medians
    out["agent-context-added-tokens"] = added
    out["tokens_per_byte"] = round(added / out["added_prompt_bytes"], 4)
    out["tokens_per_added_byte"] = out["tokens_per_byte"]
    out["per_variant_total_tokens_per_prompt_byte"] = {
        v: round(medians[v] / out[f"{v}_prompt_bytes"], 4) for v in medians
    }
    out["per_variant_note"] = TOTALS_NOTE
    return out


# ~~ the camera and the check path


def camera_tools():
    go2rtc = TOOLS / "go2rtc" / "go2rtc_linux_amd64"
    if not go2rtc.exists():
        note("fetching go2rtc's release binary")
        go2rtc.parent.mkdir(parents=True, exist_ok=True)
        run(["gh", "release", "download", "-R", "AlexxIT/go2rtc", "-p", "go2rtc_linux_amd64",
             "-D", str(go2rtc.parent), "--clobber"])
        go2rtc.chmod(0o755)
    ffmpegs = sorted((TOOLS / "ffmpeg").glob("*/bin/ffmpeg"))
    if not ffmpegs:
        note("fetching a static ffmpeg")
        (TOOLS / "ffmpeg").mkdir(parents=True, exist_ok=True)
        archive = TOOLS / "ffmpeg" / "ffmpeg.tar.xz"
        run(["curl", "-fsSL", "-o", str(archive),
             "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-linux64-gpl.tar.xz"])
        run(["tar", "-xJf", str(archive), "-C", str(TOOLS / "ffmpeg")])
        archive.unlink()
        ffmpegs = sorted((TOOLS / "ffmpeg").glob("*/bin/ffmpeg"))
    version = run([str(ffmpegs[0]), "-version"], capture_output=True).stdout.splitlines()[0]
    return go2rtc, ffmpegs[0], version


def delay(camera, check):
    rest_s = max(check["due_to_last_append_ms"]) / 1000
    worst_p99 = max(camera["sequential"]["p99_s"], camera["concurrent_4"]["p99_s"])
    cold_max = camera["cold_after_idle"]["max_s"]
    # Well above the local p99: 4x the warm p99 and 2x the slowest cold fetch,
    # whichever is larger, rounded up to the half second.
    recommended = math.ceil(max(4 * worst_p99, 2 * cold_max) * 2) / 2
    def worst(timeout):
        return round(CAMERA_ATTEMPTS * timeout + (CAMERA_ATTEMPTS - 1) * CAMERA_RETRY_GAP_S + rest_s, 3)
    largest_holding = (DELAY_BUDGET_S - (CAMERA_ATTEMPTS - 1) * CAMERA_RETRY_GAP_S - rest_s) / CAMERA_ATTEMPTS
    return {
        "rest_of_check_path_s": {"median": statistics.median(check["due_to_last_append_ms"]) / 1000,
                                 "max": rest_s, "rounds": check["rounds_measured"],
                                 "round_event_kinds": check["round_event_kinds"]},
        "recommended_timeout_s": recommended,
        "recommended_margin": {"over_warm_p99": round(recommended / worst_p99, 2),
                               "over_cold_max": round(recommended / cold_max, 2)},
        "worst_case_at_today_timeout_s": worst(TODAY_TIMEOUT_S),
        "worst_case_at_recommended_timeout_s": worst(recommended),
        "largest_timeout_holding_10s": round(largest_holding, 3),
        "ten_seconds_holds_at_today": worst(TODAY_TIMEOUT_S) <= DELAY_BUDGET_S,
        "ten_seconds_holds_at_recommended": worst(recommended) <= DELAY_BUDGET_S,
        "scheduled-check-worst-delay-seconds": worst(recommended),
    }


# ~~ Obico


def obico(bring_up):
    out = {
        "observation_interval_s": OBSERVATION_INTERVAL_S,
        "requests_per_status_read": 1,
        "status_read": "GET /api/v1/printers/{id}/ (normalized_p of the latest prediction, current_print)",
        "rate_limit": "none on the printer API: DEFAULT_THROTTLE_RATES is set (3600/hour) but no "
                      "DEFAULT_THROTTLE_CLASSES, and PrinterViewSet names no throttle_classes "
                      "(backend/config/settings.py, backend/api/viewsets.py at 49c0bc70)",
    }
    out["obico-reads-per-print-hour"] = 3600 / OBSERVATION_INTERVAL_S * out["requests_per_status_read"]
    if not bring_up:
        out["measured"] = None
        return out
    state = SCRATCH / "obico-state"
    project = "po-check-costs-obico"
    env_script = ROOT / "tools" / "obico-env" / "obico_env.py"
    note("bringing the self-hosted Obico up (tools/obico-env)")
    started = time.monotonic()
    run(["uv", "run", "-q", "python", str(env_script), "up", "--state-dir", str(state),
         "--project", project], capture_output=True)
    out["bring_up_s"] = round(time.monotonic() - started, 1)
    try:
        out["measured"] = json.loads(python(HERE / "obico_reads.py", state, 20, uv=True))
    finally:
        note("bringing Obico down")
        run(["uv", "run", "-q", "python", str(env_script), "down", "--state-dir", str(state),
             "--project", project], capture_output=True)
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--turns", type=int, nargs="?", const=6, default=0, metavar="N",
                        help="run N real model turns (default 6, the most allowed), alternating the variants")
    parser.add_argument("--obico", action="store_true", help="bring up the real Obico and time its reads")
    parser.add_argument("--out", type=Path, help="also write the summary here")
    parser.add_argument("--refigure", type=Path, metavar="SUMMARY",
                        help="recompute the token figures of a written summary from its recorded "
                             "turns, in place, measuring and spending nothing")
    args = parser.parse_args()
    if args.refigure:
        result = json.loads(args.refigure.read_text())
        token_figures(result["tokens"])
        result["candidates"]["agent-context-added-tokens"] = result["tokens"]["agent-context-added-tokens"]
        args.refigure.write_text(json.dumps(result, indent=2) + "\n")
        print(json.dumps(result["tokens"], indent=2))
        return
    SCRATCH.mkdir(parents=True, exist_ok=True)
    if args.turns > 6:
        parser.error("at most 6 real turns")
    result = {"harness": "python3 spikes/check-costs/run.py" + (f" --turns {args.turns}" if args.turns else "")
              + (" --obico" if args.obico else ""),
              "host": host(), "load": {"start": host_sample()}}
    result["gcode_shape"] = gcode()
    result["load"]["after_gcode"] = host_sample()
    result["index"] = index_measurements()
    result["load"]["after_index"] = host_sample()
    result["tokens"] = tokens(result["gcode_shape"], result["index"], args.turns)
    result["load"]["after_tokens"] = host_sample()
    go2rtc, ffmpeg, ffmpeg_version = camera_tools()
    note("camera: 200 sequential, 200 with 4 fetchers, 8 cold fetches")
    camera = json.loads(python(HERE / "camera_bench.py", go2rtc, ffmpeg, SCRATCH / "go2rtc"))
    camera["ffmpeg_version"] = ffmpeg_version
    result["camera"] = camera
    result["load"]["after_camera"] = host_sample()
    note("the scheduled-check path with a failing camera (spike_check_delay journey)")
    check = cargo_journey("spike_check_delay")
    result["delay"] = delay(camera, check)
    result["load"]["after_delay"] = host_sample()
    result["obico"] = obico(args.obico)
    result["load"]["end"] = host_sample()
    result["candidates"] = {
        "gcode-index-seconds": result["index"]["gcode-index-seconds"],
        "gcode-index-peak-memory-mb": result["index"]["gcode-index-peak-memory-mb"],
        "agent-context-added-tokens": result["tokens"]["agent-context-added-tokens"],
        "scheduled-check-worst-delay-seconds": result["delay"]["scheduled-check-worst-delay-seconds"],
        "obico-reads-per-print-hour": result["obico"]["obico-reads-per-print-hour"],
    }
    text = json.dumps(result, indent=2)
    if args.out:
        args.out.write_text(text + "\n")
    print(text)


if __name__ == "__main__":
    main()
