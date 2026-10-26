//! An address nothing listens on, held so that nothing can come to.
//!
//! Binding an ephemeral port, reading it and letting it go hands out an address
//! another process may take before the test connects to it, and a parallel
//! suite binds ephemeral listeners constantly. This binds the port and never
//! listens on it: a connection to it is refused by the kernel, and while the
//! socket lives nothing binding without `SO_REUSEADDR` can take the port and no
//! listener asking for port zero is handed it.

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
        bind_reserved(&socket, host);
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

/// Where the reservation looks for a port, and across how many: every port in
/// it lies below each supported host's ephemeral range — Linux's starts at
/// 32768, Darwin's and Windows' at 49152 — so a listener asking for port zero
/// is never handed one of them.
#[cfg(not(windows))]
const RESERVED_FROM: u16 = 20_000;
#[cfg(not(windows))]
const RESERVED_SPAN: u16 = 12_000;

/// Bind `socket` to a port on `host` no listener asking for port zero can be
/// handed.
///
/// Darwin's bind for a socket setting `SO_REUSEADDR`, as every standard
/// listener does, looks only at unconnected sockets, so it would hand out a
/// port the connected reservation holds. A port below every ephemeral range is
/// never handed out, and binding one without `SO_REUSEADDR` is refused while
/// any socket holds it, so the first free one is taken.
#[cfg(not(windows))]
fn bind_reserved(socket: &Socket, host: IpAddr) {
    use std::sync::atomic::{AtomicU32, Ordering};

    static TAKEN: AtomicU32 = AtomicU32::new(0);
    let span = u32::from(RESERVED_SPAN);
    let start = std::process::id()
        .wrapping_mul(7919)
        .wrapping_add(TAKEN.fetch_add(1, Ordering::Relaxed).wrapping_mul(131));
    for step in 0..span {
        let offset = u16::try_from(start.wrapping_add(step) % span).expect("inside the span");
        let port = RESERVED_FROM + offset;
        match socket.bind(&SocketAddr::new(host, port).into()) {
            Ok(()) => return,
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {}
            Err(error) => panic!("{host}:{port} cannot be bound: {error}"),
        }
    }
    panic!("no port from {RESERVED_FROM} for {RESERVED_SPAN} is free on {host}");
}

/// Bind `socket` to a free port on `host`: Windows hands no listener a port a
/// socket still holds.
#[cfg(windows)]
fn bind_reserved(socket: &Socket, host: IpAddr) {
    socket
        .bind(&SocketAddr::new(host, 0).into())
        .expect("a free port binds");
}
