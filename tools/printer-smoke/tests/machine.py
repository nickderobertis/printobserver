"""A printer the test controls, and the supervisor in front of it.

The smoke drives the real `printobserver` command; what that command reaches is
this — one server answering the supervisor's own operations and `OctoPrint`'s
own connection read. Nothing about the program under test is stood in for: the
binary is the one this workspace builds, and this is the far side of it.

Two things can be scripted here, and they are kept apart on purpose.

`faults` are **wrong answers**: a machine that takes an adjustment and reports
the old value, one that applies a request the policy refused, one that never
puts a bounded change back, one that reports itself printing after a cancel.
Each is a verification point the smoke has to catch, and a smoke that read the
answers and discarded them would pass every assertion about which operations it
issued while failing every one of these.

`refusals` are **failures**: one operation answered as a server that could not
do it. They are what a cleanup walk needs — a run that fails at a stage without
the machine being unable to clean up afterwards.

The policy this stands in for is the supervisor's own in one respect that
matters to a cleanup: an adjustment is valid from a printing or a paused
machine and from no other state, so one asked for after a cancel is refused
here exactly as `printobserver-core`'s own `valid_from` refuses it. Without
that, a cleanup that put values back after the print had ended would pass here
and be refused by the real thing.
"""

from __future__ import annotations

import json
import secrets
import threading
import time
import uuid
from dataclasses import dataclass, field
from datetime import UTC, datetime
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any
from urllib.parse import urlsplit

HOST = "127.0.0.1"

#: The state a printer reports, in the vocabulary the contracts normalize to.
OPERATIONAL = "operational"
PRINTING = "printing"
PAUSED = "paused"

#: What each mutating operation of the supervisor's surface changes.
ADJUSTMENTS: dict[str, str] = {
    "set_feedrate_factor": "feedrate",
    "set_flowrate_factor": "flowrate",
    "set_tool_target_c": "tool_target:0",
    "set_bed_target_c": "bed_target",
    "set_fan_percent": "fan",
}

#: The value each adjustment carries, in the field the contracts declare for it.
ASKED: dict[str, str] = {
    "feedrate": "factor",
    "flowrate": "factor",
    "tool_target:0": "target_c",
    "bed_target": "target_c",
    "fan": "percent",
}

#: Every fault this substitute can be scripted with, and there is no other.
FAULTS = (
    "adjustment-ignored",
    "cancel-cools-the-heaters",
    "out-of-bounds-applied",
    "expiry-not-restored",
    "pause-not-taken",
    "cancel-not-taken",
    "bounds-widened",
)


def identifier() -> str:
    """One identifier of the shape this system mints: a lowercase UUID version 7."""
    raw = bytearray(secrets.token_bytes(16))
    raw[6] = (raw[6] & 0x0F) | 0x70
    raw[8] = (raw[8] & 0x3F) | 0x80
    return str(uuid.UUID(bytes=bytes(raw)))


def now() -> str:
    """The instant, as the contracts spell one."""
    return datetime.now(UTC).isoformat().replace("+00:00", "Z")


def reported(value: float) -> dict[str, Any]:
    """One number, as a reported value carrying its own plausibility flag."""
    return {"value": value, "out_of_range": False}


@dataclass
class Printer:
    """What the machine is doing, and what it is holding."""

    connection: str = OPERATIONAL
    job_state: str = OPERATIONAL
    file_name: str | None = None
    values: dict[str, float] = field(
        default_factory=lambda: {
            "feedrate": 1.0,
            "flowrate": 1.0,
            "tool_target:0": 200.0,
            "bed_target": 55.0,
            "fan": 0.0,
        }
    )


class Machine:
    """One controlled far side of the command, on a port of its own."""

    def __init__(
        self,
        *,
        device: str,
        envelope: dict[str, tuple[float, float]],
        manifest: dict[str, Any],
        print_id: str,
    ) -> None:
        """Start answering, on a free port.

        Args:
            device: The serial device it reports being connected to.
            envelope: The bounds it enforces, which are the configured ones.
            manifest: The manifest it answers a manifest read with.
            print_id: The print every operation of it is about.
        """
        self.device = device
        self.envelope = envelope
        self.manifest = manifest
        self.print_id = print_id
        self.printer = Printer()
        self.events: list[dict[str, Any]] = []
        self.interventions: list[dict[str, Any]] = []
        self.operations: list[str] = []
        self.commands: list[str] = []
        self.faults: set[str] = set()
        self.refusals: dict[str, tuple[int, int]] = {}
        self.lock = threading.Lock()
        self.server = ThreadingHTTPServer((HOST, 0), _handler_for(self))
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    @property
    def url(self) -> str:
        """Where it answers."""
        host, port = self.server.server_address[:2]
        return f"http://{host}:{port}"

    def stop(self) -> None:
        """Stop answering, leaving no thread behind."""
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=10)

    def fail(self, *faults: str) -> None:
        """Script one or more wrong answers.

        Args:
            *faults: Names drawn from `FAULTS`.

        Raises:
            KeyError: If a name is not one this substitute declares, so that a
                fault nobody implements cannot be mistaken for one that passed.
        """
        for fault in faults:
            if fault not in FAULTS:
                raise KeyError(fault)
            self.faults.add(fault)

    def refuse(self, operation: str, *, times: int = 1, after: int = 0) -> None:
        """Answer one operation as a server that could not do it.

        Args:
            operation: The operation's own name.
            times: How many times before it works again.
            after: How many of them to let through first, which is how a run is
                failed at one stage rather than at the first stage using it.
        """
        self.refusals[operation] = (after, times)

    def held(self, adjustable: str) -> float:
        """What the machine is holding for one adjustable."""
        with self.lock:
            self._expire()
            return self.printer.values[adjustable]

    def state(self) -> str:
        """What the machine reports it is doing."""
        with self.lock:
            return self.printer.connection

    # ~~ the answers

    def answer(self, method: str, path: str, body: bytes) -> tuple[int, dict[str, Any]]:
        """Answer one request, recording what was asked for.

        Args:
            method: The method it was reached by.
            path: Its path, with the versioned prefix.
            body: The JSON body, when it carried one.

        Returns:
            The status and the document to answer with.
        """
        with self.lock:
            if path == "/api/connection":
                return 200, {"current": {"port": self.device, "state": "Operational"}}
            segments = path.strip("/").split("/")
            if segments[:1] != ["v1"] or len(segments) < 4:
                return 404, {"error": f"nothing serves {path}"}
            operation = segments[-1]
            self.operations.append(operation)
            let_through, refusals = self.refusals.get(operation, (0, 0))
            if refusals and let_through:
                self.refusals[operation] = (let_through - 1, refusals)
            elif refusals:
                self.refusals[operation] = (0, refusals - 1)
                return 500, {"error": f"this substitute was scripted to refuse `{operation}`"}
            if method == "GET":
                return self._read(operation)
            self.commands.append(operation)
            return self._act(operation, json.loads(body or b"{}"))

    def _read(self, operation: str) -> tuple[int, dict[str, Any]]:
        """One read of the supervisor's surface."""
        self._expire()
        if operation == "status":
            return 200, {
                "print": self._print(),
                "printer": self._printer(),
                "job": self._job(),
                "interventions": [],
            }
        if operation == "context":
            return 200, {
                "context": {
                    "print": self._print(),
                    "printer": self._printer(),
                    "job": self._job(),
                    "manifest": self.manifest,
                    "bounds": {"allowed": self._bounds()},
                    "interventions": [],
                    "recent_events": [],
                }
            }
        if operation == "manifest":
            return 200, {"manifest": self.manifest, "narrowings": []}
        if operation == "history":
            return 200, {"events": list(reversed(self.events))}
        return 404, {"error": f"this substitute serves no `{operation}` read"}

    def _bounds(self) -> dict[str, dict[str, float]]:
        """The effective bounds a context read reports."""
        widened = "bounds-widened" in self.faults
        return {
            name: {"min": low, "max": high + (10.0 if widened else 0.0)}
            for name, (low, high) in self.envelope.items()
        }

    def _print(self) -> dict[str, Any]:
        """The print record every answer carries."""
        return {
            "id": self.print_id,
            "started_at": now(),
            "file_name": self.printer.file_name,
        }

    def _printer(self) -> dict[str, Any]:
        """The printer, as a snapshot of what it is doing and holding."""
        values = self.printer.values
        return {
            "connection": self.printer.connection,
            "tools": [{"target_c": reported(values["tool_target:0"])}],
            "bed": {"target_c": reported(values["bed_target"])},
            "feedrate_factor": reported(values["feedrate"]),
            "flowrate_factor": reported(values["flowrate"]),
            "fan_percent": reported(values["fan"]),
            "observed_at": now(),
        }

    def _job(self) -> dict[str, Any]:
        """The job, as the machine reports it."""
        return {"state": self.printer.job_state, "file_name": self.printer.file_name}

    # ~~ the actions

    def _act(self, operation: str, body: dict[str, Any]) -> tuple[int, dict[str, Any]]:
        """One action of the vocabulary, decided and then carried out."""
        self._expire()
        action_id = identifier()
        self._record("action_requested", {"action_id": action_id, "action": operation})

        adjustable = ADJUSTMENTS.get(operation)
        if adjustable is not None:
            if self.printer.connection not in {PRINTING, PAUSED}:
                decision = {"rejected": {"invalid_from_state": {"state": self.printer.connection}}}
                self._record("action_rejected", {"action_id": action_id, "decision": decision})
                return 409, {"record": self._record_of(action_id, decision)}
            asked = float(body[ASKED[adjustable]])
            low, high = self.envelope[adjustable]
            if not low <= asked <= high:
                if "out-of-bounds-applied" in self.faults:
                    self.printer.values[adjustable] = asked
                decision = {
                    "rejected": {
                        "out_of_bounds": {
                            "adjustable": adjustable,
                            "requested": asked,
                            "allowed": {"min": low, "max": high},
                        }
                    }
                }
                self._record("action_rejected", {"action_id": action_id, "decision": decision})
                return 409, {"record": self._record_of(action_id, decision)}
            return self._adjust(action_id, adjustable, asked, body.get("duration_s"))

        return self._transition(action_id, operation, body)

    def _adjust(
        self, action_id: str, adjustable: str, asked: float, duration: int | None
    ) -> tuple[int, dict[str, Any]]:
        """Apply one accepted adjustment, opening a bounded intervention when asked."""
        prior = self.printer.values[adjustable]
        if "adjustment-ignored" not in self.faults:
            self.printer.values[adjustable] = asked
        intervention: dict[str, Any] | None = None
        if duration is not None:
            intervention = {
                "id": identifier(),
                "adjustable": adjustable,
                "prior_value": prior,
                "applied_value": asked,
                "expires_at": time.monotonic() + float(duration),
                "expired": False,
            }
            self.interventions.append(intervention)
        self._record(
            "action_executed",
            {
                "action_id": action_id,
                "intervention_id": None if intervention is None else intervention["id"],
            },
        )
        return 200, {"record": self._record_of(action_id, "accepted")}

    def _transition(
        self, action_id: str, operation: str, body: dict[str, Any]
    ) -> tuple[int, dict[str, Any]]:
        """One accepted action that moves the print rather than a value."""
        printer = self.printer
        if operation == "start_print":
            printer.connection = printer.job_state = PRINTING
            printer.file_name = str(body.get("file_name"))
        elif operation == "pause" and "pause-not-taken" not in self.faults:
            printer.connection = printer.job_state = PAUSED
        elif operation == "resume" and "pause-not-taken" not in self.faults:
            printer.connection = printer.job_state = PRINTING
        elif operation == "cancel" and "cancel-not-taken" not in self.faults:
            printer.connection = printer.job_state = OPERATIONAL
            if "cancel-cools-the-heaters" in self.faults:
                # What a real machine's own end-of-print script does: the
                # heaters go off with the print, after which no adjustment is
                # valid from the state it is now in.
                printer.values["tool_target:0"] = 0.0
                printer.values["bed_target"] = 0.0
        self._record("action_executed", {"action_id": action_id, "intervention_id": None})
        return 200, {"record": self._record_of(action_id, "accepted")}

    def _expire(self) -> None:
        """Put back what every bounded change replaced, once its bound has passed."""
        for intervention in self.interventions:
            if intervention["expired"] or time.monotonic() < intervention["expires_at"]:
                continue
            intervention["expired"] = True
            restored = "expiry-not-restored" not in self.faults
            if restored:
                self.printer.values[intervention["adjustable"]] = intervention["prior_value"]
            self._record(
                "intervention_expired",
                {
                    "intervention_id": intervention["id"],
                    "adjustable": intervention["adjustable"],
                    "outcome": "restored" if restored else "restore_unavailable",
                },
            )

    def _record(self, kind: str, payload: dict[str, Any]) -> None:
        """Write one event into the history this substitute answers."""
        self.events.append(
            {
                "id": identifier(),
                "print_id": self.print_id,
                "source": "supervisor",
                "received_at": now(),
                "kind": kind,
                "payload": payload,
            }
        )

    def _record_of(self, action_id: str, decision: object) -> dict[str, Any]:
        """The action record one answer carries."""
        return {
            "id": action_id,
            "print_id": self.print_id,
            "decision": decision,
            "requested_at": now(),
        }


def _handler_for(substitute: Machine) -> type[BaseHTTPRequestHandler]:
    """The request handler answering out of one substitute's own state."""

    class Handler(BaseHTTPRequestHandler):
        """One request of the supervisor's or `OctoPrint`'s surface."""

        protocol_version = "HTTP/1.1"

        def _answer(self, method: str) -> None:
            """Answer one request, whatever it was reached by."""
            length = int(self.headers.get("Content-Length") or 0)
            body = self.rfile.read(length) if length else b""
            status, document = substitute.answer(method, urlsplit(self.path).path, body)
            payload = json.dumps(document).encode()
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)

        def do_GET(self) -> None:
            """A read."""
            self._answer("GET")

        def do_POST(self) -> None:
            """A request that changes something."""
            self._answer("POST")

        def do_PUT(self) -> None:
            """A write."""
            self._answer("PUT")

        def log_message(self, format: str, *args: object) -> None:
            """Say nothing: what this substitute was asked is asserted, not printed."""

    return Handler
