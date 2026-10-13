//! One native controller per test process, shared by both command walks.
//!
//! Rust remains safe: the test-only worker contains the Windows ABI boundary.
//! Its named host mutex is independent of TEMP, and a closed, flushed ETL chunk
//! is decoded synchronously before an invocation can return its evidence.

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::SocketAddr;
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::Mutex;

use printobserver_types::serde_json::{Value, json};
use tempfile::TempDir;

/// The private worker protocol's version and checked-in frame goldens.
fn protocol() -> Value {
    printobserver_types::serde_json::from_str(include_str!("etw-protocol.json"))
        .expect("the committed ETW protocol golden")
}

/// The helper releases its logger in its stdin-EOF finally block when this
/// process exits. Requests are serialized; ownership spans callers' threads.
static SESSION: Mutex<Option<Session>> = Mutex::new(None);

/// Recover poison only after discarding the refused controller, never its evidence.
fn with_session<R>(operation: impl FnOnce(&mut Option<Session>) -> R) -> R {
    let mut slot = SESSION.lock().unwrap_or_else(|poisoned| {
        let mut slot = poisoned.into_inner();
        discard(&mut slot);
        SESSION.clear_poison();
        slot
    });
    operation(&mut slot)
}

/// Refused observations cannot be retained, including through a poisoned lock.
fn discard(slot: &mut Option<Session>) {
    if let Some(session) = slot.as_mut() {
        session.failed = true;
    }
    drop(slot.take());
}

/// Run under this process's shared session, releasing it on any failure.
pub fn run(
    program: &Path,
    arguments: &[String],
    environment: &[(String, String)],
) -> (std::process::Output, BTreeSet<SocketAddr>) {
    let (pending, connected, identity) = with_session(|session| {
        // The mutable slot cannot survive a caught unwind: Err discards the
        // controller before rethrowing while the lock is still held. No caller
        // can resume observing through its partially mutated native state.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let session = session.get_or_insert_with(|| {
                super::spike::timed("traced:etw-native-start", Session::start)
            });
            let (pending, connected) = session.observe(program, arguments, environment);
            (pending, connected, session.identity.clone())
        }));
        match result {
            Ok(ran) => ran,
            Err(error) => {
                discard(session);
                std::panic::resume_unwind(error);
            }
        }
    });
    // Join output drains after releasing the invocation lock, so no caller
    // holds it while waiting for another Rust thread.
    // PendingOutput moves into the closure, so its consumed handles and buffers
    // cannot be reused after an unwind. Err discards the matching controller
    // before rethrowing, without discarding a newer concurrent controller.
    let output = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| pending.finish()));
    match output {
        Ok(output) => (output, connected),
        Err(error) => {
            with_session(|slot| {
                if slot
                    .as_ref()
                    .is_some_and(|session| session.identity == identity)
                {
                    discard(slot);
                }
            });
            std::panic::resume_unwind(error);
        }
    }
}

/// The persistent controller and its private ETL chunks.
#[derive(Debug)]
struct Session {
    /// The helper is always one we started and may stop ourselves.
    worker: Child,
    /// The persistent native helper identity, independent of invocation logger leases.
    identity: String,
    /// An already-refused observation must not cause a second shutdown panic.
    failed: bool,
    /// Closing this pipe also releases the session if the parent dies.
    requests: Option<ChildStdin>,
    /// One complete JSON response per synchronous controller operation.
    responses: BufReader<ChildStdout>,
    /// Keeps all worker sources and rotating files alive until shutdown.
    scratch: TempDir,
}

impl Session {
    /// Compile the test-only ABI bridge once; prepare acquires the host logger.
    fn start() -> Self {
        Self::start_with_protocol(include_str!("etw-protocol.json"))
    }

    /// Use the real worker with a protocol file, including the compatibility
    /// regression that gives it a different version of the actual contract.
    fn start_with_protocol(contract: &str) -> Self {
        let scratch = TempDir::new().expect("native ETW controller scratch");
        let script = scratch.path().join("etw.ps1");
        for (name, source) in [
            ("etw.ps1", include_str!("etw.ps1")),
            ("etw.cs", include_str!("etw.cs")),
            ("etw-protocol.json", contract),
        ] {
            std::fs::write(scratch.path().join(name), source).expect("native ETW worker source");
        }
        let mut worker = Command::new("pwsh")
            .args(["-NoProfile", "-NonInteractive", "-File"])
            .arg(script)
            .arg("-Scratch")
            .arg(scratch.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("PowerShell 7 could not start the native ETW controller");
        let requests = worker.stdin.take().expect("controller request pipe");
        let responses = BufReader::new(worker.stdout.take().expect("controller response pipe"));
        let mut session = Self {
            worker,
            identity: String::new(),
            failed: false,
            requests: Some(requests),
            responses,
            scratch,
        };
        let ready = session.response("ready");
        // llmlint: ignore-block[changed_behavior_has_e2e] The real worker serializes its controller Guid using the fixed Guid string formatter and validates its response against the golden. Invalid GUID types or spellings require a tracer serializer double, forbidden here. Real controller restart verifies different valid native controller identities.
        let identity = ready["controller_guid"]
            .as_str()
            .expect("native controller GUID");
        assert!(
            identity.len() == 36
                && identity.bytes().enumerate().all(|(index, byte)| {
                    if [8, 13, 18, 23].contains(&index) {
                        byte == b'-'
                    } else {
                        byte.is_ascii_hexdigit()
                    }
                }),
            "invalid native controller GUID"
        );
        // llmlint: ignore-end[changed_behavior_has_e2e]
        identity.clone_into(&mut session.identity);
        session
    }

    /// Refuse EOF, malformed responses, and native controller failures.
    fn response(&mut self, expected: &str) -> Value {
        let mut line = String::new();
        assert!(
            self.responses
                .read_line(&mut line)
                .expect("ETW response read")
                > 0,
            "incomplete ETW observation: native controller exited before {expected}"
        );
        // llmlint: ignore[changed_behavior_has_e2e] The actual worker always uses ConvertTo-Json; no valid input makes it emit malformed JSON. A deterministic invalid reply would substitute the tracer this task forbids. Real worker death exercises incomplete transport, and a real worker using an incompatible protocol file exercises response-version refusal.
        let response: Value = printobserver_types::serde_json::from_str(&line)
            .expect("incomplete ETW observation: malformed controller response");
        assert_eq!(
            response["schema_version"],
            protocol()["schema_version"],
            "unsupported ETW worker protocol"
        );
        assert_eq!(
            response["kind"], expected,
            "incomplete ETW observation: {response}"
        );
        let contract = protocol();
        let expected_fields: BTreeSet<&str> = contract["responses"][expected]
            .as_object()
            .expect("known response")
            .keys()
            .map(String::as_str)
            .chain(std::iter::once("schema_version"))
            .collect();
        let fields: BTreeSet<&str> = response
            .as_object()
            .expect("ETW response object")
            .keys()
            .map(String::as_str)
            .collect();
        // llmlint: ignore[changed_behavior_has_e2e] The actual worker validates every response against this same golden before writing it. Missing or additional fields cannot reach this second defense through a valid operation without changing the tracer's serializer; real protocol-version mismatch and native failure replies exercise refusal without a tracer double.
        assert_eq!(fields, expected_fields, "incomplete ETW response fields");
        response
    }

    /// A request's answer is the worker's native operation completing, never a
    /// timed guess about whether its event buffers have drained.
    fn request(&mut self, request: &Value, expected: &str) -> Value {
        let pipe = self.requests.as_mut().expect("ETW request pipe is open");
        let mut request = request.clone();
        let contract = protocol();
        let kind = request["kind"].as_str().expect("request kind");
        let fields: BTreeSet<&str> = request
            .as_object()
            .expect("request object")
            .keys()
            .map(String::as_str)
            .collect();
        let expected_fields: BTreeSet<&str> = contract["requests"][kind]
            .as_object()
            .expect("known request")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            fields, expected_fields,
            "request differs from committed protocol"
        );
        request["schema_version"] = contract["schema_version"].clone();
        writeln!(pipe, "{request}").expect("incomplete ETW observation: controller request failed");
        pipe.flush().expect("ETW request flush");
        self.response(expected)
    }

    /// Run the actual program, then require its complete process-tree evidence.
    fn observe(
        &mut self,
        program: &Path,
        arguments: &[String],
        environment: &[(String, String)],
    ) -> (PendingOutput, BTreeSet<SocketAddr>) {
        super::spike::timed("traced:etw-native-prepare", || {
            self.request(&json!({ "kind": "prepare" }), "prepared");
        });
        let mut process = Command::new(program);
        process
            .args(arguments)
            .env_remove("PRINTOBSERVER_SERVER")
            .env_remove("PRINTOBSERVER_CREDENTIAL")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (name, value) in environment {
            process.env(name, value);
        }
        let started = std::time::Instant::now();
        let mut child = process
            .spawn()
            .unwrap_or_else(|error| panic!("{} could not be run: {error}", program.display()));
        let root = child.id();
        // Drain concurrently so a full output pipe cannot stop the root.
        // Inherited Windows handles can keep a pipe open after the root exits;
        // native completion must run before joining those drains.
        let out = drain(child.stdout.take().expect("invocation stdout"));
        let err = drain(child.stderr.take().expect("invocation stderr"));
        let status = child.wait().expect("traced invocation exit");
        drop(child);
        super::spike::note("traced:invocation-run", started);
        let evidence = super::spike::timed("traced:etw-native-flush-rotate-decode", || {
            self.request(&json!({ "kind": "finish", "root": root }), "completed")
        });
        let rebuild_seconds: f64 =
            std::fs::read_to_string(self.scratch.path().join("rebuild-seconds"))
                .expect("native rebuild timing")
                .parse()
                .expect("native rebuild seconds");
        super::spike::note(
            "traced:etw-native-rebuild",
            std::time::Instant::now()
                .checked_sub(
                    std::time::Duration::try_from_secs_f64(rebuild_seconds).expect(
                        "native rebuild time must be finite, nonnegative and representable",
                    ),
                )
                .expect("native rebuild time fits the observation clock"),
        );
        // llmlint: ignore-block[changed_behavior_has_e2e] The real worker validates this array against the golden and emits only native IPEndPoint strings. Invalid element types or socket syntax require substituting the tracer serializer, forbidden here. Real IPv4/IPv6 accepted and refused sockets exercise this boundary.
        let connected: BTreeSet<SocketAddr> = evidence["endpoints"]
            .as_array()
            .expect("ETW endpoint array")
            .iter()
            .map(|address| {
                address
                    .as_str()
                    .expect("ETW endpoint text")
                    .parse()
                    .expect("ETW socket endpoint")
            })
            .collect();
        // llmlint: ignore-end[changed_behavior_has_e2e]
        (PendingOutput { status, out, err }, connected)
    }
}

/// Each invocation owns independent readers and its original exit status.
struct PendingOutput {
    status: ExitStatus,
    out: std::thread::JoinHandle<std::io::Result<Vec<u8>>>,
    err: std::thread::JoinHandle<std::io::Result<Vec<u8>>>,
}

impl PendingOutput {
    /// Complete the authoritative output after native process-tree evidence.
    fn finish(self) -> std::process::Output {
        std::process::Output {
            status: self.status,
            stdout: self
                .out
                .join()
                .expect("stdout drain thread")
                .expect("invocation stdout read"),
            stderr: self
                .err
                .join()
                .expect("stderr drain thread")
                .expect("invocation stderr read"),
        }
    }
}

/// Readers own their pipes. On tracing refusal their detached threads release
/// them when the remaining descendant closes its inherited handles.
fn drain(
    mut stream: impl Read + Send + 'static,
) -> std::thread::JoinHandle<std::io::Result<Vec<u8>>> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes)?;
        Ok(bytes)
    })
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(mut requests) = self.requests.take() {
            // EOF also triggers the worker's finally block. Do not turn a
            // failed observation into a second panic while releasing it.
            let _ = writeln!(
                requests,
                "{}",
                json!({ "kind": "quit", "schema_version": protocol()["schema_version"] })
            );
        }
        let status = self
            .worker
            .wait()
            .expect("native ETW worker could not be reaped");
        assert!(
            status.success() || self.failed || std::thread::panicking(),
            "native ETW worker failed during release: {status}"
        );
    }
}

/// Release the real controller before a regression starts another observer process.
#[cfg(test)]
pub fn release() {
    with_session(|slot| drop(slot.take()));
}

/// The regression inspects the identity of the real worker it owns, so two
/// adjacent observations prove helper setup was shared rather than restarted.
#[cfg(test)]
pub fn controller_id() -> String {
    with_session(|slot| {
        slot.as_ref()
            .expect("a session is running")
            .identity
            .clone()
    })
}

/// The real native control-plane boundary: controller death refuses evidence,
/// and a new process recovers the reserved logger from a different TEMP route.
#[cfg(test)]
#[test]
fn a_crashed_controller_is_refused_then_another_process_recovers() {
    let (scratch, program, arguments, environment) = probe();
    let first = super::traced(&program, &arguments, &environment, scratch.path());
    assert_probe(&first, &environment);
    let identity = controller_id();
    with_session(|slot| {
        let session = slot.as_mut().expect("a controller we own");
        session.request(&json!({ "kind": "prepare" }), "prepared");
        session.worker.kill().expect("kill our controller");
        session.worker.wait().expect("reap our crashed controller");
    });
    let failure = std::panic::catch_unwind(|| {
        super::traced(&program, &arguments, &environment, scratch.path())
    })
    .expect_err("a dead native controller cannot supply evidence");
    assert_refusal(failure.as_ref(), "incomplete ETW observation");
    let poisoned = SESSION
        .lock()
        .expect_err("a refused invocation poisoned its lock");
    assert!(
        poisoned.get_ref().is_none(),
        "refused controller survived poisoning"
    );
    drop(poisoned);
    let recovered = super::traced(&program, &arguments, &environment, scratch.path());
    assert_probe(&recovered, &environment);
    assert!(
        !SESSION.is_poisoned(),
        "recovery retained the poisoned lock"
    );
    assert_ne!(
        controller_id(),
        identity,
        "a crashed session was silently reused"
    );
    // Release the recovered session before asking a second real process to
    // acquire the same host mutex under its different TEMP routing.
    with_session(|slot| {
        let session = slot.as_mut().expect("the recovered controller");
        session.request(&json!({ "kind": "prepare" }), "prepared");
        drop(session.requests.take());
        assert!(
            session
                .worker
                .wait()
                .expect("EOF controller release")
                .success()
        );
    });
    with_session(|slot| drop(slot.take()));
    another_process_recovers(&program, &arguments, &environment, scratch.path());
}

/// The actual data-plane boundary: malformed control inputs and a destroyed
/// completed ETL refuse evidence; runaway descendants cannot pass as empty.
#[cfg(test)]
#[test]
fn incomplete_native_evidence_is_refused_then_the_logger_recovers() {
    let (scratch, program, arguments, environment) = probe();
    let first = super::traced(&program, &arguments, &environment, scratch.path());
    assert_probe(&first, &environment);
    with_session(|slot| {
        slot.as_mut()
            .expect("our controller")
            .request(&json!({ "kind": "invalidate" }), "invalidated");
    });
    let incomplete = std::panic::catch_unwind(|| {
        super::traced(&program, &arguments, &environment, scratch.path())
    })
    .expect_err("a truncated ETL chunk cannot supply evidence");
    assert_refusal(incomplete.as_ref(), "incomplete ETW observation");
    a_raw_request_is_refused(
        &json!({ "kind": "prepare", "schema_version": protocol()["schema_version"] }),
        "invocation already active",
    );
    for envelope in [
        json!([{ "kind": "prepare", "schema_version": protocol()["schema_version"] }]),
        json!([{ "kind": "prepare", "schema_version": protocol()["schema_version"] }, { "kind": "quit", "schema_version": protocol()["schema_version"] }]),
        json!([]),
        json!(null),
        json!(true),
        json!(1),
        json!("prepare"),
    ] {
        a_raw_request_is_refused(&envelope, "envelope must be a JSON object");
    }
    for root in [
        json!(true),
        json!("1"),
        json!(1.5),
        json!(0),
        json!(-1),
        json!(4_294_967_296_u64),
    ] {
        a_raw_request_is_refused(
            &json!({ "kind": "finish", "root": root, "schema_version": protocol()["schema_version"] }),
            "positive uint32 JSON integer",
        );
    }
    a_raw_request_is_refused(
        &json!({ "kind": "finish", "schema_version": protocol()["schema_version"] }),
        "committed golden",
    );
    a_raw_request_is_refused(
        &json!({ "kind": "prepare", "schema_version": protocol()["schema_version"].as_u64().expect("protocol version") + 1 }),
        "schema version",
    );
    a_raw_request_is_refused(
        &json!({ "kind": "prepare", "extra": true, "schema_version": protocol()["schema_version"] }),
        "committed golden",
    );
    a_raw_request_is_refused(
        &json!({ "kind": "no-such-operation", "schema_version": protocol()["schema_version"] }),
        "Unknown ETW request kind",
    );
    for kind in [
        json!(["prepare", "quit"]),
        json!(true),
        json!(1),
        Value::Null,
    ] {
        a_raw_request_is_refused(
            &json!({ "kind": kind, "schema_version": protocol()["schema_version"] }),
            "scalar string",
        );
    }
    a_raw_line_is_refused("not JSON", "JSON");
    a_raw_request_is_refused(
        &json!({ "kind": "unclosed", "schema_version": protocol()["schema_version"] }),
        "not closed",
    );
    let mut different = protocol();
    different["schema_version"] = json!(different["schema_version"].as_u64().expect("version") + 1);
    let incompatible =
        std::panic::catch_unwind(|| Session::start_with_protocol(&different.to_string()))
            .expect_err("an incompatible native response cannot supply evidence");
    assert_refusal(incompatible.as_ref(), "unsupported ETW worker protocol");
    a_runaway_is_refused(&program, &arguments, &environment, scratch.path());
    another_process_recovers(&program, &arguments, &environment, scratch.path());
}

/// A reserved refused endpoint and the real socket fixture executable.
#[cfg(test)]
fn probe() -> (
    TempDir,
    std::path::PathBuf,
    Vec<String>,
    Vec<(String, String)>,
) {
    let reservation = std::net::TcpListener::bind("127.0.0.1:0").expect("reserved endpoint");
    let endpoint = reservation.local_addr().expect("refused endpoint");
    drop(reservation);
    (
        TempDir::new().expect("trace probe scratch"),
        std::env::current_exe().expect("test program"),
        vec![
            "--exact".to_owned(),
            "traced::regression::connector".to_owned(),
            "--nocapture".to_owned(),
        ],
        vec![(
            "PRINTOBSERVER_TRACE_CONNECT".to_owned(),
            endpoint.to_string(),
        )],
    )
}

/// The real invocation exited and its endpoint evidence is complete.
#[cfg(test)]
fn assert_probe(ran: &super::Ran, environment: &[(String, String)]) {
    assert_eq!(ran.code, Some(0), "{}", ran.said());
    assert_eq!(
        ran.connected,
        environment[0]
            .1
            .split(',')
            .map(|address| address.parse().expect("endpoint"))
            .collect()
    );
}

/// A refusal contains the reason at the actual boundary that failed.
#[cfg(test)]
fn assert_refusal(failure: &(dyn std::any::Any + Send), expected: &str) {
    let message = failure
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| failure.downcast_ref::<&str>().copied())
        .expect("trace failure text");
    assert!(message.contains(expected), "{message}");
}

/// Write malformed input to the real controller's stdin, without substituting
/// its parser or kernel calls, then release it and prove recovery next time.
#[cfg(test)]
fn a_raw_request_is_refused(request: &Value, expected: &str) {
    a_raw_line_is_refused(&request.to_string(), expected);
}

/// Malformed JSON itself crosses the real worker's stdin parser.
#[cfg(test)]
fn a_raw_line_is_refused(request: &str, expected: &str) {
    let failure = std::panic::catch_unwind(|| {
        with_session(|slot| {
            let session = slot.get_or_insert_with(Session::start);
            session.request(&json!({ "kind": "prepare" }), "prepared");
            writeln!(
                session.requests.as_mut().expect("controller stdin"),
                "{request}"
            )
            .expect("raw native request");
            session.response("completed");
        });
    })
    .expect_err("malformed native input was accepted");
    with_session(|slot| {
        if let Some(session) = slot.as_mut() {
            session.failed = true;
        }
        drop(slot.take());
    });
    assert_refusal(failure.as_ref(), expected);
}

/// The new process runs its own trace through the public interface, with TEMP
/// routed away from this observer's directory and the same host-wide mutex.
#[cfg(test)]
fn another_process_recovers(
    program: &Path,
    arguments: &[String],
    environment: &[(String, String)],
    scratch: &Path,
) {
    let temporary = scratch.join("a-different-temp-route");
    std::fs::create_dir_all(&temporary).expect("the recovery process's TEMP");
    let mut recovery = Command::new(program);
    recovery
        .args(arguments)
        .env("TEMP", &temporary)
        .env("TMP", &temporary)
        .env("PRINTOBSERVER_TRACE_RECOVER", "yes");
    for (name, value) in environment {
        recovery.env(name, value);
    }
    let output = recovery.output().expect("next real observing process");
    assert!(
        output.status.success(),
        "logger recovery failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The actual descendant deadline, driven by an owned sleeping subprocess.
#[cfg(test)]
fn a_runaway_is_refused(
    program: &Path,
    arguments: &[String],
    environment: &[(String, String)],
    scratch: &Path,
) {
    let pid_file = scratch.join("runaway-child.pid");
    let runaway = [
        (
            "PRINTOBSERVER_TRACE_CONNECT".to_owned(),
            environment[0].1.clone(),
        ),
        ("PRINTOBSERVER_TRACE_CHILD".to_owned(), "yes".to_owned()),
        ("PRINTOBSERVER_TRACE_DELAY".to_owned(), "60000".to_owned()),
        ("PRINTOBSERVER_TRACE_INHERIT".to_owned(), "yes".to_owned()),
        (
            "PRINTOBSERVER_TRACE_CHILD_PID".to_owned(),
            pid_file.display().to_string(),
        ),
    ];
    let refused = std::panic::catch_unwind(|| super::traced(program, arguments, &runaway, scratch));
    // This PID was captured at spawn by our own fixture, not discovered by a
    // process-name pattern. Reap that owned descendant even if refusal failed.
    let pid: u32 = std::fs::read_to_string(&pid_file)
        .expect("our fixture's captured child PID")
        .trim()
        .parse()
        .expect("our child's numeric PID");
    assert!(pid > 0, "our child PID must be positive");
    let pid = pid.to_string();
    let stopped = Command::new("taskkill")
        .args(["/PID", pid.as_str(), "/F"])
        .output()
        .expect("stop our captured runaway child");
    assert!(
        stopped.status.success(),
        "{}{}",
        String::from_utf8_lossy(&stopped.stdout),
        String::from_utf8_lossy(&stopped.stderr)
    );
    let failure = refused.expect_err("a runaway descendant cannot supply complete evidence");
    let message = failure
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| failure.downcast_ref::<&str>().copied())
        .expect("trace failure text");
    assert!(message.contains("descendants did not exit"), "{message}");
}
