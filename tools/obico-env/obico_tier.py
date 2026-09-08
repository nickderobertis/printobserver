#!/usr/bin/env python3
"""The scheduled Obico tier: reconcile what a live Obico posts with the sample.

The fast vision tier replays a recorded Obico payload, and a recording is only
as good as the last time somebody held it against the thing being recorded.
Obico is an external project on its own release cadence: the day its webhook
payload gains or renames a field, every replayed test goes on passing and the
running system stops seeing failures. This tier is the only thing that notices.

It walks one path, in this order, and nothing in it compares a body the capture
did not produce:

    1. listen on the address the stack's notification plugin was configured with
    2. cause a real failure alert on the stack
    3. capture the body the stack posts
    4. fetch the image that captured body points at, and read its bytes
    5. compare that captured body against the committed sample
    6. report a verdict naming every field that differs

What is compared is the *shape* — the set of fields and the JSON type of each —
rather than the values, because the ids, the file name and the instants differ
on every run by design and are not what the sample claims. A field the stack
added, renamed, removed or retyped moves the shape, and is named in the verdict.

    obico_tier.py reconcile --webhook-url URL --trigger compose:<state-dir>
    obico_tier.py reconcile --webhook-url URL --trigger post:<url>

The trigger says how to cause the alert and nothing else: `compose` causes a
real one on the self-hosted stack `obico_env.py up` left under a state
directory, and `post` asks a stand-in to post — which is how the tier itself is
tested, by running an altered body through the same capture.

Exit status is 0 when the captured body agrees with the sample, 1 when it
diverges — with the verdict on stdout either way — and 2 when the tier could not
reach a verdict at all.
"""

from __future__ import annotations

import argparse
import json
import sys
import threading
import time
import urllib.parse
import urllib.request
from dataclasses import dataclass, field
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any

from obico_env import PRINTER_NAME, Stack, note, run_program

# The committed sample this repository holds as its claim about the producer,
# relative to the repository root. It is the `contracts` node's file and this
# tier does not write it: a divergence found here is a finding to report.
SAMPLE = "crates/printobserver-types/samples/obico/failure-alert.json"

# The snapshot the tier hands the stack, so the alert has an image to point at.
SNAPSHOT = Path(__file__).resolve().parent / "snapshot" / "spaghetti.jpg"

REPOSITORY_ROOT = Path(__file__).resolve().parents[2]

AGREES = "agrees"
DIFFERS = "differs"


class TierError(Exception):
    """The tier could not reach a verdict, with what stopped it."""


def http(
    url: str,
    *,
    data: bytes | None = None,
    method: str = "GET",
    headers: dict[str, str] | None = None,
    timeout: float = 60.0,
) -> tuple[int, bytes, str]:
    """The one place this tier opens a URL, answering status, body and content type.

    Funnelled through here so that `S310` — opening a URL whose scheme is not
    audited — has one reviewable site rather than one per caller. Every URL
    reaching it is either an address the caller named on the command line or the
    `img_url` the captured body carries, which is the very thing being reported
    on.

    Raises:
        OSError: If the address cannot be reached or answers an error status.
        ValueError: If it is not a URL that can be opened at all.
    """
    request = urllib.request.Request(url, data=data, method=method, headers=headers or {})  # noqa: S310
    with urllib.request.urlopen(request, timeout=timeout) as answer:  # noqa: S310
        return answer.status, answer.read(), answer.headers.get("Content-Type", "")


# ~~ the shape of a body


def type_of(value: object) -> str:
    """The JSON type of one value, with whole numbers distinguished from reals.

    `integer` is narrower than `number`: a producer that started sending `17.5`
    where the sample records `17` has moved a field that is an integer
    identifier on this side, and a comparison that called both "number" would
    not see it. The other direction is not a move — a whole-valued real is a
    real — which `differs_in_type` is what encodes.
    """
    if isinstance(value, bool):
        return "boolean"
    if isinstance(value, int):
        return "integer"
    if isinstance(value, float):
        return "number"
    if isinstance(value, str):
        return "string"
    if isinstance(value, list):
        return "array"
    if isinstance(value, dict):
        return "object"
    return "null"


def differs_in_type(sample: str, captured: str) -> bool:
    """Whether a captured type is one the sample's type does not admit."""
    if sample == captured:
        return False
    # A sample that records a real admits a whole-valued one: JSON has one
    # number type, and `1772366400.0` is written `1772366400`.
    return not (sample == "number" and captured == "integer")


def shape_of(body: object, prefix: str = "") -> dict[str, str]:
    """Every field of a body, by dotted path, with the JSON type of each."""
    if isinstance(body, dict):
        found: dict[str, str] = {}
        for name, value in body.items():
            path = f"{prefix}{name}"
            found[path] = type_of(value)
            found.update(shape_of(value, f"{path}."))
        return found
    if isinstance(body, list) and body:
        # One entry stands for the whole array: the producer sends homogeneous
        # arrays, and a per-index path would name a field nobody wrote.
        return shape_of(body[0], f"{prefix}[].")
    return {}


def differences(sample: object, captured: object) -> list[str]:
    """Every field the captured body moved, against the committed sample.

    A field that was added, removed or retyped is named once; a field that was
    renamed shows as both the name that went and the name that arrived, which is
    what a rename is on the wire.
    """
    expected = shape_of(sample)
    found = shape_of(captured)
    findings = [
        f"missing field `{path}`: the committed sample records a {expected[path]} there, "
        f"and the stack sent no such field"
        for path in sorted(set(expected) - set(found))
    ]
    findings.extend(
        f"unexpected field `{path}`: the stack sent a {found[path]} the committed "
        f"sample does not record"
        for path in sorted(set(found) - set(expected))
    )
    findings.extend(
        f"field `{path}` changed type: the committed sample records a {expected[path]}, "
        f"the stack sent a {found[path]}"
        for path in sorted(set(expected) & set(found))
        if differs_in_type(expected[path], found[path])
    )
    return findings


# ~~ capturing what the stack posts


@dataclass
class Capture:
    """What the stack posted, exactly as it arrived."""

    body: bytes = b""
    path: str = ""
    content_type: str = ""
    arrived: bool = False
    posts: list[bytes] = field(default_factory=list)

    def json(self) -> object:
        """The captured body, decoded.

        Raises:
            TierError: If what arrived was not JSON.
        """
        try:
            return json.loads(self.body.decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            message = f"the stack posted something that is not JSON: {error}"
            raise TierError(message) from error


class CaptureServer:
    """The address the stack's notification plugin posts to, and what it caught."""

    def __init__(self, host: str, port: int) -> None:
        """Listen on the host and port the stack was configured with."""
        self.capture = Capture()
        self.caught = threading.Event()
        capture, caught = self.capture, self.caught

        class Handler(BaseHTTPRequestHandler):
            """One handler, which records the first body and accepts every one."""

            def do_POST(self) -> None:
                """Record what arrived and answer, so the producer sees a delivery."""
                length = int(self.headers.get("Content-Length", "0"))
                body = self.rfile.read(length)
                capture.posts.append(body)
                if not capture.arrived:
                    capture.body = body
                    capture.path = self.path
                    capture.content_type = self.headers.get("Content-Type", "")
                    capture.arrived = True
                    caught.set()
                self.send_response(200)
                self.send_header("Content-Length", "2")
                self.end_headers()
                self.wfile.write(b"ok")

            def log_message(self, format: str, *args: object) -> None:
                """Say nothing: the tier's own report is the signal here."""

        self.server = ThreadingHTTPServer((host, port), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)

    def __enter__(self) -> CaptureServer:
        """Start listening."""
        self.thread.start()
        return self

    def __exit__(self, *_: object) -> None:
        """Stop listening, leaving no thread behind."""
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=10)

    def wait(self, timeout: float) -> Capture:
        """Answer once a body has arrived.

        Raises:
            TierError: If none does.
        """
        if not self.caught.wait(timeout):
            message = (
                f"no webhook body arrived within {timeout:.0f}s: the stack was configured "
                f"to post here, so either it raised no alert or it could not reach this "
                f"address from its container"
            )
            raise TierError(message)
        return self.capture


# ~~ causing the alert


ALERT_SCRIPT = """
from app.models import Printer
from api.octoprint_views import alert_if_needed

printer = Printer.objects.get(name={name!r})
pic = printer.pic
if not pic or not pic.get('img_url'):
    raise SystemExit('the printer has no snapshot to alert on')
alert_if_needed(printer, pic['img_url'])
print('OBICO_TIER_ALERTED')
"""

ALERTED = "OBICO_TIER_ALERTED"


def cause_alert_on_stack(state_dir: Path) -> str:
    """Cause a real failure alert on the self-hosted stack under `state_dir`.

    The snapshot goes in through Obico's own printer API, exactly as the agent
    beside a printer posts one, and the alert is then raised through Obico's own
    `alert_if_needed` — the function its detection pipeline calls the moment a
    frame scores as a failure. Everything after that point is the stack's:
    Obico's models, its queue, its worker and its own webhook plugin.

    Returns:
        What causing it said, for the report.

    Raises:
        TierError: If the stack is not there, or will not raise the alert.
    """
    record_path = state_dir / "instance.json"
    if not record_path.is_file():
        message = f"{record_path} is not there: run `just obico-up` before this tier"
        raise TierError(message)
    record = json.loads(record_path.read_text(encoding="utf-8"))
    stack = Stack(Path(record["state_dir"]), str(record["project"]))

    posted = post_snapshot(str(record["url"]), str(record["printer_token"]))
    note(f"the printer posted a snapshot to {record['url']} ({posted})")

    answered = run_program(
        stack.compose("exec", "-T", "web", "python", "manage.py", "shell"),
        timeout=600,
        stdin=ALERT_SCRIPT.format(name=PRINTER_NAME),
    )
    if ALERTED not in answered.stdout:
        message = f"the stack did not raise a failure alert:\n{answered.stdout}"
        raise TierError(message)
    return answered.stdout.strip()


def post_snapshot(url: str, printer_token: str) -> str:
    """Post the snapshot through Obico's own printer API.

    Raises:
        TierError: If the stack refuses it.
    """
    boundary = "----printobserver-obico-tier"
    image = SNAPSHOT.read_bytes()
    body = b"".join(
        [
            f"--{boundary}\r\n".encode(),
            b'Content-Disposition: form-data; name="viewing_boost"\r\n\r\ntrue\r\n',
            f"--{boundary}\r\n".encode(),
            b'Content-Disposition: form-data; name="pic"; filename="snapshot.jpg"\r\n',
            b"Content-Type: image/jpeg\r\n\r\n",
            image,
            f"\r\n--{boundary}--\r\n".encode(),
        ]
    )
    try:
        status, _, _ = http(
            f"{url}/api/v1/octo/pic/",
            data=body,
            method="POST",
            headers={
                "Authorization": f"Token {printer_token}",
                "Content-Type": f"multipart/form-data; boundary={boundary}",
            },
        )
    except (OSError, ValueError) as error:
        message = f"the stack refused the printer's snapshot: {error}"
        raise TierError(message) from error
    return f"{status}, {len(image)} bytes of snapshot"


def ask_to_post(control_url: str) -> str:
    """Ask a stand-in for the stack to post, so the tier can be driven over one.

    Raises:
        TierError: If it will not.
    """
    try:
        status, _, _ = http(control_url, data=b"", method="POST")
    except (OSError, ValueError) as error:
        message = f"the stand-in at {control_url} would not post: {error}"
        raise TierError(message) from error
    return f"{status}"


def cause_alert(trigger: str) -> str:
    """Cause the alert the way the trigger says.

    Raises:
        TierError: If the trigger is not one this tier knows.
    """
    kind, _, rest = trigger.partition(":")
    if kind == "compose":
        return cause_alert_on_stack(Path(rest))
    if kind == "post":
        return ask_to_post(rest)
    message = f"`{trigger}` is not a trigger this tier knows (expected `compose:` or `post:`)"
    raise TierError(message)


# ~~ the image the captured body points at


def fetch_image(body: object) -> dict[str, Any]:
    """Retrieve the image the captured body points at, and read its bytes.

    A body that carries no reachable `img_url` is reported rather than raised
    on: a renamed or removed `img_url` is exactly the kind of move this tier
    exists to name, and a tier that fell over on it would report no verdict.
    """
    url = body.get("img_url") if isinstance(body, dict) else None
    if not isinstance(url, str) or not url:
        return {"url": None, "retrieved": False, "why": "the captured body carries no `img_url`"}
    try:
        status, content, content_type = http(url)
    except (OSError, ValueError) as error:
        return {"url": url, "retrieved": False, "why": f"{type(error).__name__}: {error}"}
    return {
        "url": url,
        "retrieved": True,
        "status": status,
        "bytes": len(content),
        "content_type": content_type,
    }


# ~~ the tier


def sample_body(path: Path) -> object:
    """The committed sample this tier holds the producer to.

    Raises:
        TierError: If it is not there or is not JSON.
    """
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        message = f"the committed sample {path} could not be read: {error}"
        raise TierError(message) from error


def listen_address(webhook_url: str) -> tuple[str, int, str]:
    """The host, port and path the stack was configured to post to.

    The host a *container* names is not one this process can bind — the webhook
    is delivered from inside the stack, so the name in the URL is the stack's
    route back to this host. The tier therefore listens on every interface,
    which is what the empty host below asks for, and serves the path the URL
    names.

    Raises:
        TierError: If the address carries no port to listen on.
    """
    parts = urllib.parse.urlsplit(webhook_url)
    if not parts.port:
        message = f"`{webhook_url}` names no port for this tier to listen on"
        raise TierError(message)
    return "", parts.port, parts.path or "/"


def reconcile(webhook_url: str, trigger: str, sample: Path, timeout: float) -> dict[str, Any]:
    """Walk the whole path and answer with the verdict.

    Raises:
        TierError: If the tier cannot reach a verdict.
    """
    expected = sample_body(sample)
    host, port, path = listen_address(webhook_url)
    note(f"listening for the webhook on every interface, port {port}, path {path}")
    with CaptureServer(host, port) as server:
        note(f"causing a failure alert ({trigger})")
        caused = cause_alert(trigger)
        captured = server.wait(timeout)
    note(f"captured {len(captured.body)} bytes posted to {captured.path}")

    # The one body: what the capture produced is what the image is fetched for
    # and what the comparison is given. Nothing below re-reads the stack.
    body = captured.json()
    image = fetch_image(body)
    note(
        f"the image the captured body points at: {image.get('bytes', 0)} bytes"
        if image["retrieved"]
        else f"the image could not be retrieved: {image['why']}"
    )
    found = differences(expected, body)
    return {
        "verdict": DIFFERS if found else AGREES,
        "differences": found,
        "sample": str(sample),
        "webhook_url": webhook_url,
        "trigger": trigger,
        "caused": caused,
        "captured": body,
        "captured_bytes": len(captured.body),
        "image": image,
    }


def argument_parser() -> argparse.ArgumentParser:
    """The command surface, whose defaults every recipe and caller shares."""
    parser = argparse.ArgumentParser(prog="obico-tier", description=__doc__)
    parser.add_argument("command", choices=("reconcile",))
    parser.add_argument(
        "--webhook-url",
        default="",
        help="the address the stack's notification plugin was configured to post to",
    )
    parser.add_argument(
        "--trigger",
        default="",
        help="how to cause the alert: `compose:<state-dir>` or `post:<control-url>`",
    )
    parser.add_argument(
        "--state-dir",
        default="",
        help=(
            "read `--webhook-url` and `--trigger compose:` from the record `up` wrote "
            "under this directory"
        ),
    )
    parser.add_argument(
        "--sample",
        default=str(REPOSITORY_ROOT / SAMPLE),
        help="the committed sample to reconcile against (default: %(default)s)",
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=180.0,
        help="seconds to wait for the stack to post (default: %(default)s)",
    )
    return parser


def resolved(arguments: argparse.Namespace) -> tuple[str, str]:
    """The webhook address and the trigger, taking a state directory as both.

    Raises:
        TierError: If neither the record nor the arguments say what to drive.
    """
    if arguments.state_dir:
        record_path = Path(arguments.state_dir) / "instance.json"
        if not record_path.is_file():
            message = f"{record_path} is not there: run `just obico-up` before this tier"
            raise TierError(message)
        record = json.loads(record_path.read_text(encoding="utf-8"))
        return str(record["webhook_url"]), f"compose:{arguments.state_dir}"
    if not arguments.webhook_url or not arguments.trigger:
        message = "pass `--state-dir`, or both `--webhook-url` and `--trigger`"
        raise TierError(message)
    return str(arguments.webhook_url), str(arguments.trigger)


def report(verdict: dict[str, Any]) -> None:
    """Say the verdict on stdout, and name every field that moved on stderr."""
    print(json.dumps(verdict, indent=2))
    if verdict["verdict"] == AGREES:
        note("the running Obico still posts what the committed sample records")
        return
    note(f"the running Obico has moved {len(verdict['differences'])} field(s):")
    for difference in verdict["differences"]:
        note(f"  {difference}")


def main(argv: list[str] | None = None) -> int:
    """Run the tier, answering 0 when it agrees, 1 when it differs, 2 on an error."""
    arguments = argument_parser().parse_args(argv)
    started = time.monotonic()
    try:
        webhook_url, trigger = resolved(arguments)
        verdict = reconcile(webhook_url, trigger, Path(arguments.sample), arguments.timeout)
    except TierError as error:
        print(f"obico-tier: reached no verdict: {error}", file=sys.stderr)
        return 2
    verdict["seconds"] = round(time.monotonic() - started, 1)
    report(verdict)
    return 0 if verdict["verdict"] == AGREES else 1


if __name__ == "__main__":
    raise SystemExit(main())
