//! An address nothing answers at, for a tracer to watch a connection to.
//!
//! The tracing suites need a connection attempt the tracer records, refused,
//! because what they assert is that the attempt was observed.
//!
//! Off Windows that is port zero, on the loopback host asked for: nothing can
//! listen on port zero — a listener asking for it is handed some other port —
//! so nothing is bound, held or released for it and nothing can come to take
//! it. `strace` records the `connect` it makes, and the interposer records the
//! address before the call is made, so a refusal there is observed like any
//! other.
//!
//! On Windows the event-tracing session reads the TCP/IP provider's connection
//! requests, and Winsock rejects port zero before any request is made, so the
//! address is a real port instead: bound, connected to a peer and never
//! listened on, for as long as this lives. A connection to it is reset, and no
//! socket can bind the port while it is held except one setting
//! `SO_REUSEADDR`, which no standard listener on Windows sets. `SO_EXCLUSIVEADDRUSE`
//! would close that last case, and nothing in this workspace may set an option
//! `socket2` does not wrap, because `unsafe` is forbidden.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

#[cfg(windows)]
use socket2::{Domain, Socket, Type};

/// An address nothing answers at, for as long as this lives.
pub struct Unreachable {
    /// Held only so the port stays bound.
    #[cfg(windows)]
    _socket: Socket,
    /// Keeps the reservation connected without ever listening on its port.
    #[cfg(windows)]
    _peer: std::net::TcpStream,
    address: SocketAddr,
}

impl Unreachable {
    /// An address on IPv4 loopback nothing answers at.
    pub fn held() -> Self {
        Self::held_on(IpAddr::V4(Ipv4Addr::LOCALHOST))
    }

    /// An address on `host` nothing answers at: port zero.
    #[cfg(not(windows))]
    pub fn held_on(host: IpAddr) -> Self {
        Self {
            address: SocketAddr::new(host, 0),
        }
    }

    /// A port on `host` bound and held without listening.
    #[cfg(windows)]
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
        // An established socket reserves the port without matching a new
        // caller's peer tuple, so that caller receives a reset. Neither end is
        // closed until the reservation is released.
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
