"""Prove the installed Python client against a real running supervisor.

This runs from the **installed distribution** rather than from this repository's
sources: what it imports is whatever `printobserver_sdk` the environment
resolves. A smoke check that reached no server would say nothing about the
artifact, so it makes two real calls — a status read, and an image
materialization whose answered path it opens and whose bytes it checks against
the digest the image record itself declares.

    python smoke.py --server http://127.0.0.1:8420 --print-id <id> --image-id <id>
"""

from __future__ import annotations

import argparse
import hashlib
import sys
from pathlib import Path

from printobserver_sdk import CONTRACT_VERSION, Client


def main(argv: list[str] | None = None) -> int:
    """Make the two calls, answering a process exit status."""
    parser = argparse.ArgumentParser(prog="printobserver-sdk-smoke", description=__doc__)
    parser.add_argument("--server", required=True)
    parser.add_argument("--print-id", required=True)
    parser.add_argument("--image-id", required=True)
    asked = parser.parse_args(argv)

    client = Client(asked.server, "operator")

    status = client.status(asked.print_id)
    if status["print"]["id"] != asked.print_id:
        print(f"the status read answered another print: {status['print']['id']}", file=sys.stderr)
        return 1

    answered = client.image(asked.image_id)
    path = answered.get("path")
    if not isinstance(path, str):
        print("the image read answered no path on the server's own host", file=sys.stderr)
        return 1
    if not Path(path).is_absolute():
        print(f"the image read answered {path}, which is not an absolute path", file=sys.stderr)
        return 1
    digest = hashlib.sha256(Path(path).read_bytes()).hexdigest()
    declared = answered["record"]["sha256"]
    if digest != declared:
        print(
            f"{path} is not the image the record declares ({digest} vs {declared})",
            file=sys.stderr,
        )
        return 1

    print(
        f"printobserver-sdk smoke: contract {CONTRACT_VERSION}, "
        f"print {status['print']['state']}, image {declared[:12]} at {path}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
