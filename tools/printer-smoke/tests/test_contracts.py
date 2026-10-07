"""The substitute supervisor decides only what the real one can.

`machine.py` spells the decisions it answers itself — a refusal for the state,
one for the bounds, one for a print a start replaced — because it stands in for
the supervisor rather than linking it. A spelling the contracts do not declare
would be one the smoke is proven against and the real supervisor never answers,
so every decision the substitute can make is driven out of it here, through the
same answer its HTTP handler serves, and held to the contracts' own
`PolicyDecision` schema.
"""

from __future__ import annotations

import json
from collections.abc import Iterator

import pytest
from jsonschema import Draft202012Validator
from machine import NO_ACTIVE_PRINT, PRINTING, Machine, identifier
from printer_smoke import CONSERVATIVE_ENVELOPE
from repo_checks.expect import equal, truth
from world import MANIFEST, REPO_ROOT

#: The contract every decision the supervisor answers is one of.
POLICY_DECISION = REPO_ROOT / "schemas" / "printobserver-core" / "PolicyDecision.json"

#: The print the substitute is about when it starts answering.
PRINT = "01860d5a-4a8f-7c3d-9a4e-4d2f6b1c8e90"


@pytest.fixture
def substitute() -> Iterator[Machine]:
    """One substitute, answering until the test is done with it."""
    machine = Machine(
        device="/dev/ttyACM0", envelope=CONSERVATIVE_ENVELOPE, manifest=MANIFEST, print_id=PRINT
    )
    yield machine
    machine.stop()


def decision_of(substitute: Machine, print_id: str, operation: str, body: object) -> object:
    """The decision one action's answer carries, asked as the command asks it."""
    _, answer = substitute.answer(
        "POST",
        f"/v1/prints/{print_id}/actions/{operation}",
        json.dumps(body).encode(),
    )
    return answer["record"]["decision"]


def test_every_decision_the_substitute_makes_is_one_the_contract_declares(
    substitute: Machine,
) -> None:
    """Accepted, and each refusal the substitute can answer, read as `PolicyDecision`."""
    schema = Draft202012Validator(json.loads(POLICY_DECISION.read_text(encoding="utf-8")))
    feedrate = {"factor": 1.0, "reason": "a contract check is asking", "actor": "operator"}

    made = {
        "invalid_from_state": decision_of(substitute, PRINT, "set_feedrate_factor", feedrate),
        "no_active_print": decision_of(substitute, identifier(), "set_feedrate_factor", feedrate),
    }
    substitute.printer.connection = substitute.printer.job_state = PRINTING
    made["out_of_bounds"] = decision_of(
        substitute, PRINT, "set_feedrate_factor", {**feedrate, "factor": 9.0}
    )
    made["accepted"] = decision_of(substitute, PRINT, "set_feedrate_factor", feedrate)

    equal(made["no_active_print"], NO_ACTIVE_PRINT, describing="the replaced print's refusal")
    for which, decision in made.items():
        problems = [error.message for error in schema.iter_errors(decision)]
        truth(
            not problems,
            describing=f"the `{which}` decision {decision!r} to be a `PolicyDecision`: {problems}",
        )


def test_a_decision_the_contract_does_not_declare_is_refused() -> None:
    """The check above refuses a spelling the contracts do not carry."""
    schema = Draft202012Validator(json.loads(POLICY_DECISION.read_text(encoding="utf-8")))

    truth(
        not schema.is_valid({"rejected": "no_print_running"}),
        describing="an undeclared refusal to be read as no `PolicyDecision`",
    )


#: The contract the history the supervisor answers is.
HISTORY_ANSWER = REPO_ROOT / "schemas" / "printobserver-server" / "HistoryAnswer.json"


def test_the_history_the_substitute_answers_is_the_one_the_contract_declares(
    substitute: Machine,
) -> None:
    """Every event the substitute keeps, read back as the server's `HistoryAnswer`.

    The events are spelled in `machine.py` as `Event`; this is what holds that
    spelling to `EventRecord`, through every kind the substitute writes: a
    request, a refusal, an execution, an expiry and a start's own print.
    """
    schema = Draft202012Validator(json.loads(HISTORY_ANSWER.read_text(encoding="utf-8")))
    feedrate = {"factor": 1.0, "reason": "a contract check is asking", "actor": "operator"}
    decision_of(substitute, PRINT, "set_feedrate_factor", feedrate)
    substitute.printer.connection = substitute.printer.job_state = PRINTING
    decision_of(substitute, PRINT, "set_feedrate_factor", {**feedrate, "factor": 9.0})
    decision_of(substitute, PRINT, "set_feedrate_factor", {**feedrate, "duration_s": 0})
    substitute.held("feedrate")
    _, history = substitute.answer("GET", f"/v1/prints/{PRINT}/history", b"")
    decision_of(substitute, PRINT, "start_print", {**feedrate, "file_name": "smoke.gcode"})
    _, started = substitute.answer("GET", f"/v1/prints/{substitute.print_id}/history", b"")

    for answer in (history, started):
        problems = [
            f"{list(error.absolute_path)}: {error.message}" for error in schema.iter_errors(answer)
        ]
        truth(not problems, describing=f"the history to be a `HistoryAnswer`: {problems}")
    kinds = {event["kind"] for event in [*history["events"], *started["events"]]}
    equal(
        kinds,
        {"action_requested", "action_rejected", "action_executed", "intervention_expired"},
        describing="the kinds of event the substitute was driven to write",
    )
