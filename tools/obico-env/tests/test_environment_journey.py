"""The bring-up, unattended, and the bring-down that leaves nothing running.

This journey drives the real script exactly as `just obico-up` drives it, with
no interactive input at all, and then reads back with clients of this suite's
own: Obico's printer API over HTTP, and Obico's own models through a query this
suite writes. Asking the script whether it provisioned would be asking the thing
under test.

Whether the plugin posts to the address the caller named is not read out of a
setting either. The journey takes a free port of its own, names it, runs the real
tier against the real stack, and reads it out of a body arriving there.
"""

from __future__ import annotations

import json

from harness import (
    answer,
    ask_the_stack,
    containers,
    environment,
    free_port,
    printer_over_the_api,
    said,
    tier,
)
from repo_checks.expect import equal, passing, truth

# What the announcement must carry, said before anything is started.
ANNOUNCED_SERVICES = ("redis", "ml_api", "web", "tasks")
HOW_LONG = "roughly how long"
STARTING = "starting the"

ACCOUNT = "printobserver@example.invalid"

# A query of this suite's own, over Obico's own models: what the stack itself
# holds for the account, the printer's owner, and the plugin's configuration.
PROVISIONED_QUERY = """
import json
from app.models import User, Printer, NotificationSetting
user = User.objects.filter(email={account!r}).first()
printer = Printer.objects.filter(name='printobserver').first()
setting = NotificationSetting.objects.filter(name='webhook').first()
print('ASKED ' + json.dumps({{
    'accounts': User.objects.filter(email={account!r}).count(),
    'printer_owner': printer.user.email if printer else None,
    'printer_id': printer.id if printer else None,
    'webhook_enabled': bool(setting and setting.enabled),
    'webhook_on_failure': bool(setting and setting.notify_on_failure_alert),
    'webhook_url': (setting.config.get('custom_webhook_URL') if setting else None),
    'setting_owner': setting.user.email if setting else None,
}}))
"""
ASKED = "ASKED "


def _announcement(output: str) -> list[str]:
    """Everything the script said before it started its first container.

    Raises:
        AssertionError: If it never said it was starting them, so that the lines
            before that point cannot be identified at all.
    """
    lines = output.splitlines()
    for index, line in enumerate(lines):
        if STARTING in line and "containers" in line:
            return lines[:index]
    message = f"expected the script to say it was starting the containers; it said:\n{output}"
    raise AssertionError(message)


def _asked(state_dir: str) -> dict[str, object]:
    """What the stack itself holds, asked of its own models with this suite's query.

    Raises:
        AssertionError: If the stack answers nothing this can read.
    """
    output = ask_the_stack(state_dir, PROVISIONED_QUERY.format(account=ACCOUNT))
    for line in output.splitlines():
        if line.startswith(ASKED):
            return dict(json.loads(line[len(ASKED) :]))
    message = f"expected the stack to answer the query; it said:\n{output}"
    raise AssertionError(message)


def test_the_bring_up_says_what_it_starts_and_how_long_before_it_starts_anything(
    state_dir: str,
) -> None:
    """Read what it emits before its first container starts, and find both statements."""
    webhook = f"http://host.docker.internal:{free_port()}/alert"

    result = environment("up", "--state-dir", state_dir, "--webhook-url", webhook)

    passing(result, describing="the bring-up")
    before = _announcement(said(result))
    for service in ANNOUNCED_SERVICES:
        truth(
            any(service in line for line in before),
            describing=(
                f"the announcement to name the `{service}` service before anything "
                f"started; it said:\n" + "\n".join(before)
            ),
        )
    truth(
        any(HOW_LONG in line for line in before),
        describing=(
            "the announcement to say roughly how long starting takes; it said:\n"
            + "\n".join(before)
        ),
    )


def test_the_stack_carries_the_account_the_printer_and_the_configured_webhook(
    state_dir: str,
) -> None:
    """Unattended: an account, a registered printer, and a plugin posting where told."""
    webhook = f"http://host.docker.internal:{free_port()}/alert"

    brought_up = environment("up", "--state-dir", state_dir, "--webhook-url", webhook)

    passing(brought_up, describing="the bring-up")
    record = answer(brought_up)
    equal(record["webhook_url"], webhook, describing="the address the plugin was configured with")

    # The account is there, and it is the account the printer and the plugin
    # belong to — asked of Obico's own models with this suite's own query.
    held = _asked(state_dir)
    equal(held["accounts"], 1, describing="the accounts Obico holds for that address")
    equal(held["printer_owner"], ACCOUNT, describing="who Obico says the printer belongs to")
    equal(held["setting_owner"], ACCOUNT, describing="who Obico says the plugin belongs to")
    equal(held["webhook_url"], webhook, describing="the address Obico's plugin is configured with")
    truth(held["webhook_enabled"], describing="the webhook plugin to be enabled")
    truth(held["webhook_on_failure"], describing="the plugin to be enabled for failure alerts")

    # The printer is registered: Obico's own printer API answers for its token.
    known = printer_over_the_api(str(record["url"]), str(record["printer_token"]))
    equal(known["printer"]["id"], record["printer_id"], describing="the printer Obico knows")
    equal(
        known["printer"]["name"],
        record["printer_name"],
        describing="the name Obico knows the printer under",
    )

    # And the plugin posts *there*: read out of a body arriving at the address
    # the caller named, rather than out of the setting asserted above.
    verdict = _reconciled(state_dir, webhook)
    equal(verdict["webhook_url"], webhook, describing="the address the body arrived on")
    truth(verdict["captured"], describing="a body the stack posted to the caller's own address")


def _reconciled(state_dir: str, webhook: str) -> dict[str, object]:
    """Run the real tier against the real stack and hand back its verdict.

    Raises:
        AssertionError: If the tier reached no verdict at all.
    """
    result = tier(
        "reconcile",
        "--webhook-url",
        webhook,
        "--trigger",
        f"compose:{state_dir}",
        "--timeout",
        "240",
        timeout=1800,
    )
    if result.returncode == 2:
        message = f"expected the tier to reach a verdict; it did not:\n{said(result)}"
        raise AssertionError(message)
    return dict(json.loads(result.stdout))


def test_the_bring_down_leaves_no_container_of_the_stack_running(state_dir: str) -> None:
    """Whatever the bring-up created, the bring-down stops — asked of Docker itself."""
    webhook = f"http://host.docker.internal:{free_port()}/alert"
    brought_up = environment("up", "--state-dir", state_dir, "--webhook-url", webhook)
    passing(brought_up, describing="the bring-up")
    project = str(answer(brought_up)["project"])
    truth(containers(project), describing="containers of the stack running before the bring-down")

    brought_down = environment("down", "--state-dir", state_dir, timeout=1800)

    passing(brought_down, describing="the bring-down")
    equal(containers(project), [], describing="the containers of the stack still running")
