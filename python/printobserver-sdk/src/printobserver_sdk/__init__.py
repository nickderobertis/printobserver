"""The typed Python client of the printobserver server's HTTP surface.

One method per public operation the server declares, generated from the same
checked-in schemas the server's own routes are folded out of — so this client
and the command-line program expose the same normalized surface because they
come from one description rather than because somebody kept two lists aligned.

`printobserver_sdk.contract` carries the request and response types those
methods take and answer, and is generated. Nothing in it is written by hand,
and `just check-generated` refuses a tree in which it differs from what the
generator writes.

A refused action is an answer rather than a failure of transport: it arrives as
`RejectedError`, carrying the reason, the value asked for and the range allowed, so
a caller can ask again for something inside the range.

Images are a path, never bytes. No method here answers image bytes or an
encoding of them; the server answers an absolute path on **its own** filesystem
and this client hands that path back exactly as it was answered.

    from printobserver_sdk import Client

    client = Client("http://127.0.0.1:8420", "operator")
    status = client.status("0198f0a1-2b3c-7d4e-8f90-123456789abc")
"""

from __future__ import annotations

from printobserver_sdk._client import Client
from printobserver_sdk._surface import (
    ClientError,
    NoReasonError,
    PrinterRefusedError,
    RefusedError,
    RejectedError,
    UnreachableError,
    UnreadableError,
    reason_given,
)
from printobserver_sdk.contract import CONTRACT_VERSION, OPERATION_NAMES

__all__ = [
    "CONTRACT_VERSION",
    "OPERATION_NAMES",
    "Client",
    "ClientError",
    "NoReasonError",
    "PrinterRefusedError",
    "RefusedError",
    "RejectedError",
    "UnreachableError",
    "UnreadableError",
    "reason_given",
]
