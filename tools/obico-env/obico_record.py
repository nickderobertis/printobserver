#!/usr/bin/env python3
"""Record Obico's printer answer, the body the server journeys' REST double serves.

The detector status reads one `GET /api/v1/printers/{id}/` per observation
interval under the `[obico] access_token`, sent as `Authorization: Bearer`. The
double those journeys run against serves a recording of that answer, and this is
the command that makes it: re-running it against the stack `obico_env.py up`
left is how the recording is compared with the real Obico again, and the
scheduled Obico tier does exactly that.

    obico_record.py --state-dir .obico-env
    obico_record.py --url URL --printer-id ID     (bearer from OBICO_RECORD_ACCESS_TOKEN)

Against a state directory it does what a user and the agent beside a printer do,
through Obico's own code: the printer posts a snapshot through Obico's printer
API, so the answer carries one, and a read-only access token is minted for the
provisioned account through Obico's own OAuth2 models — the kind of token
`[obico] access_token` holds. Then it reads the answer with that token as a
bearer, and nothing else. Given an address instead, it reads whatever Obico
answers there, with the bearer the environment variable holds.

The answer carries the printer's own credential, `auth_token`. Every value under
a credential's key is replaced with the sentinel, and so is every appearance of a
credential this command knows of anywhere else in the answer, before anything is
written. An answer that is not `200`, not a JSON object, or not the printer
asked for with a running print and a snapshot, is refused and nothing is
written. Beside the recording it writes where it came
from: the revision, the request, the authentication scheme, the status and this
command. It then compares the shape it recorded — the fields and the JSON type of
each, as the tier compares the webhook body — with the committed recording's.

Exit status is 0 when it recorded a shape the committed recording agrees with, 1
when it recorded one that differs, naming every field that moved, and 2 when it
recorded nothing, saying why on stderr. Credentials are never printed.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import urllib.error
from dataclasses import dataclass
from pathlib import Path
from typing import Literal, NewType, TypedDict

from obico_env import ACCOUNT_EMAIL, OBICO_REVISION, Stack, note, run_program
from obico_tier import (
    PrinterId,
    TierError,
    differences,
    http,
    is_service_address,
    positive_printer,
    post_snapshot,
    stack_under,
)

REPOSITORY_ROOT = Path(__file__).resolve().parents[2]

#: Where the recording goes, relative to the repository root: beside the double
#: that serves it.
RECORDING = "crates/printobserver-server/tests/support/obico-printer.json"

#: The value every credential in the answer is replaced with. The journeys look
#: for it everywhere the server writes, so it is distinctive rather than empty.
SENTINEL = "SENTINEL-obico-printer-auth-token-0b6f3c"

#: The keys whose values are credentials wherever they appear in the answer.
CREDENTIAL_KEYS = frozenset(
    {"auth_token", "access_token", "refresh_token", "token", "password", "secret", "api_key"}
)

#: The variable the bearer is read from when no state directory mints one: an
#: argument would put the credential in the process table.
BEARER_VARIABLE = "OBICO_RECORD_ACCESS_TOKEN"

#: An OAuth2 access token of Obico's, sent as a bearer. A credential, so never reported.
AccessToken = NewType("AccessToken", str)

# Run inside the web container through Obico's own `manage.py shell`: mint a
# read-only OAuth2 access token for the provisioned account through the models
# Obico's own authorization flow writes, reusing one application across runs.
MINT_SCRIPT = """
import json, secrets
from datetime import timedelta
from django.utils import timezone
from oauth2_provider.models import get_access_token_model, get_application_model
from app.models import User

request = json.loads({request!r})
user = User.objects.get(email=request['email'])
Application = get_application_model()
application, _ = Application.objects.get_or_create(
    user=user,
    name='printobserver-record',
    defaults=dict(
        client_type=Application.CLIENT_CONFIDENTIAL,
        authorization_grant_type=Application.GRANT_AUTHORIZATION_CODE,
        redirect_uris='http://127.0.0.1/',
    ),
)
token = get_access_token_model().objects.create(
    user=user,
    application=application,
    token=secrets.token_urlsafe(30),
    expires=timezone.now() + timedelta(minutes=10),
    scope='read',
)
print('OBICO_RECORD_MINTED ' + token.token)
"""

MINTED = "OBICO_RECORD_MINTED "

#: The instants Obico sets on a print once it is no longer running.
ENDED = ("ended_at", "finished_at", "cancelled_at")


class RecordError(Exception):
    """Nothing was recorded, with what stopped it."""


@dataclass(frozen=True)
class Answered:
    """What Obico answered the printer read: its status and its body."""

    status: int
    body: bytes


class Provenance(TypedDict):
    """Where a recording came from, written beside it."""

    obico_revision: str
    request: str
    authentication: str
    status: int
    sentinel: str
    credentials_replaced: list[str]
    recorded_by: str


class Recorded(Provenance):
    """What a run reports: the provenance, where it wrote, and how the shape compares."""

    recording: str
    provenance: str
    compared_with: str
    verdict: Literal["agrees", "differs"]
    differences: list[str]


def mint_access_token(stack: Stack) -> AccessToken:
    """A read-only access token for the provisioned account, minted through Obico's own models.

    Raises:
        RecordError: If Obico does not say it minted one.
    """
    answered = run_program(
        stack.compose("exec", "-T", "web", "python", "manage.py", "shell"),
        timeout=600,
        stdin=MINT_SCRIPT.format(request=json.dumps({"email": ACCOUNT_EMAIL})),
    )
    for line in answered.stdout.splitlines():
        if line.startswith(MINTED) and line[len(MINTED) :].strip():
            return AccessToken(line[len(MINTED) :].strip())
    # What Obico said is not echoed: a partial line could carry the token.
    message = "Obico's `manage.py shell` did not report a minted access token"
    raise RecordError(message)


def read_printer(url: str, printer: PrinterId, bearer: AccessToken) -> Answered:
    """Obico's answer to the printer read under the bearer.

    Raises:
        RecordError: If Obico cannot be reached at all.
    """
    try:
        # llmlint: ignore[async_typed_clients_at_boundaries] suppressions.toml has the reason.
        status, body, _ = http(
            f"{url.rstrip('/')}/api/v1/printers/{printer}/",
            headers={"Authorization": f"Bearer {bearer}", "Accept": "application/json"},
        )
    except urllib.error.HTTPError as refused:
        return Answered(refused.code, b"")
    except (OSError, ValueError) as error:
        message = f"Obico at {url} could not be read: {error}"
        raise RecordError(message) from error
    return Answered(status, body)


def sanitized(answer: object, known: tuple[str, ...]) -> tuple[object, list[str]]:
    """The answer with every credential replaced by the sentinel, and where each was.

    A value under a credential's key is replaced whole. A credential this command
    knows of — the printer's token, the bearer — is replaced wherever else it
    appears, inside a URL included.
    """
    replaced: list[str] = []

    def walk(value: object, where: str) -> object:
        match value:
            case dict():
                kept: dict[object, object] = {}
                for key, item in value.items():
                    at = f"{where}.{key}" if where else str(key)
                    if key in CREDENTIAL_KEYS and item is not None:
                        replaced.append(at)
                        kept[key] = SENTINEL
                    else:
                        kept[key] = walk(item, at)
                return kept
            case list():
                return [walk(item, f"{where}[{index}]") for index, item in enumerate(value)]
            case str():
                scrubbed = value
                for credential in known:
                    if credential:
                        scrubbed = scrubbed.replace(credential, SENTINEL)
                if scrubbed != value:
                    replaced.append(where)
                return scrubbed
            case _:
                return value

    return walk(answer, ""), replaced


def not_a_running_print_with_a_snapshot(answer: dict[str, object], printer: PrinterId) -> list[str]:
    """What keeps the answer from being this printer's running print with a snapshot.

    Only that state is checked here. Whether the recording carries every field the
    detector status reads, in the form it reads it, is the server's own reader's to
    say, and the server journeys say it by parsing this recording with that reader.
    """
    lacking: list[str] = []
    if positive_printer(answer.get("id")) != printer:
        lacking.append(f"id {printer}")
    current = answer.get("current_print")
    if (
        not isinstance(current, dict)
        or positive_printer(current.get("id")) is None
        or any(current.get(end) is not None for end in ENDED)
    ):
        lacking.append("running current_print")
    pic = answer.get("pic")
    if not isinstance(pic, dict) or not isinstance(pic.get("img_url"), str) or not pic["img_url"]:
        lacking.append("pic.img_url")
    return lacking


def record(
    url: str,
    printer: PrinterId,
    bearer: AccessToken,
    out: Path,
    *,
    known: tuple[str, ...] = (),
) -> Recorded:
    """Read the printer answer under the bearer and write it, sanitized, with its provenance.

    Raises:
        RecordError: If the answer is not one to record; nothing is written then.
    """
    path = f"/api/v1/printers/{printer}/"
    answered = read_printer(url, printer, bearer)
    if answered.status != 200:
        message = f"Obico answered {path} {answered.status} under the bearer access token, not 200"
        raise RecordError(message)
    try:
        answer = json.loads(answered.body)
    except ValueError as error:
        message = f"Obico's answer to {path} is not JSON: {error}"
        raise RecordError(message) from error
    if not isinstance(answer, dict):
        message = f"Obico's answer to {path} is not a JSON object"
        raise RecordError(message)
    lacking = not_a_running_print_with_a_snapshot(answer, printer)
    if lacking:
        message = (
            f"Obico's answer to {path} carries no {', '.join(lacking)}: it is not this "
            "printer's running print with a snapshot, which is what the double serves"
        )
        raise RecordError(message)
    credentials = (bearer, *known)
    clean, replaced = sanitized(answer, credentials)
    written = json.dumps(clean, indent=2) + "\n"
    if any(credential and credential in written for credential in credentials):
        message = f"a credential survived sanitizing the answer to {path}; nothing was written"
        raise RecordError(message)
    committed = REPOSITORY_ROOT / RECORDING
    try:
        before = json.loads(committed.read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        message = f"the committed recording {RECORDING} could not be read: {error}"
        raise RecordError(message) from error
    provenance = Provenance(
        obico_revision=OBICO_REVISION,
        request=f"GET {path}",
        authentication="Authorization: Bearer <[obico] access_token>",
        status=answered.status,
        sentinel=SENTINEL,
        credentials_replaced=replaced,
        recorded_by="uv run python tools/obico-env/obico_record.py --state-dir <state-dir>",
    )
    provenance_path = out.with_suffix(".provenance.json")
    try:
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(written, encoding="utf-8")
        provenance_path.write_text(json.dumps(provenance, indent=2) + "\n", encoding="utf-8")
    except OSError as error:
        message = f"the recording could not be written to {out}: {error}"
        raise RecordError(message) from error
    moved = differences(before, clean)
    note(f"recorded {path} ({answered.status}) to {out}, replacing {len(replaced)} credential(s)")
    return Recorded(
        **provenance,
        recording=str(out),
        provenance=str(provenance_path),
        compared_with=RECORDING,
        verdict="differs" if moved else "agrees",
        differences=moved,
    )


def from_stack(state_dir: Path, out: Path) -> Recorded:
    """Post a snapshot, mint a bearer, and record, against the stack under `state_dir`.

    Raises:
        RecordError: If the stack is not there or will not answer.
    """
    try:
        instance = stack_under(state_dir)
        if instance.printer_id is None:
            message = f"{state_dir / 'instance.json'} names no printer; run `just obico-up` again"
            raise RecordError(message)
        # llmlint: ignore[async_typed_clients_at_boundaries] suppressions.toml has the reason.
        posted = post_snapshot(instance.url, instance.printer_token)
    except TierError as error:
        raise RecordError(str(error)) from error
    note(f"the printer posted a snapshot to {instance.url} ({posted})")
    bearer = mint_access_token(instance.stack)
    return record(instance.url, instance.printer_id, bearer, out, known=(instance.printer_token,))


def from_address(url: str, printer: int, out: Path) -> Recorded:
    """Record from an Obico named by its address, with the bearer the environment holds.

    Raises:
        RecordError: If the address, the printer or the bearer is not one to use.
    """
    if not is_service_address(url):
        message = f"{url!r} is no http(s) address with a host and port"
        raise RecordError(message)
    identified = positive_printer(printer)
    if identified is None:
        message = f"--printer-id {printer} is not a positive integer"
        raise RecordError(message)
    bearer = os.environ.get(BEARER_VARIABLE, "")
    if not bearer:
        message = f"--url reads with the bearer {BEARER_VARIABLE} holds; it is unset"
        raise RecordError(message)
    return record(url, identified, AccessToken(bearer), out)


def argument_parser() -> argparse.ArgumentParser:
    """Two ways to name the Obico to read, and where the recording goes."""
    parser = argparse.ArgumentParser(prog="obico-record", description=__doc__)
    parser.add_argument("--state-dir", default="", help="the stack `obico_env.py up` left")
    parser.add_argument("--url", default="", help="an Obico to read, in place of a state dir")
    parser.add_argument("--printer-id", type=int, default=0, help="the printer to read at --url")
    parser.add_argument(
        "--out",
        default=str(REPOSITORY_ROOT / RECORDING),
        help="where the recording goes (default: %(default)s)",
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    """Record once: 0 when the shape agrees, 1 when it differs, 2 when nothing was recorded."""
    arguments = argument_parser().parse_args(argv)
    out = Path(arguments.out)
    try:
        if arguments.state_dir:
            recorded = from_stack(Path(arguments.state_dir), out)
        elif arguments.url or arguments.printer_id:
            recorded = from_address(arguments.url, arguments.printer_id, out)
        else:
            message = "pass `--state-dir`, or both `--url` and `--printer-id`"
            raise RecordError(message)
    except RecordError as error:
        print(f"obico-record: recorded nothing: {error}", file=sys.stderr)
        return 2
    print(json.dumps(recorded, indent=2))
    for moved in recorded["differences"]:
        note(f"  moved since the committed recording: {moved}")
    return 0 if recorded["verdict"] == "agrees" else 1


if __name__ == "__main__":
    raise SystemExit(main())
