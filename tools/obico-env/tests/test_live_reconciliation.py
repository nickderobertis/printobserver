"""The tier against the real self-hosted Obico, walking every stage of its path.

The stand-in journeys prove the tier's own arithmetic — that an alteration
travels through the capture and comes out named. They cannot prove the stages
before the capture, because a stand-in posts whatever it is told to. This one
does: a real stack is brought up, a real failure alert is raised on it through
Obico's own `alert_if_needed`, Obico's own worker delivers it through Obico's own
webhook plugin, and the tier captures what arrives, fetches the image that body
points at, and reports a verdict.

Which way that verdict goes is deliberately not asserted. Whether the running
Obico still posts what the committed sample records is a fact about an external
project on its own release cadence, and a divergence found here is a finding to
report rather than a failure of this suite. What is asserted is that every stage
ran and that a verdict came out — an error would mean the tier learned nothing.
"""

from __future__ import annotations

import json

from harness import answer, environment, free_port, said, tier
from repo_checks.expect import contains, equal, truth


def test_the_tier_walks_every_stage_against_the_real_stack(state_dir: str) -> None:
    """Six stages, one verdict: the run this whole tier exists to make."""
    webhook = f"http://host.docker.internal:{free_port()}/alert"
    brought_up = environment("up", "--state-dir", state_dir, "--webhook-url", webhook)
    truth(brought_up.returncode == 0, describing=f"the bring-up to succeed:\n{said(brought_up)}")
    record = answer(brought_up)

    result = tier(
        "reconcile",
        "--webhook-url",
        webhook,
        "--trigger",
        f"compose:{state_dir}",
        "--timeout",
        "300",
        timeout=1800,
    )

    # Stage 6 first, because it is what the other five are for: a verdict rather
    # than an error. Exit 2 is "reached no verdict"; 0 and 1 are both verdicts.
    truth(
        result.returncode in {0, 1},
        describing=f"the tier to report a verdict rather than an error:\n{said(result)}",
    )
    verdict = json.loads(result.stdout)
    contains({"agrees", "differs"}, verdict["verdict"], describing="the verdicts a run may reach")

    # Stage 1 and 2: it listened where the stack was configured, and the alert
    # was caused on the stack rather than posted by this suite.
    equal(verdict["webhook_url"], webhook, describing="the address the tier listened on")
    equal(verdict["trigger"], f"compose:{state_dir}", describing="how the alert was caused")
    truth(verdict["caused"], describing="what causing the alert on the stack said")

    # Stage 3: a body was captured, and it is Obico's failure-alert body.
    captured = verdict["captured"]
    truth(verdict["captured_bytes"], describing="bytes captured from the stack's own post")
    contains(captured, "event", describing="the body the stack posted")
    equal(
        captured["event"]["type"],
        "PrintFailure",
        describing="the event type Obico's own webhook plugin posted",
    )
    equal(
        captured["printer"]["name"],
        record["printer_name"],
        describing="the printer the live stack alerted about",
    )

    # Stage 4: the image that captured body points at was retrieved and read.
    image = verdict["image"]
    truth(image["retrieved"], describing=f"the image to be retrieved: {image}")
    truth(image["bytes"], describing="bytes of the image the captured body points at")
    equal(image["url"], captured["img_url"], describing="the address the tier fetched")

    # Stage 5: the comparison ran over that captured body, against the sample.
    contains(verdict["sample"], "failure-alert.json", describing="the sample compared against")
    truth(isinstance(verdict["differences"], list), describing="a list of fields that moved")
