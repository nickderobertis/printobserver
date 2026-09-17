"""Prove the installed Python client against a real running supervisor.

This runs from the **installed distribution** rather than from this repository's
sources: what it imports is whatever `printobserver_sdk` the environment
resolves. A smoke check that reached no server would say nothing about the
artifact, so it makes two real calls — a status read, and an image
materialization whose answered path it opens and whose bytes it checks against
the digest the image record itself declares.

    python smoke.py --server http://127.0.0.1:8420 --credential <credential> \
        --print-id <id> --image-id <id>
"""

from __future__ import annotations

import hashlib
import sys
from pathlib import Path

from printobserver_sdk import CONTRACT_VERSION, Client

#: What a refused `--credential` is told, which never quotes what it was given.
CREDENTIAL_USAGE = (
    "smoke: --credential takes the credential the supervisor serves under: printable ASCII, "
    "not empty, and neither beginning nor ending with a space"
)

#: The exit of a check stopped by what it was given, before it made any request.
USAGE = 2


class UsageError(Exception):
    """The check was given something it cannot run with, and says which."""


def argument(argv: list[str], name: str) -> str:
    """One named argument, taken exactly as given.

    The token after `--<name>` is the value whatever it begins with: the
    credential a supervisor generates for itself is unpadded URL-safe base64,
    whose alphabet includes `-`, so one in sixty-four of them begins with a
    character an option parser would read as the start of another option. The
    Node and Rust smoke checks read theirs the same way.

    Raises:
        UsageError: If the argument is absent or given no value.
    """
    try:
        at = argv.index(f"--{name}")
    except ValueError:
        at = -1
    if at < 0 or at + 1 >= len(argv):
        msg = f"smoke: --{name} takes a value and was given none"
        raise UsageError(msg)
    return argv[at + 1]


def presentable(credential: str) -> bool:
    """Whether a credential is one an `Authorization` header carries intact."""
    return (
        bool(credential)
        and all(" " <= character <= "~" for character in credential)
        and not credential.startswith(" ")
        and not credential.endswith(" ")
    )


def main(argv: list[str] | None = None) -> int:
    """Make the two calls, answering a process exit status."""
    given = sys.argv[1:] if argv is None else argv
    try:
        server = argument(given, "server")
        credential = argument(given, "credential")
        print_id = argument(given, "print-id")
        image_id = argument(given, "image-id")
        if not presentable(credential):
            raise UsageError(CREDENTIAL_USAGE)
    except UsageError as usage:
        print(usage, file=sys.stderr)
        return USAGE

    # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
    client = Client(server, "operator", credential)

    status = client.status(print_id)
    if status["print"]["id"] != print_id:
        print(f"the status read answered another print: {status['print']['id']}", file=sys.stderr)
        return 1

    answered = client.image(image_id)
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
