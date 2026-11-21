"""The port a journey's server listens on is one the host never hands anything else.

A journey that kills its server and waits for the manager to bring it back
needs the brought-back process to find its port free. A port the operating
system chose is one of its dynamic ports, which the host hands any outgoing
connection that names none; once the killed server lets it go, a connection on
a busy host can take it, and the brought-back server cannot listen there until
that connection ends. These hold `listen_port` to answering a port outside that
range, on every host the tier runs on.
"""

from __future__ import annotations

import socket

from journey import dynamic_ports, listen_port, worker_ports
from repo_checks.expect import equal, truth


def _chosen_by_the_host() -> int:
    """A port the operating system picks itself, as `bind` to port 0 has it pick one."""
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
        probe.bind(("127.0.0.1", 0))
        return int(probe.getsockname()[1])


def test_the_dynamic_range_read_is_the_one_the_host_hands_ports_out_of() -> None:
    """The range is read from the host, and a port the host picks itself is in it."""
    dynamic = dynamic_ports()
    truth(len(dynamic) > 0, describing=f"this host's dynamic port range to be read: {dynamic}")
    chosen = _chosen_by_the_host()
    truth(
        chosen in dynamic,
        describing=f"a port the host chose ({chosen}) to be in its dynamic range {dynamic}",
    )


def test_a_journeys_listen_port_is_free_and_outside_the_hosts_dynamic_range() -> None:
    """Nothing the host hands out on its own can take the port a server is brought back on."""
    port = listen_port()
    dynamic = dynamic_ports()
    truth(
        port not in dynamic,
        describing=f"the listen port {port} to be outside the dynamic range {dynamic}",
    )
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as server:
        server.bind(("127.0.0.1", port))
        server.listen()
        equal(server.getsockname()[1], port, describing="a server listening on the port answered")


def test_a_port_something_already_holds_is_not_handed_out() -> None:
    """A port another process listens on is passed over, and the next free one answered."""
    held = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    try:
        for candidate in worker_ports():
            try:
                held.bind(("127.0.0.1", candidate))
            except OSError:
                continue
            break
        held.listen()
        taken = held.getsockname()[1]
        answered = [listen_port() for _ in range(3)]
        truth(
            taken not in answered,
            describing=f"the port held elsewhere ({taken}) not among those answered: {answered}",
        )
    finally:
        held.close()


def test_two_asks_before_either_is_bound_answer_two_ports() -> None:
    """A journey that asks for a second port before its server binds the first gets another."""
    first, second = listen_port(), listen_port()
    truth(first != second, describing=f"two different ports: {first} and {second}")
