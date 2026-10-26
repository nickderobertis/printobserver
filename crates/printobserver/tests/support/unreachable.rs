//! A port bound and never listened on, for a tracer to watch a connection to.
//!
//! The tracing suites need a connection the kernel really attempts and refuses,
//! because what they assert is that the attempt was observed; port zero, which
//! every other unreachable address in these suites is, is refused before any
//! attempt on Darwin and Windows. So this binds a port and never listens on it,
//! and a connection to it is refused by the kernel.
//!
//! What holds the port is the platform's bind rules, and they differ. On Linux
//! and Windows no other socket can bind it while it is held. Darwin drops SYNs
//! for an unconnected bound socket, so the reservation is connected instead,
//! and on Darwin a bind setting `SO_REUSEADDR` — as every standard listener
//! does — looks only at unconnected sockets and can take the port. Nothing that
//! needs an address no listener can ever take uses this: those use port zero.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use socket2::{Domain, Socket, Type};

/// A loopback port bound and never listened on, for as long as this lives.
pub struct Unreachable {
    /// Held only so the port stays bound.
    _socket: Socket,
    /// Keeps the reservation connected without ever listening on its port.
    _peer: std::net::TcpStream,
    address: SocketAddr,
}

impl Unreachable {
    /// Bind a free port on IPv4 loopback and hold it without listening.
    pub fn held() -> Self {
        Self::held_on(IpAddr::V4(Ipv4Addr::LOCALHOST))
    }

    /// Bind a free port on `host` and hold it without listening.
    pub fn held_on(host: IpAddr) -> Self {
        let socket = Socket::new(
            Domain::for_address(SocketAddr::new(host, 0)),
            Type::STREAM,
            None,
        )
        .expect("a socket opens");
        socket
            .bind(&SocketAddr::new(host, 0).into())
            .expect("a free port binds");
        let address = socket
            .local_addr()
            .expect("the bound address reads")
            .as_socket()
            .expect("the bound address is an internet address");
        // Darwin silently drops SYNs for an unconnected bound socket. An
        // established socket reserves the same port without matching a new
        // caller's peer tuple, so that caller receives a reset on every host.
        // Neither end is closed until the reservation is released.
        let listener =
            std::net::TcpListener::bind((address.ip(), 0)).expect("a peer listener binds");
        socket
            .connect(&listener.local_addr().expect("the peer address").into())
            .expect("the reservation connects to its peer");
        let (peer, _) = listener.accept().expect("the peer accepts the reservation");
        Self {
            _socket: socket,
            _peer: peer,
            address,
        }
    }

    /// The address, as `host:port`.
    pub fn address(&self) -> SocketAddr {
        self.address
    }
}
