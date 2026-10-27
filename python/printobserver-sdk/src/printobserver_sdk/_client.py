"""The client every generated method is a method of.

It carries three things and no more: where the supervisor is, what
authenticates to it, and who this client acts as. The third is why no generated
method takes an actor: a call cannot act as somebody the client is not.

The one request this makes is written with the standard library's own HTTP
client rather than with a dependency. This client speaks to a supervisor over a
loopback or a local network — the supervisor is the thing beside the printer —
and a package a supervision tool takes as a dependency is better without a
transport stack of its own.
"""

from __future__ import annotations

import http.client
import json
from typing import cast
from urllib.parse import quote, urlsplit

from printobserver_sdk._surface import (
    NoReasonError,
    PrinterRefusedError,
    RefusedError,
    RejectedError,
    UnreachableError,
    UnreadableError,
)
from printobserver_sdk.contract import (
    ActionAnswer,
    Actor,
    GeneratedClient,
    Range,
    RecoveryAnswer,
    RecoveryRejection,
    RejectionReason,
    SpoolCommandAnswer,
)

#: The status a rejected action is answered under.
REJECTED_STATUS = 409

#: The status an action the machine itself refused is answered under.
MACHINE_REFUSED_STATUS = 502

#: The status a read or a write that was carried out is answered under.
SUCCESS_STATUS = 200

#: The media type every operation takes a body in and answers in.
MEDIA_TYPE = "application/json"

#: How long one request waits, in seconds.
TIMEOUT_SECONDS = 30.0


class Client(GeneratedClient):
    """A typed client of one printobserver supervisor.

    One method per public operation that supervisor serves, each generated from
    the same checked-in description the server's own routes are folded out of.
    """

    def __init__(self, server: str, actor: Actor, credential: str | None = None) -> None:
        """Point this client at one supervisor, acting as one actor.

        Args:
            server: Where the supervisor answers, as its own configuration
                writes it — `http://127.0.0.1:8420` — or as a bare `host:port`.
            actor: Who this client acts as.
            credential: What authenticates to it. The supervisor serves no
                versioned operation to a caller that does not present a
                credential it admits — the operator's, whose verifier it is
                configured with and which `printobserver credential issue`
                issues, or the one a supervision turn is handed — and a call it
                refuses for that raises `RefusedError` with status 401. A call
                whose actor is not the identity that credential is raises it
                with status 403.
        """
        self.address = _address(server)
        self.actor = actor
        self.credential = credential

    def call(
        self,
        method: str,
        target: str,
        asked: list[tuple[str, str]],
        sending: dict[str, object] | None,
    ) -> object:
        """Make one call and answer the document that came back.

        Returns:
            The document the supervisor answered, parsed.

        Raises:
            UnreachableError: If nothing answered at the configured address.
            UnreadableError: If the answer is not a document this client can read.
            RefusedError: If the supervisor will not do what it was asked.
            PrinterRefusedError: If the policy accepted the action and the machine
                refused it.
            RejectedError: If the policy refused the action.
        """
        headers = {"Accept": MEDIA_TYPE}
        body = None
        if sending is not None:
            body = json.dumps(sending)
            headers["Content-Type"] = MEDIA_TYPE
        if self.credential is not None:
            headers["Authorization"] = f"Bearer {self.credential}"

        connection = http.client.HTTPConnection(self.address, timeout=TIMEOUT_SECONDS)
        try:
            connection.request(method, _target(target, asked), body=body, headers=headers)
            answered = connection.getresponse()
            status = answered.status
            said = answered.read().decode("utf-8", errors="replace")
        except OSError as unreachable:
            raise UnreachableError(self.address, str(unreachable)) from unreachable
        finally:
            connection.close()

        if status == SUCCESS_STATUS:
            return _document(status, said)
        if status == REJECTED_STATUS:
            raise _rejection(status, said)
        if status == MACHINE_REFUSED_STATUS:
            raise _machine_refusal(status, said)
        raise RefusedError(status, _said(said))


def _address(server: str) -> str:
    """One supervisor's address, as its own configuration writes it."""
    trimmed = server.strip().rstrip("/")
    split = urlsplit(trimmed)
    return split.netloc if split.netloc else trimmed


def _target(path: str, asked: list[tuple[str, str]]) -> str:
    """The request target one call is made to, with what it asks for."""
    if not asked:
        return path
    written = "&".join(f"{quote(name, safe='')}={quote(value, safe='')}" for name, value in asked)
    return f"{path}?{written}"


def _document(status: int, said: str) -> object:
    """One answer, parsed.

    Raises:
        UnreadableError: If it is not a document this client can read.
    """
    try:
        return json.loads(said)
    except ValueError as unreadable:
        raise UnreadableError(status, str(unreadable)) from unreadable


def _said(said: str) -> str:
    """What the supervisor said about a request it will not act on."""
    try:
        answer = json.loads(said)
    except ValueError:
        return "it said nothing this client can read"
    if isinstance(answer, dict) and isinstance(answer.get("error"), str):
        return answer["error"]
    return "it said nothing this client can read"


def _rejection(status: int, said: str) -> Exception:
    """The policy's own refusal, as the answer to it carries it."""
    document = _document(status, said)
    # An action's answer carries its decision in its record; a recovery's names
    # its action beside its decision, and a spool command's carries its decision
    # at the top. Each `cast` below names the type `operations.json` declares for
    # the refusal the match has just told apart: `_document` hands back the
    # server's JSON untyped, and the match is the check this client makes of its
    # shape before naming it.
    answer: ActionAnswer | SpoolCommandAnswer | RecoveryAnswer
    match document:
        case {"record": {"decision": {"rejected": refused}}}:
            answer = cast(ActionAnswer, document)
        case {"action": dict(), "detail": str(), "decision": {"rejected": refused}}:
            answer = cast(RecoveryAnswer, document)
        case {"decision": {"rejected": refused}}:
            answer = cast(SpoolCommandAnswer, document)
        case _:
            return UnreadableError(
                status,
                "the supervisor refused this action and answered a record whose decision "
                "is not a refusal",
            )
    # A refusal is a variant's name, or one variant carrying its fields.
    is_named = isinstance(refused, str) and bool(refused)
    is_carried = (
        isinstance(refused, dict)
        and len(refused) == 1
        and all(isinstance(fields, dict) for fields in refused.values())
    )
    if not (is_named or is_carried):
        return UnreadableError(status, f"the supervisor refused with no refusal: {refused!r}")
    # What `rejected` holds is the contracts' `RejectionReason` or their
    # `RecoveryRejection`, which the server serializes it from; the match bound
    # it untyped, and the check above holds it to a refusal's shape.
    reason = cast(RejectionReason | RecoveryRejection, refused)
    requested: float | None = None
    allowed: Range | None = None
    if isinstance(refused, dict) and "out_of_bounds" in refused:
        match refused["out_of_bounds"]:
            case {
                "requested": int() | float() as asked,
                "allowed": {"min": int() | float(), "max": int() | float()} as bounds,
            }:
                requested = float(asked)
                allowed = cast(Range, bounds)
            case malformed:
                return UnreadableError(
                    status, f"the refusal's bounds are not a value and a range: {malformed!r}"
                )
    return RejectedError(reason, requested, allowed, answer)


def _machine_refusal(status: int, said: str) -> Exception:
    """The machine's own refusal of an action the policy accepted.

    A recovery the machine refused carries no action record, and arrives as the
    supervisor's own words for it.
    """
    document = _document(status, said)
    match document:
        case {"action": _, "detail": str() as detail}:
            return RefusedError(status, detail)
    answer = cast(ActionAnswer, document)
    return PrinterRefusedError(
        answer.get("printer_refusal") or "it said nothing this client can read", answer
    )


__all__ = ["Client", "NoReasonError"]
