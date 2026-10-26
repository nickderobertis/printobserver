"""The `compose` trigger's refusals, reached through the tier's own entry point.

Before it raises an alert on a self-hosted stack, the tier reads the record the
bring-up wrote and points the stack's webhook plugin at the port its capture
holds. Either step can fail, and each must end the run with no verdict — exit
2 — and a sentence naming what stopped it, rather than with a traceback or a
wait for a body that nothing will post.
"""

from __future__ import annotations

import json
from pathlib import Path

from harness import TIER_CHOOSES, answer, environment, said, tier
from repo_checks.expect import contains, equal, passing

# A Compose project nothing runs under: the stack's own compose files, and no
# container of it, so Docker itself refuses the command that would repoint it.
ABSENT_PROJECT = "printobserver-obico-absent"


def _reconcile_over(state_dir: Path) -> tuple[int, str]:
    result = tier(
        "reconcile",
        "--webhook-url",
        TIER_CHOOSES,
        "--trigger",
        f"compose:{state_dir}",
        "--timeout",
        "30",
        timeout=900,
    )
    return result.returncode, said(result)


def test_a_record_missing_what_the_tier_reads_is_refused_naming_each_field(
    tmp_path: Path,
) -> None:
    """A record the bring-up would never write ends the run naming what it lacks."""
    (tmp_path / "instance.json").write_text(
        json.dumps({"state_dir": str(tmp_path), "project": 7}), encoding="utf-8"
    )

    status, output = _reconcile_over(tmp_path)

    equal(status, 2, describing=f"the tier's exit with no usable record:\n{output}")
    for named in ("project", "url", "printer_token", "just obico-up"):
        contains(output, named, describing="the refusal of an unusable record")


def test_a_record_that_is_not_an_object_is_refused(tmp_path: Path) -> None:
    """A record holding no object is refused rather than indexed."""
    (tmp_path / "instance.json").write_text("[]", encoding="utf-8")

    status, output = _reconcile_over(tmp_path)

    equal(status, 2, describing=f"the tier's exit with a record holding a list:\n{output}")
    contains(output, "holds no object", describing="the refusal of a record holding a list")


def test_a_stack_that_will_not_post_to_the_held_port_is_refused_naming_it(
    state_dir: str, tmp_path: Path
) -> None:
    """A stack that does not confirm the new address ends the run naming that address."""
    brought_up = environment("up", "--state-dir", state_dir, "--webhook-url", TIER_CHOOSES)
    passing(brought_up, describing="the bring-up")
    record = answer(brought_up)
    record["project"] = ABSENT_PROJECT
    (tmp_path / "instance.json").write_text(json.dumps(record), encoding="utf-8")

    status, output = _reconcile_over(tmp_path)

    equal(status, 2, describing=f"the tier's exit when the stack is not repointed:\n{output}")
    contains(output, "the stack would not post to http://", describing="the refusal")
    contains(output, "/alert", describing="the address the refusal names")
