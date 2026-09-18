//! Windows `printobserver sign-in`, driven as the real binary.
//!
//! The same three journeys `sign_in.rs` walks on Unix, observed through what
//! Windows offers instead. The recording stand-in for each harness program is a
//! compiled program rather than a shell script, because a Windows process is
//! found by `.exe` and runs no interpreter line; the directory's privacy is not
//! read as a file mode, which Windows has none of; and "nothing else happened"
//! is observed by the same kernel event-tracing session the command-line
//! journeys read their connections out of, first held over a command that does
//! connect so that a session saying nothing below is one that looked.

#![cfg(windows)]

#[path = "support/traced.rs"]
mod traced;

use std::io::Write as _;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use printobserver::failure::Exit;
use printobserver_server::{CLIENT_CONFIG_FILE, HARNESS_DIRECTORY, HarnessSignIn, SIGN_INS};
use tempfile::TempDir;

/// What the operator types at the harness's prompt.
const TYPED: &str = "the-code-from-the-link";

/// What every stand-in's sign-in exits with, unless a journey says otherwise:
/// a status this program declares for nothing of its own, so passing it through
/// is the only way to exit with it.
const HARNESS_STATUS: u8 = 23;

/// The simulated sign-in state file a stand-in's sign-in writes.
const SIGNED_IN: &str = "signed-in";

/// The file a stand-in's sign-in records into, beside the state file.
const SIGN_IN_SEEN: &str = "sign-in-seen";

/// The file a stand-in's supervision turn records into, beside the state file.
const TURN_SEEN: &str = "turn-seen";

/// What a stand-in's sign-in prints on standard output for the line it was
/// answered, before the line itself.
const ANSWERED: &str = "was answered: ";

/// One journey's own root: a state directory, a directory of stand-ins, and
/// the file every stand-in invocation is appended to.
struct Host {
    root: TempDir,
}

impl Host {
    /// A root with a compiled stand-in for every harness program of the table.
    fn with_stand_ins(status: u8) -> Self {
        let host = Self {
            root: TempDir::new().expect("a journey's own root"),
        };
        std::fs::create_dir_all(host.bin()).expect("a directory for the stand-ins");
        std::fs::create_dir_all(host.state()).expect("a state directory");
        for entry in &SIGN_INS {
            stand_in(&host.bin(), entry, &host.invocations(), status);
        }
        host
    }

    /// Where the stand-ins are.
    fn bin(&self) -> PathBuf {
        self.root.path().join("bin")
    }

    /// The file every stand-in invocation appends one line to.
    fn invocations(&self) -> PathBuf {
        self.root.path().join("invocations")
    }

    /// The state directory the configuration names.
    fn state(&self) -> PathBuf {
        self.root.path().join("state")
    }

    /// Write one configuration file under one name, and answer its path.
    fn configuration(&self, name: &str, text: &str) -> PathBuf {
        let path = self.root.path().join(name);
        std::fs::write(&path, text).expect("the configuration is writable");
        path
    }

    /// The configuration naming the state directory and one harness, and
    /// nothing else at all.
    fn only_what_signing_in_reads(&self, harness: &str) -> PathBuf {
        self.configuration(
            "config.toml",
            &format!(
                "state_dir = \"{}\"\n\n[supervisor]\nharness = \"{harness}\"\n",
                toml_path(&self.state())
            ),
        )
    }

    /// Where one identity's sign-in is kept, as the state directory resolves.
    fn directory(&self, entry: &HarnessSignIn) -> PathBuf {
        self.state()
            .canonicalize()
            .expect("the state directory resolves")
            .join(HARNESS_DIRECTORY)
            .join(entry.identity())
    }

    /// Every stand-in invocation, one line each.
    fn invoked(&self) -> Vec<String> {
        std::fs::read_to_string(self.invocations())
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    /// This host's directory of stand-ins first on the caller's own path.
    fn path(&self) -> String {
        std::env::join_paths(std::iter::once(self.bin()).chain(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        )))
        .expect("a Windows PATH")
        .to_string_lossy()
        .into_owned()
    }
}

/// A path as a TOML basic string spells it: every backslash doubled.
fn toml_path(path: &Path) -> String {
    path.display().to_string().replace('\\', "\\\\")
}

/// Compile a recording stand-in for one harness program into `directory`.
///
/// It appends one line to `invocations` however it is asked, so a journey can
/// assert it was never run; on a supervision turn (`-p`) it records beside the
/// state file and exits 0; otherwise it records its arguments, the directory
/// the table's variable named and its working directory into that directory,
/// writes the simulated sign-in state, prompts on standard error, echoes the
/// line it was answered on standard output, and exits with `status`.
fn stand_in(directory: &Path, harness: &HarnessSignIn, invocations: &Path, status: u8) {
    let program = harness.program();
    let variable = harness.config_env();
    let invocations = invocations.display();
    let source = format!(
        r#"
use std::io::{{BufRead as _, Write as _}};
fn main() {{
    let args: Vec<String> = std::env::args().skip(1).collect();
    let joined = args.join(" ");
    if let Ok(mut log) = std::fs::OpenOptions::new().append(true).create(true).open(r"{invocations}") {{
        let _ = writeln!(log, "{program} {{joined}}");
    }}
    if args.first().map(String::as_str) == Some("--version") {{
        println!("0.0.0 (a recording stand-in for {program})");
        return;
    }}
    let dir = std::path::PathBuf::from(std::env::var_os("{variable}").unwrap_or_default());
    if args.first().map(String::as_str) == Some("-p") {{
        std::fs::write(dir.join("{TURN_SEEN}"), "a supervision turn ran\n").expect("the directory is there");
        return;
    }}
    let cwd = std::env::current_dir().expect("a working directory");
    let seen = format!("argv={{joined}}\ndirectory={{}}\ncwd={{}}\n", dir.display(), cwd.display());
    std::fs::write(dir.join("{SIGN_IN_SEEN}"), seen).expect("the directory the variable names is there");
    std::fs::write(dir.join("{SIGNED_IN}"), "a-simulated-sign-in\n").expect("the state is writable");
    eprintln!("{program} is asking for the code it printed a link to");
    let mut answered = String::new();
    if std::io::stdin().lock().read_line(&mut answered).is_ok() && !answered.trim_end().is_empty() {{
        println!("{program} {ANSWERED}{{}}", answered.trim_end());
    }}
    std::process::exit({status});
}}
"#
    );
    let file = directory.join(format!("{program}.rs"));
    std::fs::write(&file, source).expect("the stand-in's source is writable");
    let built = Command::new("rustc")
        .arg(&file)
        .arg("-o")
        .arg(directory.join(format!("{program}.exe")))
        .output()
        .expect("rustc runs");
    assert!(
        built.status.success(),
        "the stand-in for {program} did not build: {}",
        String::from_utf8_lossy(&built.stderr)
    );
}

/// Run `printobserver sign-in` under one configuration, with the host's
/// stand-ins first on the path, writing `typed` to its standard input.
fn signing_in(host: &Host, config: &Path, typed: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_printobserver"))
        .args(["sign-in", "--config"])
        .arg(config)
        .env("PATH", host.path())
        .env_remove("PRINTOBSERVER_SERVER")
        .env_remove("PRINTOBSERVER_CREDENTIAL")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the built program runs");
    {
        let mut stdin = child
            .stdin
            .take()
            .expect("the child's standard input is piped");
        writeln!(stdin, "{typed}").expect("the answer is written");
    }
    child.wait_with_output().expect("the built program exits")
}

/// Every entry of a directory, by name, sorted.
fn names_in(directory: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(directory)
        .expect("the directory is readable")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

/// An address on loopback nothing is listening on.
fn nothing_listening() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a free loopback port");
    let address = listener.local_addr().expect("the bound address");
    drop(listener);
    address.to_string()
}

/// Everything a run said, on either stream.
fn said(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// One `key=value` line a stand-in recorded.
fn recorded(recording: &str, key: &str) -> String {
    recording
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{key}=")))
        .unwrap_or_else(|| panic!("the stand-in recorded no {key}: {recording}"))
        .to_owned()
}

/// Signing in runs the harness's own sign-in with the table's variable naming
/// its directory, on the caller's terminal, and exits with its status.
#[test]
fn signing_in_runs_the_harnesss_own_sign_in_on_the_callers_terminal() {
    for entry in &SIGN_INS {
        let host = Host::with_stand_ins(HARNESS_STATUS);
        let config = host.only_what_signing_in_reads(entry.identity());

        let output = signing_in(&host, &config, TYPED);

        let stdout = String::from_utf8_lossy(&output.stdout);
        assert_eq!(
            output.status.code(),
            Some(i32::from(HARNESS_STATUS)),
            "`{}` did not exit with the harness's own status: {}",
            entry.identity(),
            said(&output)
        );
        // The harness read what the caller typed and wrote to the caller's own
        // output and error: that is an interactive sign-in a person can answer.
        assert!(
            stdout.contains(&format!("{} {ANSWERED}{TYPED}", entry.program())),
            "the harness was not handed the caller's input and output: {stdout}"
        );
        assert!(
            said(&output).contains("is asking for the code"),
            "the harness's prompt did not reach the caller's error: {}",
            said(&output)
        );

        let directory = host.directory(entry);
        assert!(directory.is_dir(), "the harness directory was not created");
        let recording = std::fs::read_to_string(directory.join(SIGN_IN_SEEN))
            .expect("the harness recorded what it was handed");
        assert_eq!(recorded(&recording, "argv"), entry.arguments().join(" "));
        assert_eq!(
            PathBuf::from(recorded(&recording, "directory"))
                .canonicalize()
                .expect("the recorded directory resolves"),
            directory,
            "the harness was pointed somewhere other than its directory"
        );
        assert_eq!(
            PathBuf::from(recorded(&recording, "cwd"))
                .canonicalize()
                .expect("the recorded working directory resolves"),
            directory,
            "the harness ran somewhere other than inside its own directory"
        );
        assert_eq!(
            std::fs::read_to_string(directory.join(SIGNED_IN)).expect("the sign-in was kept"),
            "a-simulated-sign-in\n"
        );

        // Nothing but the sign-in: one invocation of the right program, and
        // nothing a server would have written.
        assert_eq!(
            host.invoked(),
            vec![format!(
                "{} {}",
                entry.program(),
                entry.arguments().join(" ")
            )]
        );
        assert_eq!(names_in(&host.state()), vec![HARNESS_DIRECTORY.to_owned()]);
        assert_eq!(
            names_in(&host.state().join(HARNESS_DIRECTORY)),
            vec![entry.identity().to_owned()]
        );
        assert!(!host.state().join(CLIENT_CONFIG_FILE).exists());
        assert!(
            !directory.join(TURN_SEEN).exists(),
            "a supervision turn ran"
        );
    }
}

/// Signing in reads the state directory and the harness and nothing else: a
/// configuration whose machine and ingress values are unreachable or invalid
/// still signs in, and the program connects to nothing.
#[test]
fn signing_in_reaches_no_printer_and_no_failure_detector() {
    let host = Host::with_stand_ins(0);
    let entry = &SIGN_INS[0];
    let octoprint = nothing_listening();
    let listen = nothing_listening();
    // Every value but the two signing in reads either names an address nothing
    // is listening on or is one the server itself would refuse to start under.
    let config = host.configuration(
        "config.toml",
        &format!(
            "state_dir = \"{}\"\nlisten = \"{listen}\"\n\n[octoprint]\nurl = \"http://{octoprint}\"\n\
             api_key = \"\"\nfan = \"a-fan-nobody-declared\"\n\n[supervisor]\nharness = \"{}\"\n\
             model = \"\"\n\n[ingress]\nshared_secret = \"\"\nanswer_bound_ms = 0\n",
            toml_path(&host.state()),
            entry.identity()
        ),
    );
    let environment = vec![("PATH".to_owned(), host.path())];
    let program = PathBuf::from(env!("CARGO_BIN_EXE_printobserver"));

    // The session is first held over a command that does reach an address, so
    // that a session saying nothing below is one that looked.
    let client = host.configuration(
        "client.toml",
        &format!("[client]\nserver = \"http://{octoprint}\"\n"),
    );
    let reached = traced::traced(
        &program,
        &[
            "status".to_owned(),
            "--print-id".to_owned(),
            "01a08000-0000-7000-8000-000000000001".to_owned(),
            "--config".to_owned(),
            client.display().to_string(),
        ],
        &environment,
        host.root.path(),
    );
    assert_eq!(
        reached.code,
        Some(i32::from(Exit::Unreachable.status())),
        "{}",
        reached.said()
    );
    let expected: std::net::SocketAddr = octoprint.parse().expect("a loopback address");
    assert!(
        reached.connected.contains(&expected),
        "the session did not record `{}` connecting to {octoprint}, so it says nothing \
         about a command that does not: {:?}",
        reached.arguments.join(" "),
        reached.connected
    );

    let signed = traced::traced(
        &program,
        &[
            "sign-in".to_owned(),
            "--config".to_owned(),
            config.display().to_string(),
        ],
        &environment,
        host.root.path(),
    );

    assert_eq!(
        signed.code,
        Some(0),
        "the sign-in did not complete: {}",
        signed.said()
    );
    assert!(
        host.directory(entry).join(SIGNED_IN).is_file(),
        "the harness was not signed in"
    );
    assert_eq!(
        signed.connected,
        std::collections::BTreeSet::new(),
        "signing in connected to something"
    );
    assert!(!host.state().join(CLIENT_CONFIG_FILE).exists());
}

/// A harness outside the table is refused before anything runs, naming it and
/// every harness this program can sign in.
#[test]
fn a_harness_outside_the_table_is_refused_before_anything_runs() {
    let host = Host::with_stand_ins(0);
    let config = host.only_what_signing_in_reads("copilot");

    let output = signing_in(&host, &config, TYPED);

    assert_eq!(
        output.status.code(),
        Some(i32::from(Exit::Unconfigured.status())),
        "a harness outside the table was not refused as configuration: {}",
        said(&output)
    );
    for named in std::iter::once("copilot").chain(SIGN_INS.iter().map(HarnessSignIn::identity)) {
        assert!(
            said(&output).contains(&format!("`{named}`")),
            "the refusal does not name `{named}`: {}",
            said(&output)
        );
    }
    assert_eq!(host.invoked(), Vec::<String>::new(), "a stand-in was run");
    assert!(
        !host.state().join(HARNESS_DIRECTORY).exists(),
        "a directory was made for a harness nothing can sign in"
    );
}
