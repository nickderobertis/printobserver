//! A supervision turn run by a stood-in harness on the supervisor's own path.
//!
//! The stand-in is `support/harness.rs`'s recording one, a shell script, which
//! is why this is Unix alone: it replaces the paid provider process and only
//! that, is found by name the way the real program is, and records the
//! directory it was run in — which is what a skill's relative links resolve
//! from.

use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use printobserver_server::{HARNESS_DIRECTORY, HarnessSignIn, INGRESS_PATH, SIGN_INS, TOKEN_PARAM};
use printobserver_types::serde_json::{Value, json};

use crate::harness::{SIGN_IN_STATE, SIGNED_IN, TURN_SEEN, assessment_answer, recorded, stand_in};
use crate::world::{SECRET, World};

/// The harness the world's configuration names.
const IDENTITY: &str = "claude-code";

/// How long a turn is waited for once an alert prompted one.
const TURN_BOUND: Duration = Duration::from_secs(120);

/// A stand-in for the configured harness, in a directory of its own.
pub struct HarnessOnPath {
    /// The directory the stand-in is in.
    bin: PathBuf,
}

impl HarnessOnPath {
    /// Put a stand-in for the configured harness under `root`.
    pub fn new(root: &Path) -> Self {
        let bin = root.join("harness-bin");
        stand_in(
            &bin,
            entry(),
            &root.join("invocations"),
            0,
            &assessment_answer(),
        );
        Self { bin }
    }

    /// The whole search path the supervisor runs with: the stand-in's
    /// directory first, then the system directories the stand-in's own shell
    /// commands are in.
    pub fn search_path(&self) -> PathBuf {
        PathBuf::from(format!("{}:/usr/bin:/bin", self.bin.display()))
    }

    /// Sign the stand-in in, prompt a turn with a real failure alert, and
    /// answer the directory that turn's harness was run in.
    pub fn turn_ran_in(world: &World) -> PathBuf {
        let directory = entry().directory(&world.root.path().join("state"));
        let directory = directory.canonicalize().unwrap_or_else(|error| {
            panic!(
                "the server prepared no harness directory at {}: {error}",
                directory.display()
            )
        });
        assert!(
            directory.ends_with(Path::new(HARNESS_DIRECTORY).join(IDENTITY)),
            "the harness directory is not where the sign-in keeps it: {}",
            directory.display()
        );
        std::fs::write(directory.join(SIGNED_IN), format!("{SIGN_IN_STATE}\n"))
            .expect("the stand-in's sign-in is writable");

        let answered = post_a_failure_alert(world);
        assert!(
            answered.contains("202"),
            "the ingress did not take the alert: {answered}"
        );
        let seen = directory.join(TURN_SEEN);
        let deadline = Instant::now() + TURN_BOUND;
        while Instant::now() < deadline {
            if let Ok(turn) = std::fs::read_to_string(&seen) {
                return PathBuf::from(recorded(&turn, "cwd"));
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        panic!("no supervision turn wrote {} in time", seen.display());
    }
}

/// The configured harness, as the adapter's table declares it.
fn entry() -> &'static HarnessSignIn {
    SIGN_INS
        .iter()
        .find(|entry| entry.identity() == IDENTITY)
        .unwrap_or_else(|| panic!("`{IDENTITY}` is not in the adapter's table"))
}

/// Post the committed `Obico` failure alert to the supervisor's own ingress,
/// and answer the status line it came under.
fn post_a_failure_alert(world: &World) -> String {
    let mut alert: Value = printobserver_types::serde_json::from_str(include_str!(
        "../../../printobserver-obico/samples/obico/failure-alert.json"
    ))
    .expect("the committed sample is JSON");
    alert["img_url"] = json!("http://127.0.0.1:9/snapshot.jpg");
    let body = alert.to_string();
    let address = world.proxy.address;
    let mut stream = std::net::TcpStream::connect(address).expect("the supervisor is reachable");
    write!(
        stream,
        "POST {INGRESS_PATH}?{TOKEN_PARAM}={SECRET} HTTP/1.1\r\nHost: {address}\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .expect("the alert is written");
    let mut answer = String::new();
    stream
        .read_to_string(&mut answer)
        .expect("the answer is read");
    answer.lines().next().unwrap_or_default().to_owned()
}
