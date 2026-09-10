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


#: The order the walk against a **real** supervisor drives the operations in,
#: and the state the machine has to be in for each to be answered rather than
#: refused for want of one. `""` is any state.
#:
#: A real printer is a state machine, and the policy refuses an action that is
#: not valid from where the machine is — so an all-operation walk against one
#: has an order, and this is it. The set below is held to the server's own
#: operation list by `plan_live`: an operation with no place here stops the
#: generator, which is what keeps a new operation from arriving with nothing
#: driving it.
LIVE_ORDER: tuple[tuple[str, str], ...] = (
    ("status", ""),
    ("context", ""),
    ("image", ""),
    ("manifest_set", ""),
    ("manifest_get", ""),
    ("history", ""),
    # The bring-up left a print running, so the walk sets it down and starts
    # one — which is also the only order in which both are answered.
    ("cancel", "printing"),
    ("start_print", "operational"),
    ("set_feedrate_factor", "printing"),
    ("set_flowrate_factor", "printing"),
    ("set_fan_percent", "printing"),
    ("set_tool_target_c", "printing"),
    ("set_bed_target_c", "printing"),
    ("acknowledge_failure", ""),
    ("pause", "printing"),
    # Last, so the walk leaves the machine printing — where the bring-up left
    # it, and where the scripted environment's own suite expects it.
    ("resume", "paused"),
)

#: What the real walk supplies for one value, by the operation and the value it
#: is for, and by the value alone where every operation supplies the same. The
#: names beginning `world.` are read off the supervisor the walk brought up;
#: everything else is written out.
LIVE_VALUES: dict[tuple[str, str], str] = {
    ("*", "print_id"): "world.print_id",
    ("*", "image_id"): "world.image_id",
    ("*", "event_id"): "world.event_id",
    ("*", "file_name"): "world.file_name",
    ("*", "manifest"): "manifest",
    ("*", "reason"): "reason",
    ("*", "duration_s"): "60",
    ("*", "limit"): "20",
    ("*", "disposition"): "disposition:continue",
    ("set_feedrate_factor", "factor"): "1.1",
    ("set_flowrate_factor", "factor"): "1.0",
    ("set_fan_percent", "percent"): "50.0",
    ("set_tool_target_c", "tool"): "0",
    ("set_tool_target_c", "target_c"): "210.0",
    ("set_bed_target_c", "target_c"): "60.0",
}


@dataclass(frozen=True, slots=True)
class LiveStep:
    """One operation, as the walk against a real supervisor drives it."""

    operation: Operation
    #: The state the machine has to be in first, or empty for any.
    state: str
    #: Every value the method is called with, in method order, as the token the
    #: emitters render into their own language.
    arguments: tuple[tuple[Parameter, str], ...]

    @property
    def name(self) -> str:
        """The operation's own name."""
        return self.operation.name

    @property
    def method(self) -> str:
        """The method the call is made by."""
        return self.operation.method


def plan_live(contract: Contract) -> tuple[LiveStep, ...]:
    """Every step of the real-supervisor walk, one per operation.

    Raises:
        ValueError: If the declared order names an operation the server does
            not serve, omits one it does, or leaves a value of one with nothing
            to supply it. Each of those is an operation, or a value of one,
            that would otherwise arrive with nothing driving it.
    """
    served = {operation.name: operation for operation in contract.operations}
    ordered = [name for name, _ in LIVE_ORDER]
    missing = sorted(set(served) - set(ordered))
    invented = sorted(set(ordered) - set(served))
    if missing or invented:
        msg = (
            f"the real-supervisor walk's declared order omits {missing} and names "
            f"{invented}, which the server does not serve. Every operation it serves "
            f"has a place in that order, and nothing else does."
        )
        raise ValueError(msg)

    steps: list[LiveStep] = []
    for name, state in LIVE_ORDER:
        operation = served[name]
        supplied: list[tuple[Parameter, str]] = []
        for parameter in operation.supplied():
            given = LIVE_VALUES.get((name, parameter.name)) or LIVE_VALUES.get(
                ("*", parameter.name)
            )
            if given is None:
                msg = (
                    f"`{name}` takes `{parameter.name}`, and the real-supervisor walk "
                    f"has nothing to supply it with. A value a real machine has to be "
                    f"given cannot be guessed at."
                )
                raise ValueError(msg)
            supplied.append((parameter, given))
        steps.append(LiveStep(operation=operation, state=state, arguments=tuple(supplied)))
    return tuple(steps)


def rejected_live(contract: Contract) -> tuple[LiveStep, ...]:
    """Every step the real-supervisor walk drives as a refused action.

    One per method carrying an action of the vocabulary. What refuses them is
    the grant: the policy takes that decision before it looks at the state, the
    interval or the bounds, so one client acting as an actor class granted
    nothing is refused every action from wherever the machine happens to be.
    """
    return tuple(step for step in plan_live(contract) if step.operation.rejectable)
