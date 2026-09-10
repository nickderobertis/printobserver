"""What each client's generated walk drives, worked out once for all three.

The walk is generated rather than written because of what it is for: a client
that gained a method the walk did not drive would be a method nothing proved
anything about, and a walk written by hand is exactly where that happens. Here
the plan comes from the same operation list the methods do, so the two cannot
come apart — and the drift gate refuses a tree in which a committed walk is not
what this writes.
"""

from __future__ import annotations

import json
from dataclasses import dataclass

from contract_codegen.examples import TEXT, answer_of, argument_of, rejection_of
from contract_codegen.model import (
    ACTOR_PARAMETER,
    REASON_PARAMETER,
    Contract,
    Operation,
    Parameter,
    Ref,
)

#: The reason every mutating call of the walk carries.
REASON = "a generated walk is asking"


@dataclass(frozen=True, slots=True)
class Step:
    """One operation, as the walk drives it."""

    operation: Operation
    #: Every value the method is called with, in method order.
    arguments: tuple[tuple[Parameter, object], ...]
    #: The request target the call must reach.
    target: str
    #: The body the server must receive, or `None` where the call sends none.
    body: dict[str, object] | None
    #: The document the host answers, and what the method must answer back.
    answer: object
    #: The document the host refuses a mutating call with, or `None`.
    rejection: object | None

    @property
    def name(self) -> str:
        """The operation's own name."""
        return self.operation.name

    @property
    def method(self) -> str:
        """The method the call is made by."""
        return self.operation.method


def _value(parameter: Parameter, contract: Contract) -> object:
    """The value the walk calls one method with for one of its parameters."""
    if parameter.name == REASON_PARAMETER:
        return REASON
    return argument_of(parameter.carried, contract)


def plan(contract: Contract) -> tuple[Step, ...]:
    """Every step of the walk, one per operation the server declares."""
    steps: list[Step] = []
    for operation in contract.operations:
        arguments = tuple(
            (parameter, _value(parameter, contract)) for parameter in operation.supplied()
        )
        given = dict(
            (parameter.name, value) for parameter, value in arguments if parameter.located != "path"
        )
        target = operation.path
        for parameter in operation.parameters:
            if parameter.located == "path":
                target = target.replace(f"{{{parameter.name}}}", TEXT)
        asked = "&".join(
            f"{parameter.name}={value}"
            for parameter, value in arguments
            if parameter.located == "query"
        )
        body: dict[str, object] | None = None
        if any(parameter.located == "body" for parameter in operation.parameters):
            body = {
                name: value for name, value in given.items() if _located(operation, name) == "body"
            }
            if _located(operation, ACTOR_PARAMETER) == "body":
                # Who a client acts as is what it was made with rather than
                # what a call says, so a body carries it although no method
                # takes it. An operation that declares none — replacing a
                # manifest asks nothing of the machine — sends none.
                body[ACTOR_PARAMETER] = argument_of(Ref("Actor"), contract)
        steps.append(
            Step(
                operation=operation,
                arguments=arguments,
                target=f"{target}?{asked}" if asked else target,
                body=body,
                answer=answer_of(operation, contract),
                rejection=rejection_of(operation, contract) if operation.rejectable else None,
            )
        )
    return tuple(steps)


def _located(operation: Operation, name: str) -> str:
    """Where one value of a request travels."""
    for parameter in operation.parameters:
        if parameter.name == name:
            return parameter.located
    return ""


def document(value: object) -> str:
    """One example document, as the text a generated walk carries it as."""
    return json.dumps(value, sort_keys=True)


def actor(contract: Contract) -> str:
    """The actor every client of the walk is made with, as a document."""
    return document(argument_of(Ref("Actor"), contract))
