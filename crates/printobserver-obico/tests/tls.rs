//! The adapter's own HTTP client over a real TLS handshake.
//!
//! A snapshot URL is whatever Obico wrote into an alert, so it may be `https`,
//! and the client that fetches it is the adapter's own `rustls`-backed one. These
//! journeys stand a real TLS host up on the loopback address — `tokio-rustls`
//! over the same `rustls` — and fetch from it through [`ObicoVision`] exactly as
//! an alert would, with certificate verification exactly as it ships.
//!
//! The certificates under `fixtures/tls/` exist only for these journeys: a test
//! CA, whose key was discarded once it signed, and the host certificate it
//! signed, naming `127.0.0.1` alone. No trust store a host ships holds that CA,
//! so a fetch under the ambient trust store is refused in the handshake.
//!
//! A fetch that is *trusted* reads that CA from a trust store of its own, and
//! the only way in is the one the client already reads: on Linux its platform
//! verifier loads roots through `rustls-native-certs`, which takes
//! `SSL_CERT_FILE`, when it is set, as the whole store. So the trusted journey
//! runs one ignored test of this binary in a child process whose `SSL_CERT_FILE`
//! names the test CA and nothing else — the parent's trust, and the adapter's,
//! untouched. macOS and Windows verify against the operating system's own store,
//! which no process can be handed privately, so that journey is Linux's alone.

use core::time::Duration;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use printobserver_obico::{ObicoVision, ObicoVisionConfig};
use printobserver_vision_api::{VisionError, VisionPort as _};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::pki_types::pem::PemObject as _;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio_rustls::rustls::{self, AlertDescription, ServerConfig};

/// The bytes the host serves as its snapshot.
const SNAPSHOT: &[u8] = b"\xff\xd8\xff\xe0 a snapshot served over TLS \xff\xd9";

/// The longest the host waits for one connection, its handshake and its
/// request, so a client that never arrives fails the journey rather than
/// hanging it.
const HOST_WAIT: Duration = Duration::from_secs(10);

/// The fixture directory holding the test CA and the host's certificate and key.
fn tls_fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tls")
}

/// The adapter as it ships, with a fetch timeout inside the host's own wait.
fn vision() -> ObicoVision {
    ObicoVision::new(ObicoVisionConfig {
        fetch_timeout: Duration::from_secs(5),
        ..ObicoVisionConfig::default()
    })
    .expect("the adapter builds")
}

/// The host's TLS configuration, presenting the certificate the test CA signed.
fn host() -> ServerConfig {
    let fixtures = tls_fixtures();
    let chain = vec![
        CertificateDer::from_pem_file(fixtures.join("host.pem"))
            .expect("the host certificate is committed"),
    ];
    let key = PrivateKeyDer::from_pem_file(fixtures.join("host-key.pem"))
        .expect("the host key is committed");
    ServerConfig::builder_with_provider(Arc::new(rustls::crypto::aws_lc_rs::default_provider()))
        .with_safe_default_protocol_versions()
        .expect("the provider supports the default versions")
        .with_no_client_auth()
        .with_single_cert(chain, key)
        .expect("the certificate and key match")
}

/// Accept one connection over TLS, read its request's head and answer it with
/// the snapshot.
async fn serve_once(listener: TcpListener, acceptor: TlsAcceptor) -> io::Result<String> {
    let (stream, _) = listener.accept().await?;
    let mut tls = acceptor.accept(stream).await?;
    let mut head = Vec::new();
    let mut buffer = [0_u8; 1024];
    while !head.windows(4).any(|window| window == b"\r\n\r\n") {
        let read = tls.read(&mut buffer).await?;
        if read == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        head.extend_from_slice(&buffer[..read]);
    }
    let answer = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        SNAPSHOT.len()
    );
    tls.write_all(answer.as_bytes()).await?;
    tls.write_all(SNAPSHOT).await?;
    tls.shutdown().await?;
    Ok(String::from_utf8_lossy(&head).into_owned())
}

/// Serve one connection, bounded by [`HOST_WAIT`], and answer the snapshot's
/// URL and the head of the request it was sent, or how serving it failed.
async fn serving_one() -> (String, JoinHandle<io::Result<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a loopback port");
    let address = listener.local_addr().expect("the bound address");
    let acceptor = TlsAcceptor::from(Arc::new(host()));
    let serving = tokio::spawn(async move {
        tokio::time::timeout(HOST_WAIT, serve_once(listener, acceptor))
            .await
            .unwrap_or_else(|_| Err(io::ErrorKind::TimedOut.into()))
    });
    (format!("https://{address}/snapshot.jpg"), serving)
}

/// A snapshot served under a certificate no trusted root signed is refused in
/// the handshake: the fetch answers unreachable, and the host is told why by
/// the client's own certificate alert rather than sent a request.
#[tokio::test]
async fn a_snapshot_host_whose_certificate_nobody_trusts_is_refused_in_the_handshake() {
    let (url, serving) = serving_one().await;

    let fetched = vision().fetch_image(url).await;
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

/// The journeys that hand a child process a trust store of its own.
#[cfg(target_os = "linux")]
mod trusted {
    use core::time::Duration;
    use std::process::{Command, Stdio};
    use std::time::Instant;

    use printobserver_types::RawBytes;
    use printobserver_vision_api::VisionPort as _;

    use super::{SNAPSHOT, serving_one, tls_fixtures, vision};

    /// The variable `rustls-native-certs` reads as the whole trust store.
    const TRUST_FILE: &str = "SSL_CERT_FILE";

    /// The variable it reads a directory of roots from, which the child must
    /// not inherit: a directory set beside the file is read instead of it.
    const TRUST_DIR: &str = "SSL_CERT_DIR";

    /// The child's own name for its test, as libtest is asked for it.
    const CHILD: &str = "trusted::the_child_fetches_a_snapshot_over_tls";

    /// The longest the child is given before it is killed and the journey
    /// fails.
    const CHILD_WAIT: Duration = Duration::from_secs(60);

    /// A snapshot served under a certificate the trust store's own CA signed is
    /// fetched over TLS, verified, and its bytes and type read back exactly.
    #[test]
    fn a_snapshot_host_the_trust_store_vouches_for_is_fetched_over_tls() {
        let mut child = Command::new(std::env::current_exe().expect("this test binary has a path"))
            .args([CHILD, "--exact", "--ignored", "--nocapture"])
            .env(TRUST_FILE, tls_fixtures().join("ca.pem"))
            .env_remove(TRUST_DIR)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the child process starts");
        let deadline = Instant::now() + CHILD_WAIT;
        while child
            .try_wait()
            .expect("the child can be waited on")
            .is_none()
        {
            if Instant::now() >= deadline {
                let _ = child.kill();
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let output = child.wait_with_output().expect("the child's output reads");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let reported = format!(
            "exit {:?}\nstdout:\n{stdout}\nstderr:\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );

        assert!(
            output.status.success(),
            "the child's fetch failed: {reported}"
        );
        assert!(
            stdout.contains(&format!("test {CHILD} ... ok")),
            "the child ran its journey: {reported}"
        );
    }

    /// The child's half: run only by the journey above, under the trust store
    /// it was handed.
    #[tokio::test]
    #[ignore = "run in a child process by the journey above, with its own trust store"]
    async fn the_child_fetches_a_snapshot_over_tls() {
        // Named exactly rather than merely set: the parent's own client probes
        // for a system bundle and sets this variable in its environment, which
        // a child handed no store of its own would inherit.
        let trust = std::env::var_os(TRUST_FILE);
        assert_eq!(
            trust.as_deref(),
            Some(tls_fixtures().join("ca.pem").as_os_str()),
            "{TRUST_FILE} names the test CA the journey hands its child"
        );
        let (url, serving) = serving_one().await;

        let fetched = vision().fetch_image(url).await;
        let head = serving.await.expect("the host's task ends");

        let fetched = fetched.expect("the trusted snapshot is fetched");
        assert_eq!(fetched.bytes, RawBytes::new(SNAPSHOT));
        assert_eq!(fetched.content_type, "image/jpeg");
        let head = head.expect("the host served the request");
        assert!(
            head.starts_with("GET /snapshot.jpg HTTP/1.1\r\n"),
            "the host was asked for the snapshot, not {head:?}"
        );
    }
}
