"""What every generated method is written against, and every way a call can end.

The generated module carries one method per operation the server declares.
This is what those methods stand on: the two things a call needs from the
client — who it acts as, and how to make a request — and the closed set of
failures a caller distinguishes.

Nothing here imports the generated module at run time. The type names below are
that module's, brought in for the type checker alone, so that the two files can
name each other's shapes without either being unimportable.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:  # pragma: no cover - imported for annotations alone
    from printobserver_sdk.contract import ActionAnswer, Actor, Range, RejectionReason


class ClientError(Exception):
    """Why a call did not answer what it asked for."""


class UnreachableError(ClientError):
    """Nothing answered at the configured address."""

    def __init__(self, address: str, detail: str) -> None:
        """Record where this client looked and what happened there."""
        super().__init__(
            f"nothing answered at {address}: {detail}. Start the supervisor there, or "
            f"point this client at the address it is answering on"
        )
        self.address = address
        self.detail = detail


class NoReasonError(ClientError):
    """This client would not send the request, because it carried no reason.

    Every mutating operation carries a reason, and a call whose reason is empty
    is refused here rather than at the server: nothing reaches the policy, the
    printer or the record.
    """

    def __init__(self) -> None:
        """Say what every mutating call owes."""
        super().__init__("every mutating call carries a reason, and this one carries none")


class UnreadableError(ClientError):
    """The supervisor answered something this client cannot read."""

    def __init__(self, status: int, detail: str) -> None:
        """Record the status it answered under and what could not be read."""
        super().__init__(
            f"the supervisor answered {status} with something this client cannot read: {detail}"
        )
        self.status = status
        self.detail = detail


class RefusedError(ClientError):
    """The supervisor will not do what it was asked, and said why."""

    def __init__(self, status: int, detail: str) -> None:
        """Record the status it answered under and what it said."""
        super().__init__(f"the supervisor answered {status}: {detail}")
        self.status = status
        self.detail = detail


class PrinterRefusedError(ClientError):
    """The policy accepted the action and the machine refused it."""

    def __init__(self, detail: str, answer: ActionAnswer) -> None:
        """Record what the printer said and the record the request left."""
        super().__init__(
            f"the policy accepted this action and the machine refused it: {detail}. "
            f"Look at the printer, then ask again"
        )
        self.detail = detail
        self.answer = answer


class RejectedError(ClientError):
    """The policy refused the action, and the refusal is the answer.

    `reason` is the refusal itself, matched rather than read; `requested` and
    `allowed` are the value that was asked for and the range that is allowed,
    which the policy states when it ruled on a value and which are `None` when
    it ruled on something else.
    """

    def __init__(
        self,
        reason: RejectionReason,
        requested: float | None,
        allowed: Range | None,
        answer: ActionAnswer,
    ) -> None:
        """Record the reason, the value asked for and the range allowed."""
        said = f"the supervisor's policy refused this action: {reason!r}"
        if requested is not None and allowed is not None:
            said += (
                f". It was asked for {requested}, and what is allowed is "
                f"{allowed['min']} to {allowed['max']}"
            )
        super().__init__(said)
        self.reason = reason
        self.requested = requested
        self.allowed = allowed
        self.answer = answer


def reason_given(reason: str) -> None:
    """Refuse a mutating call that carries no reason, before it is made.

    Raises:
        NoReasonError: If the reason is absent or is nothing but whitespace.
    """
    if not reason or not reason.strip():
        raise NoReasonError


class GeneratedSurface:
    """The two things every generated method needs from the client it is on.

    A surface on its own makes no request: `Client` is what implements `call`,
    and the generated methods are written against this so that the transport
    and the operation list stay separable.
    """

    #: Who this client acts as, which every mutating call carries.
    actor: Actor

    def call(
        self,
        method: str,
        target: str,
        asked: list[tuple[str, str]],
        sending: dict[str, object] | None,
    ) -> object:
        """Make one call and answer the document that came back.

        Raises:
            NotImplementedError: Always. `Client` is what makes a request; a
                surface that answered one would be a second transport.
        """
        raise NotImplementedError
