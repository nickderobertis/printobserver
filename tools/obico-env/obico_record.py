#!/usr/bin/env python3
"""Record Obico's printer answer, the body the server journeys' REST double serves.

The detector status reads one `GET /api/v1/printers/{id}/` per observation
interval under the `[obico] access_token`, sent as `Authorization: Bearer`. The
double those journeys run against serves a recording of that answer, and this is
the command that makes it: re-running it against the stack `obico_env.py up`
left is how the recording is compared with the real Obico again.

    obico_record.py --state-dir .obico-env
    obico_record.py --url URL --printer-id ID     (bearer from OBICO_RECORD_ACCESS_TOKEN)

Against a state directory it does what a user and the agent beside a printer do,
through Obico's own code: the printer posts a snapshot through Obico's printer
API, so the answer carries one, and an access token is minted for the
provisioned account through Obico's own OAuth2 models — the kind of token
`[obico] access_token` holds. Then it reads the answer with that token as a
bearer, and nothing else. Given an address instead, it reads whatever Obico
answers there, with the bearer the environment variable holds.

The answer carries the printer's own credential, `auth_token`. Every value under
a credential's key is replaced with the sentinel, and so is every appearance of a
credential this command knows of anywhere else in the answer, before anything is
written; an answer that is not `200`, not a JSON object, or missing a field the
status reads is refused and nothing is written. Beside the recording it writes
where it came from: the revision, the request path, the authentication scheme,
the status, and this command.

Exit status is 0 when it recorded, and 2 when it did not, saying why on stderr.
Credentials are never printed.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import urllib.error
from pathlib import Path
from typing import Any

from obico_env import ACCOUNT_EMAIL, OBICO_REVISION, Stack, note, run_program
from obico_tier import PrinterToken, TierError, http, post_snapshot, stack_under

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

#: The fields the detector status reads, which an answer must carry to be recorded.
STATUS_FIELDS = ("id", "current_print", "pic", "normalized_p")

#: The variable the bearer is read from when no state directory mints one: an
#: argument would put the credential in the process table.
BEARER_VARIABLE = "OBICO_RECORD_ACCESS_TOKEN"

# Run inside the web container through Obico's own `manage.py shell`: mint an
# OAuth2 access token for the provisioned account through the same models
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
    scope='read write',
)
print('OBICO_RECORD_MINTED ' + token.token)
"""

MINTED = "OBICO_RECORD_MINTED "


class RecordError(Exception):
    """Nothing was recorded, with what stopped it."""


def mint_access_token(stack: Stack) -> str:
    """An access token for the provisioned account, minted through Obico's own models.

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
            return line[len(MINTED) :].strip()
    # What Obico said is not echoed: a partial line could carry the token.
    message = "Obico's `manage.py shell` did not report a minted access token"
    raise RecordError(message)


def read_printer(url: str, printer_id: int, access_token: str) -> tuple[int, bytes]:
    """Obico's answer to the printer read, under the bearer, as status and body.

    Raises:
        RecordError: If Obico cannot be reached at all.
    """
    try:
        status, body, _ = http(
            f"{url.rstrip('/')}/api/v1/printers/{printer_id}/",
            headers={"Authorization": f"Bearer {access_token}", "Accept": "application/json"},
        )
    except urllib.error.HTTPError as refused:
        return refused.code, b""
    except (OSError, ValueError) as error:
        message = f"Obico at {url} could not be read: {error}"
        raise RecordError(message) from error
    return status, body


def sanitized(answer: object, known: tuple[str, ...]) -> tuple[object, list[str]]:
    """The answer with every credential replaced by the sentinel, and where each was.

    A value under a credential's key is replaced whole. A credential this command
    knows of — the printer's token, the bearer — is replaced wherever else it
    appears, inside a URL included.
    """
    replaced: list[str] = []

    def walk(value: object, where: str) -> object:
        if isinstance(value, dict):
            kept: dict[str, object] = {}
            for key, item in value.items():
                at = f"{where}.{key}" if where else key
                if key in CREDENTIAL_KEYS and item is not None:
                    replaced.append(at)
                    kept[key] = SENTINEL
                else:
                    kept[key] = walk(item, at)
            return kept
        if isinstance(value, list):
            return [walk(item, f"{where}[{index}]") for index, item in enumerate(value)]
        if isinstance(value, str):
            scrubbed = value
            for credential in known:
                if credential and credential in scrubbed:
                    scrubbed = scrubbed.replace(credential, SENTINEL)
            if scrubbed != value:
                replaced.append(where)
            return scrubbed
        return value

    return walk(answer, ""), replaced


def record(
    url: str,
    printer_id: int,
    access_token: str,
    out: Path,
    *,
    known: tuple[str, ...] = (),
) -> dict[str, Any]:
    """Read the printer answer under the bearer and write it, sanitized, with its provenance.

    Raises:
        RecordError: If the answer is not one to record; nothing is written then.
    """
    path = f"/api/v1/printers/{printer_id}/"
    status, body = read_printer(url, printer_id, access_token)
    if status != 200:
        message = f"Obico answered {path} {status} under the bearer access token, not 200"
        raise RecordError(message)
    try:
        answer = json.loads(body)
    except ValueError as error:
        message = f"Obico's answer to {path} is not JSON: {error}"
        raise RecordError(message) from error
    if not isinstance(answer, dict):
        message = f"Obico's answer to {path} is not a JSON object"
        raise RecordError(message)
    missing = [name for name in STATUS_FIELDS if answer.get(name) is None]
    if missing:
        message = (
            f"Obico's answer to {path} carries no {', '.join(missing)}: the stack has no "
            "running print with a snapshot, so it is not the answer the double serves"
        )
        raise RecordError(message)
    clean, replaced = sanitized(answer, (access_token, *known))
    written = json.dumps(clean)
    if any(credential and credential in written for credential in (access_token, *known)):
        message = f"a credential survived sanitizing the answer to {path}; nothing was written"
        raise RecordError(message)
    provenance = {
        "obico_revision": OBICO_REVISION,
        "request": f"GET {path}",
        "authentication": "Authorization: Bearer <[obico] access_token>",
        "status": status,
        "sentinel": SENTINEL,
        "credentials_replaced": replaced,
        "recorded_by": "uv run python tools/obico-env/obico_record.py --state-dir <state-dir>",
    }
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(clean, indent=2) + "\n", encoding="utf-8")
    provenance_path = out.with_suffix(".provenance.json")
    provenance_path.write_text(json.dumps(provenance, indent=2) + "\n", encoding="utf-8")
    note(f"recorded {path} ({status}) to {out}, replacing {len(replaced)} credential(s)")
    return {"recording": str(out), "provenance": str(provenance_path), **provenance}


def from_stack(state_dir: Path, out: Path) -> dict[str, Any]:
    """Post a snapshot, mint a bearer, and record, against the stack under `state_dir`.

    Raises:
        RecordError: If the stack is not there or will not answer.
    """
    try:
        instance = stack_under(state_dir)
    except TierError as error:
        raise RecordError(str(error)) from error
    written = json.loads((state_dir / "instance.json").read_text(encoding="utf-8"))
    printer_id = written.get("printer_id")
    if not isinstance(printer_id, int):
        message = f"{state_dir / 'instance.json'} names no printer; run `just obico-up` again"
        raise RecordError(message)
    try:
        posted = post_snapshot(instance.url, instance.printer_token)
    except TierError as error:
        raise RecordError(str(error)) from error
    note(f"the printer posted a snapshot to {instance.url} ({posted})")
    token = mint_access_token(instance.stack)
    known: tuple[str, ...] = (PrinterToken(instance.printer_token),)
    return record(instance.url, printer_id, token, out, known=known)


def argument_parser() -> argparse.ArgumentParser:
    """The command surface."""
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
    """Record once, answering 0 when it did and 2 when it did not."""
    arguments = argument_parser().parse_args(argv)
    out = Path(arguments.out)
    try:
        if arguments.state_dir:
            answer = from_stack(Path(arguments.state_dir), out)
        elif arguments.url and arguments.printer_id:
            token = os.environ.get(BEARER_VARIABLE, "")
            if not token:
                message = f"--url reads with the bearer {BEARER_VARIABLE} holds; it is unset"
                raise RecordError(message)
            answer = record(arguments.url, arguments.printer_id, token, out)
        else:
            message = "pass `--state-dir`, or both `--url` and `--printer-id`"
            raise RecordError(message)
    except RecordError as error:
        print(f"obico-record: recorded nothing: {error}", file=sys.stderr)
        return 2
    print(json.dumps(answer, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
