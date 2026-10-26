"""The one way a loopback HTTP stand-in is bound: by its number, asking no name service.

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
import functools
import socket
import socketserver
import threading
from collections.abc import Callable, Iterator
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

#: The loopback address every stand-in binds unless it names another.
LOOPBACK = "127.0.0.1"

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
