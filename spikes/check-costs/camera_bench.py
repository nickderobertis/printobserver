#!/usr/bin/env python3
"""Fetch go2rtc's `frame.jpeg` from a local go2rtc serving an ffmpeg 1920x1080
test source, and report the fetch times as JSON.

Usage: camera_bench.py GO2RTC_BIN FFMPEG_BIN WORK_DIR

Three series: 200 fetches one after another (the stream warm), 200 with four
concurrent fetchers, and a few cold fetches each taken after go2rtc has
stopped the idle stream — which is what a check every 120 s meets.
"""

import concurrent.futures
import json
import os
import socket
import statistics
import subprocess
import sys
import time
import urllib.request

COLD_GAP_S = 15
COLD_FETCHES = 8


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def fetch(url, timeout=60):
    started = time.perf_counter()
    try:
        with urllib.request.urlopen(url, timeout=timeout) as answer:
            body = answer.read()
            status = answer.status
    except urllib.error.HTTPError as error:
        body, status = error.read(), error.code
    elapsed = time.perf_counter() - started
    return {"s": elapsed, "status": status, "bytes": len(body), "jpeg": body[:2] == b"\xff\xd8"}


def summary(results):
    times = sorted(r["s"] for r in results)
    q = statistics.quantiles(times, n=100, method="inclusive") if len(times) > 1 else times * 99
    return {
        "n": len(times),
        "ok": sum(1 for r in results if r["status"] == 200 and r["jpeg"]),
        "p50_s": round(q[49], 4),
        "p95_s": round(q[94], 4),
        "p99_s": round(q[98], 4),
        "max_s": round(times[-1], 4),
        "mean_bytes": round(statistics.mean(r["bytes"] for r in results)),
    }


def main():
    go2rtc, ffmpeg, work = sys.argv[1:4]
    os.makedirs(work, exist_ok=True)
    port, rtsp = free_port(), free_port()
    config = os.path.join(work, "go2rtc.yaml")
    with open(config, "w") as out:
        out.write(
            f"api:\n  listen: 127.0.0.1:{port}\n"
            f"rtsp:\n  listen: 127.0.0.1:{rtsp}\nwebrtc:\n  listen: \"\"\nsrtp:\n  listen: \"\"\n"
            f"ffmpeg:\n  bin: {ffmpeg}\n"
            "streams:\n  test: ffmpeg:virtual?video=testsrc&size=1920x1080#video=h264\n"
        )
    log = open(os.path.join(work, "go2rtc.log"), "w")
    server = subprocess.Popen([go2rtc, "-config", config], stdout=log, stderr=log)
    try:
        url = f"http://127.0.0.1:{port}/api/frame.jpeg?src=test"
        deadline = time.time() + 30
        while time.time() < deadline:
            try:
                urllib.request.urlopen(f"http://127.0.0.1:{port}/api", timeout=2).read()
                break
            except OSError:
                time.sleep(0.2)
        first = fetch(url)
        sequential = [fetch(url) for _ in range(200)]
        with concurrent.futures.ThreadPoolExecutor(4) as pool:
            concurrent_ = list(pool.map(lambda _: fetch(url), range(200)))
        cold = []
        for _ in range(COLD_FETCHES):
            time.sleep(COLD_GAP_S)
            cold.append(fetch(url))
        print(json.dumps({
            "source": "ffmpeg:virtual?video=testsrc&size=1920x1080#video=h264",
            "go2rtc_version": subprocess.run([go2rtc, "-version"], capture_output=True, text=True).stdout.strip(),
            "first_fetch_s": round(first["s"], 4),
            "sequential": summary(sequential),
            "concurrent_4": summary(concurrent_),
            "cold_after_idle": {**summary(cold), "gap_s": COLD_GAP_S, "times_s": [round(r["s"], 4) for r in cold]},
        }))
    finally:
        server.terminate()
        server.wait(10)
        log.close()


if __name__ == "__main__":
    main()
