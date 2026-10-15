//! The adapter's own HTTP client over a real TLS handshake.
//!
//! A snapshot URL is whatever Obico wrote into an alert, so it may be `https`,
//! and the client that fetches it is the adapter's own `rustls`-backed one. This
//! journey stands a real TLS host up on the loopback address — `tokio-rustls`
//! over the same `rustls` — presenting a certificate no root this host trusts
//! has signed, and fetches from it through [`ObicoVision`] exactly as an alert
//! would. What it proves is the handshake: the client spoke TLS, read the
//! certificate the host presented, and refused it, so the host was sent a fatal
//! certificate alert and never one byte of a request.
//!
//! The certificate and its key under `fixtures/tls/` exist only for this host,
//! name `127.0.0.1` alone and are signed by nobody; nothing trusts them.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use printobserver_obico::{ObicoVision, ObicoVisionConfig};
use printobserver_vision_api::{VisionError, VisionPort as _};
use tokio::io::AsyncReadExt as _;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::pki_types::pem::PemObject as _;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio_rustls::rustls::{self, AlertDescription, ServerConfig};

/// The fixture directory holding the host's certificate and key.
fn tls_fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tls")
}

/// The host's TLS configuration, presenting the untrusted certificate.
fn untrusted_host() -> ServerConfig {
    let fixtures = tls_fixtures();
    let chain = vec![
        CertificateDer::from_pem_file(fixtures.join("cert.pem"))
            .expect("the test certificate is committed"),
    ];
    let key =
        PrivateKeyDer::from_pem_file(fixtures.join("key.pem")).expect("the test key is committed");
    ServerConfig::builder_with_provider(Arc::new(rustls::crypto::aws_lc_rs::default_provider()))
        .with_safe_default_protocol_versions()
        .expect("the provider supports the default versions")
        .with_no_client_auth()
        .with_single_cert(chain, key)
        .expect("the certificate and key match")
}

/// Accept one connection over TLS and answer how much of a request it was
/// sent, or how its handshake failed.
async fn serving_one() -> (String, JoinHandle<io::Result<usize>>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a loopback port");
    let address = listener.local_addr().expect("the bound address");
    let acceptor = TlsAcceptor::from(Arc::new(untrusted_host()));
    let serving = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("the client connects");
        let mut tls = acceptor.accept(stream).await?;
        let mut request = [0_u8; 1024];
        tls.read(&mut request).await
    });
    (format!("https://{address}/snapshot.jpg"), serving)
}

/// A snapshot served under a certificate nobody signed is refused in the
/// handshake: the fetch answers unreachable, and the host is told why by the
/// client's own certificate alert rather than sent a request.
#[tokio::test]
async fn a_snapshot_host_whose_certificate_nobody_signed_is_refused_in_the_handshake() {
    let (url, serving) = serving_one().await;
    let vision = ObicoVision::new(ObicoVisionConfig::default()).expect("the adapter builds");

    let fetched = vision.fetch_image(url).await;
    let ended = serving.await.expect("the host's task ends");
    let refusal = ended
        .as_ref()
        .err()
        .and_then(io::Error::get_ref)
        .and_then(|inner| inner.downcast_ref::<rustls::Error>());

    assert!(
        matches!(fetched, Err(VisionError::Unreachable { .. })),
        "the fetch is refused as unreachable, not {fetched:?}"
    );
    assert!(
        matches!(
            refusal,
            Some(rustls::Error::AlertReceived(
                AlertDescription::UnknownCA
                    | AlertDescription::BadCertificate
                    | AlertDescription::CertificateUnknown
            ))
        ),
        "the client sent a certificate alert, not {ended:?}"
    );
}
