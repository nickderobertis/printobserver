"""Temporary: which ETW events attribute a TCP connect attempt to its process.

Run on the Windows runners by `platform-probe.yml`, and deleted with the probe.
It starts one trace session over three providers, runs a child that makes four
connects — refused, accepted, to an unroutable address, and one from a
grandchild — stops the session, and prints every decoded event of those
processes, so the Windows backend of the command-line journeys' tracer can be
written against what the runner actually records.
"""

from __future__ import annotations

import json
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

CHILD = r"""
import socket, subprocess, sys
refused, accepted = int(sys.argv[1]), int(sys.argv[2])
def attempt(host, port):
    s = socket.socket()
    s.settimeout(1.5)
    try:
        s.connect((host, port))
        print("connected", host, port, flush=True)
    except OSError as error:
        print("failed", host, port, error, flush=True)
    finally:
        s.close()
attempt("127.0.0.1", refused)
attempt("127.0.0.1", accepted)
attempt("192.0.2.1", 9)
code = "import socket\ns = socket.socket()\ns.settimeout(1)\ntry:\n    s.connect(('127.0.0.1', %d))\nexcept OSError as e:\n    print('grandchild failed', e)\n" % (refused + 1)
grandchild = subprocess.Popen([sys.executable, "-c", code])
grandchild.wait()
print("grandchild pid", grandchild.pid, flush=True)
"""


def run(argv: list[str]) -> subprocess.CompletedProcess[str]:
    """Run one program and echo what it said."""
    print("$", " ".join(argv), flush=True)
    done = subprocess.run(argv, capture_output=True, text=True, check=False)
    print(done.returncode, done.stdout[-4000:], done.stderr[-4000:], flush=True)
    return done


def main() -> int:
    """Trace one child's connects and print what was recorded."""
    work = Path(tempfile.mkdtemp())
    etl = work / "experiment.etl"
    providers = work / "providers.txt"
    providers.write_text(
        '"Microsoft-Windows-Kernel-Network" 0xFFFFFFFFFFFFFFFF 0xFF\n'
        '"Microsoft-Windows-Winsock-AFD" 0xFFFFFFFFFFFFFFFF 0xFF\n'
        '"Microsoft-Windows-Kernel-Process" 0x10 0xFF\n'
        '"Microsoft-Windows-TCPIP" 0xFFFFFFFFFFFFFFFF 0xFF\n',
        encoding="ascii",
    )
    logman = shutil.which("logman") or "logman"
    run([logman, "start", "po-experiment", "-ets", "-o", str(etl), "-pf", str(providers), "-bs", "1024", "-nb", "64", "128"])

    listener = socket.socket()
    listener.bind(("127.0.0.1", 0))
    listener.listen()
    accepted_port = listener.getsockname()[1]
    threading.Thread(target=lambda: [listener.accept() for _ in range(3)], daemon=True).start()
    probe = socket.socket()
    probe.bind(("127.0.0.1", 0))
    refused_port = probe.getsockname()[1]
    probe.close()

    child = subprocess.Popen(
        [sys.executable, "-c", CHILD, str(refused_port), str(accepted_port)],
        stdout=subprocess.PIPE, text=True,
    )
    out, _ = child.communicate(timeout=60)
    print("child pid", child.pid, "said", out, flush=True)
    pids = {child.pid}
    for line in out.splitlines():
        if line.startswith("grandchild pid "):
            pids.add(int(line.split()[-1]))
    time.sleep(2)
    run([logman, "stop", "po-experiment", "-ets"])
    print("refused", refused_port, "accepted", accepted_port, "etl bytes", etl.stat().st_size if etl.exists() else None, flush=True)

    powershell = shutil.which("powershell") or "powershell"
    script = (
        f"Get-WinEvent -Path '{etl}' -Oldest | "
        "Select-Object Id,ProviderName,ProcessId,@{n='Props';e={($_.Properties | ForEach-Object { \"$($_.Value)\" }) -join ' | '}},"
        "@{n='Msg';e={$_.Message}} | ConvertTo-Json -Depth 3 -Compress"
    )
    decoded = run([powershell, "-NoProfile", "-Command", script])
    try:
        events = json.loads(decoded.stdout)
    except json.JSONDecodeError:
        print("undecodable", decoded.stdout[:2000])
        return 0
    if isinstance(events, dict):
        events = [events]
    ports = {str(refused_port), str(accepted_port), str(refused_port + 1), "9"}
    print("total events", len(events), flush=True)
    for event in events:
        text = json.dumps(event)
        if event.get("ProcessId") in pids or any(p in text for p in ports) or "192.0.2.1" in text:
            print(text[:1500], flush=True)
    counts: dict[str, int] = {}
    for event in events:
        key = f"{event.get('ProviderName')}#{event.get('Id')}"
        counts[key] = counts.get(key, 0) + 1
    print(json.dumps(counts, indent=1), flush=True)
    tracerpt = shutil.which("tracerpt") or "tracerpt"
    xml = work / "experiment.xml"
    run([tracerpt, str(etl), "-of", "XML", "-o", str(xml), "-y"])
    if xml.exists():
        text = xml.read_text(encoding="utf-16", errors="replace") if xml.read_bytes()[:2] in (b"\xff\xfe", b"\xfe\xff") else xml.read_text(errors="replace")
        for chunk in text.split("<Event ")[1:]:
            if str(refused_port) in chunk or "192.0.2.1" in chunk or any(f'ProcessID="{pid}"' in chunk for pid in pids):
                print("XML <Event " + chunk[:1200], flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
