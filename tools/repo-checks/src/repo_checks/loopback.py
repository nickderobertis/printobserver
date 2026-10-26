"""The one way a loopback HTTP stand-in is bound, and the one way an address is held unreachable.

`unreachable` is the address nothing listens on: a port bound and never
listened on, held for as long as it is used, because a port bound, read and
let go can be taken by another listener before the connection meant to be
refused is made.

The standard library's HTTP server names itself in `server_bind` with
`socket.getfqdn`, a reverse lookup of the address it just bound. Nothing a
stand-in serves reads that name, and on a hosted macOS runner the lookup waits
out a resolver timeout of about 35 seconds before the stand-in is ready — once
per start. `LoopbackServer` binds through the TCP server and names itself by
the numeric address it bound instead.

It also starts with the name service refused on the thread that builds it, so a
stand-in that would consult one — a subclass binding the standard way, a
handler factory resolving something — fails its start naming the lookup rather
than waiting on a resolver. The refusal is per thread: a server answering on
another thread, or a client resolving a name beside it, is untouched.
"""

from __future__ import annotations

import contextlib
import errno
import functools
import itertools
import os
import socket
import socketserver
import threading
from collections.abc import Callable, Iterator
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

#: The loopback address every stand-in binds unless it names another.
LOOPBACK = "127.0.0.1"

#: Where `unreachable` looks for a port off Windows, and across how many: every
#: port in it lies below each supported host's ephemeral range — Linux's starts
#: at 32768, Darwin's and Windows' at 49152 — so a listener asking for port zero
#: is never handed one of them.
RESERVED_FROM = 20_000
RESERVED_SPAN = 12_000

_reserved = itertools.count()

#: The `socket` functions that consult a name service, refused while a
#: stand-in is being built.
LOOKUPS = (
    "getaddrinfo",
    "getfqdn",
    "gethostbyaddr",
    "gethostbyname",
    "gethostbyname_ex",
    "getnameinfo",
)

_refusing = threading.local()
_installed = threading.Lock()


class NameServiceRefusedError(OSError):
    """A loopback stand-in consulted a name service while it was being started."""


def _refusable(name: str, original: Callable[..., object]) -> Callable[..., object]:
    @functools.wraps(original)
    def lookup(*arguments: object, **keywords: object) -> object:
        if getattr(_refusing, "active", False):
            msg = f"a loopback stand-in looked a name up with socket.{name}{arguments!r}"
            raise NameServiceRefusedError(msg)
        return original(*arguments, **keywords)

    return lookup


def _install() -> None:
    """Put the refusable lookups in `socket` once; each defers to the original elsewhere."""
    with _installed:
        for name in LOOKUPS:
            current = getattr(socket, name)
            if not hasattr(current, "__wrapped__"):
                setattr(socket, name, _refusable(name, current))


@contextlib.contextmanager
def name_service_refused() -> Iterator[None]:
    """Refuse every name lookup made on this thread while the block runs."""
    _install()
    was = getattr(_refusing, "active", False)
    _refusing.active = True
    try:
        yield
    finally:
        _refusing.active = was


class LoopbackServer(ThreadingHTTPServer):
    """An HTTP server on a numeric loopback address, ready without a name lookup."""

    def __init__(
        self,
        handler: type[BaseHTTPRequestHandler],
        port: int = 0,
        host: str = LOOPBACK,
    ) -> None:
        """Bind `host:port` (a free port by default) and get ready to serve `handler`.

        Raises:
            NameServiceRefusedError: If binding consulted a name service.
        """
        with name_service_refused():
            super().__init__((host, port), handler)

    def server_bind(self) -> None:
        """Bind, and name the server by the address it bound rather than by a lookup of it."""
        socketserver.TCPServer.server_bind(self)
        host, port = self.server_address[:2]
        self.server_name = str(host)
        self.server_port = int(port)

    @property
    def url(self) -> str:
        """Where this server answers, as `http://host:port` with no trailing slash."""
        return f"http://{self.server_name}:{self.server_port}"


@contextlib.contextmanager
def unreachable(host: str = LOOPBACK) -> Iterator[tuple[str, int]]:
    """A loopback address bound and never listened on, for as long as the block runs.

    A connection to it is refused, and while the block runs no listener asking
    for port zero is handed the port and nothing binding it without
    `SO_REUSEADDR` can take it — on Windows because the port is held
    exclusively, which a later bind there could otherwise share.
    """
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as held:
        exclusive = getattr(socket, "SO_EXCLUSIVEADDRUSE", None)
        if exclusive is not None:
            held.setsockopt(socket.SOL_SOCKET, exclusive, 1)
            held.bind((host, 0))
        else:
            _bind_reserved(held, host)
        # Darwin drops SYNs matching an unconnected bound TCP socket. Keep an
        # established peer tuple instead: new callers match no connection and
        # receive a reset, while this socket still reserves the port.
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
            listener.bind((host, 0))
            listener.listen(1)
            held.connect(listener.getsockname())
            peer, _ = listener.accept()
        with peer:
            yield host, int(held.getsockname()[1])


def _bind_reserved(held: socket.socket, host: str) -> None:
    """Bind `held` to the first free port on `host` below every ephemeral range.

    Darwin's bind for a socket setting `SO_REUSEADDR`, as the standard servers
    do, looks only at unconnected sockets, so it would hand out a port the
    connected reservation holds. A port below every ephemeral range is never
    handed out, and binding one without `SO_REUSEADDR` is refused while any
    socket holds it.

    Raises:
        OSError: If no port in the range is free, or a bind fails otherwise.
    """
    start = os.getpid() * 7919 + next(_reserved) * 131
    for step in range(RESERVED_SPAN):
        port = RESERVED_FROM + (start + step) % RESERVED_SPAN
        try:
            held.bind((host, port))
        except OSError as refused:
            if refused.errno != errno.EADDRINUSE:
                raise
        else:
            return
    raise OSError(errno.EADDRINUSE, f"no port from {RESERVED_FROM} is free on {host}")
