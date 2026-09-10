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
    RejectionReasonOutOfBoundsPayload,
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
            credential: What authenticates to it. This server requires none of
                its API callers; a credential is for a deployment that has put
                something in front of it that does.
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
    answer = cast(ActionAnswer, _document(status, said))
    decision = answer["record"]["decision"]
    if not isinstance(decision, dict) or "rejected" not in decision:
        return UnreadableError(
            status,
            "the supervisor refused this action and answered a record whose decision "
            "is not a refusal",
        )
    reason = decision["rejected"]
    requested: float | None = None
    allowed: Range | None = None
    if isinstance(reason, dict) and "out_of_bounds" in reason:
        bounds = cast(RejectionReasonOutOfBoundsPayload, reason["out_of_bounds"])
        requested = bounds["requested"]
        allowed = bounds["allowed"]
    return RejectedError(reason, requested, allowed, answer)


def _machine_refusal(status: int, said: str) -> Exception:
    """The machine's own refusal of an action the policy accepted."""
    answer = cast(ActionAnswer, _document(status, said))
    return PrinterRefusedError(
        answer.get("printer_refusal") or "it said nothing this client can read", answer
    )


__all__ = ["Client", "NoReasonError"]
