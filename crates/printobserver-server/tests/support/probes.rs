//! The two addresses the configuration walk points the machine at.
//!
//! That walk drives the real composition root, which asks the machine whether
//! the address and the key are the ones it answers to. So it needs an address
//! that *answers* and an address nothing is at, and neither of them is an
//! `OctoPrint`: nothing here carries an `OctoPrint` path, header or response
//! shape, because only the adapter crate may construct one.

use std::net::{Ipv4Addr, SocketAddr};

use socket2::{Domain, Socket, Type};

use crate::http_host::Host;

/// A host that answers, and says nothing else.
pub async fn silent_host() -> Host {
    Host::serving("application/json", b"{}".to_vec()).await
}

/// A host that refuses every caller, the way an instance refuses a key it does
/// not know.
pub async fn refusing_host() -> Host {
    Host::answering("403 Forbidden", "application/json", b"{}".to_vec()).await
}

/// The base URL a caller reaches one host at.
#[must_use]
pub fn base_url(host: &Host) -> String {
    format!("http://{}", host.address)
}

/// An address nothing is listening on, for as long as this lives.
///
/// The port is bound and never listened on, so what a caller meets is a
/// refused connection rather than a served refusal; and while the socket lives
/// no other socket can bind the port, however many listeners the suite binds
/// beside it.
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
