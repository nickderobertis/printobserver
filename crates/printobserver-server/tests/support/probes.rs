//! The two addresses the configuration walk points the machine at.
//!
//! That walk drives the real composition root, which asks the machine whether
//! the address and the key are the ones it answers to. So it needs an address
//! that *answers* and an address nothing is at, and neither of them is an
//! `OctoPrint`: nothing here carries an `OctoPrint` path, header or response
//! shape, because only the adapter crate may construct one.

use std::net::SocketAddr;

use tokio::net::TcpListener;

use crate::http_host::Host;

/// A host that answers, and says nothing else.
pub async fn silent_host() -> Host {
    Host::serving("application/json", b"{}".to_vec()).await
}

/// The base URL a caller reaches one host at.
#[must_use]
pub fn base_url(host: &Host) -> String {
    format!("http://{}", host.address)
}

/// An address nothing is listening on.
///
/// A port is bound to learn one that is free and then released, so what a
/// caller meets is a refused connection rather than a served refusal.
pub async fn unreachable_address() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a loopback port");
    let address = listener.local_addr().expect("the bound address");
    drop(listener);
    address
}
