//! The two addresses the configuration walk points the machine at.
//!
//! That walk drives the real composition root, which asks the machine whether
//! the address and the key are the ones it answers to. So it needs an address
//! that *answers* and an address nothing is at, and neither of them is an
//! `OctoPrint`: nothing here carries an `OctoPrint` path, header or response
//! shape, because only the adapter crate may construct one.

use std::net::{Ipv4Addr, SocketAddr};

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

/// An address no server can be listening on: port zero on IPv4 loopback.
///
/// Nothing can listen on port zero — a listener asking for it is handed some
/// other port — so nothing is held and nothing can come to take it. A
/// connection there fails on every host: refused on Linux, and refused before
/// any packet is sent on Darwin and Windows, where port zero is no address to
/// connect to. Every caller here treats any connect failure as nothing
/// answering.
#[must_use]
pub fn unreachable_address() -> SocketAddr {
    SocketAddr::from((Ipv4Addr::LOCALHOST, 0))
}
