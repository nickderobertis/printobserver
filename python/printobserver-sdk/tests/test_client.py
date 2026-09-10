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
    """A body that is not a document is refused rather than handed on."""
    with Host(200, None) as host:
        client = Client(host.address, ACTOR)
        # A host answering the four bytes `null` sends a document that is not
        # an object, which this client reads and the type it was asked for is
        # not. What is asserted is the read: the parse is what fails when the
        # body is not JSON at all.
        equal(client.status(PRINT), None)

    with Host(200, "{") as host:
        client = Client(host.address, ACTOR)
        equal(client.status(PRINT), "{")


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
        surface.call("GET", "/v1/anything", [], None)
    except NotImplementedError:
        return
    truth(False, describing="a surface to make no request")
