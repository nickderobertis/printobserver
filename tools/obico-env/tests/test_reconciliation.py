"""The tier's reconciliation, driven end to end over a stand-in for the Obico stack.

Each journey below alters the body the stand-in *posts*, so the alteration goes
through the tier's own capture — through the listener, the fetch of the image the
captured body points at, and the comparison — rather than past it. That ordering
is the whole point: a tier that captured the real payload and compared something
else would pass a suite that handed the comparator an altered body directly, and
would fail every journey here.

The four alterations are the four ways a producer moves a field: adding one,
renaming one, removing one, and changing the type of one. Each must fail the
tier naming the field that moved, and the unaltered body must pass.
"""

from __future__ import annotations

import json
import subprocess
from collections.abc import Callable
from typing import Any

from harness import SAMPLE, SNAPSHOT, answer, free_port, said, tier
from repo_checks.expect import contains, equal, failing, passing, refused_naming, truth
from substitute import Substitute

Alteration = Callable[[dict[str, Any]], None]


def _add_a_field(body: dict[str, Any]) -> None:
    """A field the producer started sending that the sample does not record."""
    body["event"]["severity"] = "high"


def _rename_a_field(body: dict[str, Any]) -> None:
    """A field the producer renamed, which is one name gone and one arrived."""
    body["image_url"] = body.pop("img_url")


def _remove_a_field(body: dict[str, Any]) -> None:
    """A field the producer stopped sending."""
    del body["print"]["filename"]


def _retype_a_field(body: dict[str, Any]) -> None:
    """A field the producer still sends, under a different JSON type."""
    body["printer"]["id"] = str(body["printer"]["id"])


def _run_over_a_substitute(
    alter: Alteration | None, image_url_field: str = "img_url"
) -> subprocess.CompletedProcess[str]:
    """Run the whole tier against a stand-in posting one body, and hand back the run.

    The body is the committed sample with its `img_url` pointed at the stand-in's
    own image, because the sample's own address is not one anything can fetch —
    and then whatever the alteration does to it. Everything else about it is the
    sample's, which is what makes the shape comparison meaningful.
    """
    body = json.loads(SAMPLE.read_text(encoding="utf-8"))
    webhook = f"http://127.0.0.1:{free_port()}/alert"
    with Substitute(webhook, SNAPSHOT.read_bytes()) as stand_in:
        body[image_url_field] = stand_in.image_url
        if alter is not None:
            alter(body)
        stand_in.will_post(body)
        result = tier(
            "reconcile",
            "--webhook-url",
            webhook,
            "--trigger",
            f"post:{stand_in.control_url}",
            "--sample",
            str(SAMPLE),
            "--timeout",
            "60",
        )
        truth(stand_in.posted, describing="the stand-in to have posted, so the tier captured one")
    return result


def test_an_unaltered_body_agrees_with_the_committed_sample() -> None:
    """The sample the producer still sends is the sample this repository records."""
    result = _run_over_a_substitute(None)

    passing(result, describing="the tier over a stand-in posting the committed sample")
    verdict = answer(result)
    equal(verdict["verdict"], "agrees", describing="the tier's verdict")
    equal(verdict["differences"], [], describing="the fields the tier found moved")
    # The image stage really ran: the tier fetched what the captured body pointed
    # at and read its bytes, rather than reporting a verdict without looking.
    truth(verdict["image"]["retrieved"], describing="the image the captured body points at")
    equal(
        verdict["image"]["bytes"],
        len(SNAPSHOT.read_bytes()),
        describing="the bytes of the image the tier read",
    )


def test_a_field_the_producer_added_fails_the_tier_naming_it() -> None:
    """A payload that gained a field is a payload the sample no longer describes."""
    result = _run_over_a_substitute(_add_a_field)

    failing(result, naming="event.severity")
    verdict = json.loads(result.stdout)
    equal(verdict["verdict"], "differs", describing="the tier's verdict")
    refused_naming(verdict["differences"], "unexpected field", "event.severity")


def test_a_field_the_producer_renamed_fails_the_tier_naming_both_names() -> None:
    """A rename is one name that went and one that arrived, and both are named."""
    result = _run_over_a_substitute(_rename_a_field, image_url_field="img_url")

    failing(result, naming="img_url")
    verdict = json.loads(result.stdout)
    equal(verdict["verdict"], "differs", describing="the tier's verdict")
    refused_naming(verdict["differences"], "missing field", "img_url")
    refused_naming(verdict["differences"], "unexpected field", "image_url")
    # The captured body is what was compared: the renamed field is in it.
    contains(verdict["captured"], "image_url", describing="the body the tier captured")


def test_a_field_the_producer_removed_fails_the_tier_naming_it() -> None:
    """A payload that lost a field is one every replayed test still passes on."""
    result = _run_over_a_substitute(_remove_a_field)

    failing(result, naming="print.filename")
    verdict = json.loads(result.stdout)
    equal(verdict["verdict"], "differs", describing="the tier's verdict")
    refused_naming(verdict["differences"], "missing field", "print.filename")


def test_a_field_the_producer_retyped_fails_the_tier_naming_it() -> None:
    """The same field, under a type nothing on this side deserializes."""
    result = _run_over_a_substitute(_retype_a_field)

    failing(result, naming="printer.id")
    verdict = json.loads(result.stdout)
    equal(verdict["verdict"], "differs", describing="the tier's verdict")
    refused_naming(verdict["differences"], "printer.id", "changed type")


def test_the_tier_reports_no_verdict_when_nothing_posts() -> None:
    """A stack that raised no alert is an error, not a verdict of agreement."""
    webhook = f"http://127.0.0.1:{free_port()}/alert"
    with Substitute(webhook, SNAPSHOT.read_bytes()) as stand_in:
        # A stack that takes the request and raises no alert: the tier waits its
        # own timeout out and must say so rather than call the silence agreement.
        stand_in.will_post(None)
        result = tier(
            "reconcile",
            "--webhook-url",
            webhook,
            "--trigger",
            f"post:{stand_in.control_url}",
            "--sample",
            str(SAMPLE),
            "--timeout",
            "5",
        )
        equal(stand_in.posted, [], describing="what the stand-in posted")

    equal(result.returncode, 2, describing="the exit status of a tier that reached no verdict")
    truth("reached no verdict" in said(result), describing="the tier to say it reached none")
    truth(
        "no webhook body arrived" in said(result),
        describing="the tier to say that nothing was posted, rather than a bare timeout",
    )
