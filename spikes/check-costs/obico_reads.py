#!/usr/bin/env python3
"""Time the REST reads that give the active print's latest Obico prediction,
against the self-hosted Obico `tools/obico-env/obico_env.py up` left running.

Usage: obico_reads.py OBICO_STATE_DIR [N]

Setting up (through the stack's own `manage.py shell` and printer API, the way
`obico_tier.py` does): give the provisioned account a known password (the
bring-up keeps none, and the printer API takes a Django session or an OAuth2
token, never a per-user API token), start a print on the printer, and post one
snapshot through `/api/v1/octo/pic/` so Obico's own detector scores it.

Then, signed in by session, N reads each of:

- `GET /api/v1/printers/{id}/` — the printer, its `current_print` and the live
  `normalized_p` of its latest prediction;
- `GET /api/v1/prints/{id}/prediction_json/` — one print's per-frame
  prediction list, which Obico fills only when the print ends.

Prints one JSON document.
"""

import json
import statistics
import sys
import time
import urllib.parse
import urllib.request
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "tools" / "obico-env"))
import obico_tier  # noqa: E402
from obico_env import run_program  # noqa: E402

PASSWORD = "check-costs-spike-only"
SETUP = """
from django.utils import timezone
from app.models import Printer, Print, User
user = User.objects.get(email={email!r})
user.set_password({password!r}); user.save()
printer = Printer.objects.get(name={name!r})
current = printer.current_print
if current is None or current.finished_at or current.cancelled_at:
    started = timezone.now()
    current = Print.objects.create(user=user, printer=printer, filename='spike-long.gcode',
                                   started_at=started, ext_id=int(started.timestamp() * 1000))
    printer.current_print = current
    printer.save()
print('SPIKE_PRINT', current.id, printer.id)
"""


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *_):
        return None


def cookies_of(headers):
    return {
        value.split("=", 1)[0]: value.split(";", 1)[0].split("=", 1)[1]
        for value in headers.get_all("Set-Cookie") or []
    }


def signed_in(base):
    """An opener carrying a Django session for the provisioned account.

    Cookies are carried by hand: `http.cookiejar` files a bare `localhost`
    under `localhost.local` and then sends the session to nobody.
    """
    plain = urllib.request.build_opener(NoRedirect)
    with plain.open(f"{base}/accounts/login/", timeout=30) as page:
        jar = cookies_of(page.headers)
    form = urllib.parse.urlencode(
        {"login": "printobserver@example.invalid", "password": PASSWORD,
         "csrfmiddlewaretoken": jar["csrftoken"]}
    ).encode()
    login = urllib.request.Request(
        f"{base}/accounts/login/",
        data=form,
        headers={"Referer": f"{base}/accounts/login/",
                 "Cookie": "; ".join(f"{k}={v}" for k, v in jar.items())},
    )
    try:
        plain.open(login, timeout=30)
    except urllib.error.HTTPError as answer:  # the 302 a successful sign-in answers
        jar.update(cookies_of(answer.headers))
    opener = urllib.request.build_opener()
    opener.addheaders = [("Cookie", "; ".join(f"{k}={v}" for k, v in jar.items()))]
    return opener


def get(open_, url):
    started = time.perf_counter()
    try:
        with open_.open(url, timeout=30) as answer:
            body, status = answer.read(), answer.status
    except urllib.error.HTTPError as error:
        body, status = error.read(), error.code
    return status, body, time.perf_counter() - started


def series(open_, url, n):
    runs = [get(open_, url) for _ in range(n)]
    times = sorted(r[2] for r in runs)
    return {
        "path": urllib.parse.urlparse(url).path,
        "status": sorted({r[0] for r in runs}),
        "n": n,
        "median_ms": round(statistics.median(times) * 1000, 2),
        "max_ms": round(times[-1] * 1000, 2),
        "bytes": len(runs[-1][1]),
    }, runs[-1][1]


def main():
    state_dir = Path(sys.argv[1])
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 20
    instance = obico_tier.stack_under(state_dir)
    base = instance.url.rstrip("/")
    answered = run_program(
        instance.stack.compose("exec", "-T", "web", "python", "manage.py", "shell"),
        timeout=600,
        stdin=SETUP.format(
            email="printobserver@example.invalid", password=PASSWORD, name="printobserver"
        ),
    )
    marker = [line for line in answered.stdout.splitlines() if line.startswith("SPIKE_PRINT")]
    if not marker:
        sys.exit(f"the stack did not start a print:\n{answered.stdout}")
    posted = obico_tier.post_snapshot(base, instance.printer_token)
    open_ = signed_in(base)
    status, body, _ = get(open_, f"{base}/api/v1/printers/")
    printers = json.loads(body) if status == 200 else []
    if not printers:
        sys.exit(f"no printer is readable ({status}): {body[:300]!r}")
    printer_id = printers[0]["id"]
    printer_read, printer_body = series(open_, f"{base}/api/v1/printers/{printer_id}/", n)
    detail = json.loads(printer_body)
    current = detail.get("current_print") or {}
    out = {
        "obico_revision": "49c0bc7001a3fd8d56297fc3032ba287bfe1d50b",
        "snapshot_posted": posted,
        "auth": "Django session from POST /accounts/login/ (the printer API takes a session or an OAuth2 token)",
        "printer_read": printer_read,
        "printer_fields": sorted(detail),
        "normalized_p": detail.get("normalized_p"),
        "current_print_fields": sorted(current),
        "current_print_id": current.get("id"),
    }
    if current.get("id"):
        pj_read, pj_body = series(
            open_, f"{base}/api/v1/prints/{current['id']}/prediction_json/", n
        )
        out["prediction_json_read"] = pj_read
        out["prediction_json_answer_for_running_print"] = json.loads(pj_body or b"null")
    unauth = get(urllib.request.build_opener(), f"{base}/api/v1/printers/{printer_id}/")
    out["printer_read_with_printer_token_or_none"] = {
        "no_credential_status": unauth[0],
        "printer_token_status": get_with_token(base, printer_id, instance.printer_token),
    }
    print(json.dumps(out))


def get_with_token(base, printer_id, token):
    request = urllib.request.Request(
        f"{base}/api/v1/printers/{printer_id}/", headers={"Authorization": f"Token {token}"}
    )
    try:
        with urllib.request.urlopen(request, timeout=30) as answer:
            return answer.status
    except urllib.error.HTTPError as error:
        return error.code


if __name__ == "__main__":
    main()
