"""What the client does with an answer that is not the one it asked for.

The generated walk drives every method's success path and its refusal. These
are the paths beside them: an address written the several ways a supervisor's
own configuration writes it, and each answer this client will not act on.
"""

from __future__ import annotations

import json
from typing import cast

from host import Host
from printobserver_sdk import (
    Client,
    ClientError,
    PrinterRefusedError,
    RefusedError,
    UnreachableError,
    UnreadableError,
    reason_given,
)
from printobserver_sdk._surface import GeneratedSurface, NoReasonError, RejectedError
from printobserver_sdk.contract import ActionAnswer, Actor, Range, RejectionReason
from repo_checks.expect import contains, equal, truth

ACTOR: Actor = "operator"
PRINT = "0198f0a1-2b3c-7d4e-8f90-123456789abc"

#: An answer carrying nothing but the record a decision is taken on.
ACCEPTED: ActionAnswer = json.loads(
    '{"record":{"id":"a","print_id":"b","request":{"action":{"action":"pause",'
    '"actor":"operator","reason":"because"},"actor":"operator","requested_at":"now"},'
    '"decision":"accepted"}}'
)


def test_an_address_is_taken_as_the_supervisors_configuration_writes_it() -> None:
    """A supervisor's own client configuration writes it three ways."""
    for given in ("http://127.0.0.1:8420", "127.0.0.1:8420", "http://127.0.0.1:8420/"):
        equal(Client(given, ACTOR).address, "127.0.0.1:8420", describing=given)


def test_a_client_acts_as_the_actor_it_was_made_with() -> None:
    """Who a client is, and what authenticates it, are what it was made with."""
    client = Client("127.0.0.1:8420", ACTOR, credential="a-token")

    equal(client.actor, ACTOR)
    equal(client.credential, "a-token")


def test_nothing_answering_at_the_configured_address_is_its_own_failure() -> None:
    """Port one is privileged and never listened on, so this reaches nothing."""
    client = Client("http://127.0.0.1:1", ACTOR)

    try:
        client.status(PRINT)
    except UnreachableError as unreachable:
        equal(unreachable.address, "127.0.0.1:1")
    else:
        truth(False, describing="a call reaching nothing to be its own failure")


def test_an_answer_this_client_cannot_read_is_said_to_be_one() -> None:
    """A document that is not of the type the operation declares is refused rather than handed on.

    `null`, and a bare string, are documents a parse reads and no status answer
    is; each is refused under the status it arrived with.
    """
    for answered in (None, "{"):
        with Host(200, answered) as host:
            client = Client(host.address, ACTOR)
            try:
                # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
                client.status(PRINT)
            except UnreadableError as unreadable:
                equal(unreadable.status, 200, describing=f"{answered!r}")
            else:
                truth(False, describing=f"{answered!r} to be refused as unreadable")


def test_a_request_the_supervisor_will_not_act_on_carries_its_own_words() -> None:
    """A refusal in words arrives as those words."""
    with Host(404, {"error": "no such print"}) as host:
        client = Client(host.address, ACTOR)
        try:
            client.status(PRINT)
        except RefusedError as refused:
            equal(refused.status, 404)
            equal(refused.detail, "no such print")
        else:
            truth(False, describing="a refusal to arrive as one")


def test_a_refusal_the_supervisor_did_not_put_in_words_is_said_to_be_unreadable() -> None:
    """A refusal carrying no words of its own is said to carry none."""
    with Host(404, ["not an error answer"]) as host:
        client = Client(host.address, ACTOR)
        try:
            client.status(PRINT)
        except RefusedError as refused:
            equal(refused.detail, "it said nothing this client can read")
        else:
            truth(False, describing="a refusal to arrive as one")


def test_an_action_the_machine_itself_refused_carries_what_the_printer_said() -> None:
    """The policy accepted it and the machine did not; the record is the answer."""
    answer = {**ACCEPTED, "printer_refusal": "the printer is offline"}
    with Host(502, answer) as host:
        client = Client(host.address, ACTOR)
        try:
            client.pause(PRINT, "because")
        except PrinterRefusedError as refused:
            equal(refused.detail, "the printer is offline")
            equal(refused.answer, answer)
        else:
            truth(False, describing="a machine refusal to arrive as one")


def test_a_machine_refusal_that_said_nothing_says_so() -> None:
    """A refusal the printer gave no words for is said to have given none."""
    with Host(502, ACCEPTED) as host:
        client = Client(host.address, ACTOR)
        try:
            client.pause(PRINT, "because")
        except PrinterRefusedError as refused:
            equal(refused.detail, "it said nothing this client can read")
        else:
            truth(False, describing="a machine refusal to arrive as one")


def test_a_refusal_whose_record_was_not_refused_is_said_to_be_unreadable() -> None:
    """A refused action whose decision is not a refusal is a body nobody can act on."""
    with Host(409, ACCEPTED) as host:
        client = Client(host.address, ACTOR)
        try:
            client.pause(PRINT, "because")
        except UnreadableError as unreadable:
            contains(unreadable.detail, "not a refusal")
        else:
            truth(False, describing="an unreadable refusal to be said to be one")


def test_a_refusal_on_something_other_than_a_value_carries_no_range() -> None:
    """The value asked for and the range allowed are absent where none was ruled on."""
    refusal = {
        **ACCEPTED,
        "record": {**ACCEPTED["record"], "decision": {"rejected": "no_active_print"}},
    }
    with Host(409, refusal) as host:
        client = Client(host.address, ACTOR)
        try:
            client.pause(PRINT, "because")
        except RejectedError as refused:
            equal(refused.reason, "no_active_print")
            equal(refused.requested, None)
            equal(refused.allowed, None)
            contains(str(refused), "no_active_print")
        else:
            truth(False, describing="a refusal to arrive as one")


def test_a_refusal_on_a_value_says_the_value_and_the_range() -> None:
    """A caller that can read the range can ask again inside it."""
    reason = cast(
        RejectionReason,
        {
            "out_of_bounds": {
                "adjustable": "feedrate",
                "requested": 9.9,
                "allowed": {"min": 0.5, "max": 1.5},
            }
        },
    )
    allowed = cast(Range, {"min": 0.5, "max": 1.5})

    refused = RejectedError(reason, 9.9, allowed, ACCEPTED)

    contains(str(refused), "9.9")
    contains(str(refused), "0.5 to 1.5")


def test_every_failure_is_one_a_caller_can_tell_from_the_others() -> None:
    """Five ways a call ends, each saying what to do next."""
    failures: list[ClientError] = [
        UnreachableError("127.0.0.1:8420", "connection refused"),
        NoReasonError(),
        UnreadableError(200, "not a document"),
        RefusedError(404, "no such print"),
        PrinterRefusedError("offline", ACCEPTED),
    ]

    for failure in failures:
        truth(isinstance(failure, ClientError), describing=f"{failure!r} to be a client error")
        truth(str(failure), describing=f"{failure!r} to say something")


def test_a_reason_that_is_nothing_but_whitespace_is_no_reason() -> None:
    """A change nobody gave a reason for is refused before it is made."""
    for given in ("", "   "):
        try:
            reason_given(given)
        except NoReasonError:
            continue
        truth(False, describing=f"{given!r} to be no reason")
    equal(reason_given("because the print is failing"), None)


def test_a_surface_on_its_own_makes_no_request() -> None:
    """`Client` is what makes a request; a surface that answered one would be a second."""
    surface = GeneratedSurface()

    try:
        surface.call("GET", "/v1/anything", [], None, "PrintsAnswer")
    except NotImplementedError:
        return
    truth(False, describing="a surface to make no request")


#: A recovery's answer, carrying whatever decision a test gives it.
def _recovery(decision: object) -> dict[str, object]:
    """A recovery answer around one decision."""
    return {
        "action": {"action": "recover_fan", "percent": 50.0},
        "decision": decision,
        "detail": "recover_fan reached the printer, which refused it: the fan is stuck",
        "events": [],
    }


def test_a_recovery_the_machine_refused_carries_the_supervisors_words() -> None:
    """A recovery has no action record; the supervisor's sentence is the answer."""
    with Host(502, _recovery({"executed": {"failed": {"reason": "the fan is stuck"}}})) as host:
        client = Client(host.address, ACTOR)
        try:
            # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
            client.recover_fan(50.0, "because")
        except RefusedError as refused:
            equal(refused.status, 502)
            contains(refused.detail, "the fan is stuck", describing="what the printer said")
        else:
            truth(False, describing="a recovery the machine refused to arrive as one")


def test_a_recovery_refused_with_bounds_no_reader_can_read_is_unreadable() -> None:
    """A refusal whose bounds are not a value and a range is not handed on as one."""
    for refused in (
        {"out_of_bounds": {"field": "percent", "requested": "lots", "allowed": {}}},
        {"out_of_bounds": "everything"},
        {"out_of_bounds": {"field": "percent", "requested": True, "allowed": {"min": 0, "max": 1}}},
        {"out_of_bounds": {"requested": 150.0, "allowed": {"min": 0.0, "max": 100.0}}},
        None,
        {"two": {}, "arms": {}},
        {"print_running": {}},
        "print_running",
        {"z_untrusted": {}},
        {"printer_cannot": {"action": 7, "detail": None}},
        {"insufficient_clearance": {"margin_mm": "5", "nozzle_z_mm": 1.0, "tallest_mm": 2.0}},
        {"print_running": {"state": 7}},
        {
            "out_of_bounds": {
                "field": "percent",
                "requested": float("nan"),
                "allowed": {"min": 0.0, "max": 100.0},
            }
        },
        {
            "out_of_bounds": {
                "field": "percent",
                "requested": 10**400,
                "allowed": {"min": 0.0, "max": 100.0},
            }
        },
    ):
        with Host(409, _recovery({"rejected": refused})) as host:
            client = Client(host.address, ACTOR)
            try:
                # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
                client.recover_fan(50.0, "because")
            except UnreadableError as unreadable:
                equal(unreadable.status, 409, describing=f"{refused!r}")
            else:
                truth(False, describing=f"{refused!r} to be refused as unreadable")


def test_a_refusal_whose_answer_is_not_an_object_is_unreadable() -> None:
    """A policy or machine refusal answered as anything but an object is not handed on as one."""
    for status in (409, 502):
        for answer in (None, "refused", 7, [_recovery({"rejected": "print_running"})]):
            with Host(status, answer) as host:
                client = Client(host.address, ACTOR)
                try:
                    # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
                    client.recover_fan(50.0, "because")
                except UnreadableError as unreadable:
                    equal(unreadable.status, status, describing=f"{answer!r}")
                else:
                    truth(False, describing=f"{answer!r} at {status} to be unreadable")


def test_a_known_refusal_well_formed_arrives_as_a_refusal() -> None:
    """A refusal the contracts declare, carrying a state no vocabulary names, is handed on."""
    refused = {"print_running": {"state": {"unknown": "Connecting"}}}
    with Host(409, _recovery({"rejected": refused})) as host:
        client = Client(host.address, ACTOR)
        try:
            # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
            client.recover_fan(50.0, "because")
        except RejectedError as rejected:
            equal(rejected.reason, refused)
        else:
            truth(False, describing="a well-formed refusal to arrive as one")


#: Recovery answers the contracts forbid, each refused under `409`: a recovery
#: action missing its parameter, naming no declared action, or carrying a
#: parameter of the wrong type; a refusal no vocabulary declares, or carrying a
#: printer state no vocabulary names; and an event entry of no declared shape.
MALFORMED_RECOVERIES = (
    {**_recovery({"rejected": "z_untrusted"}), "action": {"action": "recover_lift"}},
    {**_recovery({"rejected": "z_untrusted"}), "action": {"action": "recover_dance"}},
    {
        **_recovery({"rejected": "z_untrusted"}),
        "action": {"action": "recover_fan", "percent": "lots"},
    },
    _recovery({"rejected": "a_newer_reason"}),
    _recovery({"rejected": {"a_newer_reason": {"anything": 1}}}),
    _recovery({"rejected": {"print_running": {"state": "melting"}}}),
    {**_recovery({"rejected": "z_untrusted"}), "events": [{"kind": 7}]},
)


def test_a_recovery_refusal_no_contract_declares_is_unreadable() -> None:
    """Every malformed recovery answer is refused as unreadable rather than handed on."""
    for answer in MALFORMED_RECOVERIES:
        with Host(409, answer) as host:
            client = Client(host.address, ACTOR)
            try:
                # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
                client.recover_fan(50.0, "because")
            except UnreadableError as unreadable:
                equal(unreadable.status, 409, describing=f"{answer!r}")
            else:
                truth(False, describing=f"{answer!r} to be unreadable")


#: A spool command's refusal, around one spool refusal cause.
def _spool_refusal(cause: object) -> dict[str, object]:
    """A spool command answer refused with `spool_refused` for `cause`."""
    return {
        "decision": {"rejected": {"spool_refused": {"cause": cause, "message": "refused"}}},
        "events": [],
    }


def test_a_spool_refusal_is_read_only_with_a_cause_the_contracts_declare() -> None:
    """A declared cause arrives as a refusal; an undeclared one is unreadable."""
    spool = "0198f0a1-2b3c-7d4e-8f90-123456789abc"
    declared = _spool_refusal({"cause": "unknown_spool", "spool_id": spool})
    with Host(409, declared) as host:
        client = Client(host.address, ACTOR)
        try:
            # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
            client.retire_spool(spool, "because")
        except RejectedError as rejected:
            equal(rejected.answer, declared)
        else:
            truth(False, describing="a declared spool refusal to arrive as one")
    for cause in ({"cause": "melted"}, {"cause": "unknown_spool"}):
        with Host(409, _spool_refusal(cause)) as host:
            client = Client(host.address, ACTOR)
            try:
                # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
                client.retire_spool(spool, "because")
            except UnreadableError as unreadable:
                equal(unreadable.status, 409, describing=f"{cause!r}")
            else:
                truth(False, describing=f"{cause!r} to be unreadable")


def test_a_machine_refusal_of_a_recovery_that_did_not_fail_is_unreadable() -> None:
    """A 502 carrying a recovery whose decision is no failure at the printer is not one."""
    for decision in (
        {"rejected": "z_untrusted"},
        {"executed": "succeeded"},
        None,
        {"executed": {"failed": {"reason": "the fan is stuck"}}, "rejected": "z_untrusted"},
        {"executed": {"failed": {"reason": "the fan is stuck"}, "succeeded": True}},
    ):
        with Host(502, _recovery(decision)) as host:
            client = Client(host.address, ACTOR)
            try:
                # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
                client.recover_fan(50.0, "because")
            except UnreadableError as unreadable:
                equal(unreadable.status, 502, describing=f"{decision!r}")
            else:
                truth(False, describing=f"{decision!r} at 502 to be unreadable")


def test_a_policy_refusal_whose_decision_has_two_arms_is_unreadable() -> None:
    """A decision carrying a refusal beside another arm is no decision at all."""
    with Host(409, _recovery({"rejected": "z_untrusted", "executed": "succeeded"})) as host:
        client = Client(host.address, ACTOR)
        try:
            # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
            client.recover_fan(50.0, "because")
        except UnreadableError as unreadable:
            equal(unreadable.status, 409)
        else:
            truth(False, describing="a two-armed decision to be unreadable")


def test_a_recovery_refusal_missing_its_parts_is_unreadable() -> None:
    """A recovery's refusal with no action name, or no events, is not handed on as one."""
    for answer in (
        {**_recovery({"rejected": "z_untrusted"}), "action": {}},
        {
            key: value
            for key, value in _recovery({"rejected": "z_untrusted"}).items()
            if key != "events"
        },
    ):
        with Host(409, answer) as host:
            client = Client(host.address, ACTOR)
            try:
                # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
                client.recover_fan(50.0, "because")
            except UnreadableError as unreadable:
                equal(unreadable.status, 409, describing=f"{answer!r}")
            else:
                truth(False, describing=f"{answer!r} to be unreadable")


def _status_with(allowance: object) -> dict[str, object]:
    """A recovery read whose one allowance is `allowance`."""
    return {
        "actions": [allowance],
        "last_print": None,
        "paused_print": None,
        "position": None,
        "state": "operational",
        "temperatures": {
            "bed": {"actual_c": 25.0, "target_c": 0.0},
            "tools": [{"actual_c": 25.0, "target_c": 0.0, "tool": 0}],
        },
        "z_home": None,
        "z_trust": {"untrusted": "this server started and has homed nothing"},
    }


def test_a_recovery_read_whose_allowance_contradicts_itself_is_unreadable() -> None:
    """An allowance is handed on only when `refusal` is sent exactly when `allowed` is false."""
    for agreeing in (
        {"action": "recover_lift", "allowed": True},
        {"action": "recover_heat", "allowed": False, "refusal": "z_untrusted"},
    ):
        with Host(200, _status_with(agreeing)) as host:
            client = Client(host.address, ACTOR)
            # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
            equal(client.recover_status(), _status_with(agreeing), describing=f"{agreeing!r}")
    for contradiction in (
        {"action": "recover_lift", "allowed": True, "refusal": "z_untrusted"},
        {"action": "recover_heat", "allowed": False},
        {"action": "recover_lift", "allowed": True, "refusal": None},
    ):
        with Host(200, _status_with(contradiction)) as host:
            client = Client(host.address, ACTOR)
            try:
                # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
                client.recover_status()
            except UnreadableError as unreadable:
                equal(unreadable.status, 200, describing=f"{contradiction!r}")
            else:
                truth(False, describing=f"{contradiction!r} to be unreadable")
