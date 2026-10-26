//! An address nothing listens on, held so that nothing can come to.
//!
//! Binding an ephemeral port, reading it and letting it go hands out an address
//! another process may take before the test connects to it, and a parallel
//! suite binds ephemeral listeners constantly. This binds the port and never
//! listens on it: a connection to it is refused by the kernel, and while the
//! socket lives no other socket can bind the port.

use std::net::{Ipv4Addr, SocketAddr};

use socket2::{Domain, Socket, Type};

/// A loopback port bound and never listened on, for as long as this lives.
pub struct Unreachable {
    /// Held only so the port stays bound.
    _socket: Socket,
    address: SocketAddr,
}

impl Unreachable {
    /// Bind a free loopback port and hold it without listening.
    pub fn held() -> Self {
        let socket =
            Socket::new(Domain::IPV4, Type::STREAM, None).expect("a loopback socket opens");
        socket
            .bind(&SocketAddr::from((Ipv4Addr::LOCALHOST, 0)).into())
            .expect("a loopback port binds");
        let address = socket
            .local_addr()
            .expect("the bound address reads")
            .as_socket()
            .expect("the bound address is an internet address");
        Self {
            _socket: socket,
            address,
        }
    }

    /// The address, as `host:port`.
    pub fn address(&self) -> SocketAddr {
        self.address
    }
}
