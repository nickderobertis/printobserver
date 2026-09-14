//! `printobserver sign-in`, driven as the real binary.
//!
//! Every journey here runs the compiled program with a recording stand-in for
//! every harness program of the adapter's table first on its path, so the only
//! thing that is not real is the paid provider process. What is asserted is
//! what a person at a terminal would see and what the machine was left holding:
//! the harness's own sign-in ran with the table's variable naming
//! `<state_dir>/harness/<identity>`, that directory was created private, the
//! harness was handed the caller's own standard input, output and error, and
//! the command exited with the harness's status — and nothing else happened.
//!
//! # "Nothing else happened" is observed rather than assumed
//!
//! Signing in must reach no printer and no failure detector, and must listen on
//! nothing. This journey binds nothing at the addresses the configuration names
//! and records every `connect`, `bind` and `listen` the program makes, through a
//! recorder loaded into it the way the dynamic linker loads any library. A
//! recorder that never records would satisfy that assertion by saying nothing,
//! so the same recorder is first held over a client command pointed at the same
//! kind of address, and that one is required to have been recorded.

#[path = "support/harness.rs"]
mod harness;

use std::io::Write as _;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use printobserver::failure::Exit;
use printobserver_server::{CLIENT_CONFIG_FILE, HARNESS_DIRECTORY, HarnessSignIn, SIGN_INS};
use tempfile::TempDir;

use harness::{
    ANSWERED, SIGN_IN_SEEN, SIGN_IN_STATE, SIGNED_IN, TURN_SEEN, assessment_answer, forking,
    recorded, stand_in,
};

/// What the operator types at the harness's prompt.
const TYPED: &str = "the-code-from-the-link";

/// What every stand-in's sign-in exits with, unless a journey says otherwise:
/// a status this program declares for nothing of its own, so passing it through
/// is the only way to exit with it.
const HARNESS_STATUS: u8 = 23;

/// The variable the network recorder writes to the file it names.
const RECORDING_ENV: &str = "PRINTOBSERVER_NETWORK_RECORDING";

/// One journey's own root: a state directory, a directory of stand-ins, and
/// the file every stand-in invocation is appended to.
struct Host {
    /// The root everything is under.
    root: TempDir,
}

impl Host {
    /// A root with a stand-in for every harness program of the table.
    fn with_stand_ins(status: u8) -> Self {
        let host = Self {
            root: TempDir::new().expect("a journey's own root"),
        };
        for entry in &SIGN_INS {
            stand_in(
                &host.bin(),
                entry,
                &host.invocations(),
                status,
                &assessment_answer(),
            );
        }
        std::fs::create_dir_all(host.state()).expect("a state directory");
        host
    }

    /// A root with no program on its path at all.
    fn bare() -> Self {
        let host = Self {
            root: TempDir::new().expect("a journey's own root"),
        };
        std::fs::create_dir_all(host.bin()).expect("an empty bin directory");
        std::fs::create_dir_all(host.state()).expect("a state directory");
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
                self.state().display()
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
}

/// Run `printobserver sign-in` under one configuration, with the host's
/// directory first on the path, writing `typed` to its standard input.
fn signing_in(host: &Host, config: &Path, typed: &str, environment: &[(&str, &Path)]) -> Output {
    let mut process = Command::new(env!("CARGO_BIN_EXE_printobserver"));
    process
        .args(["sign-in", "--config"])
        .arg(config)
        .env("PATH", format!("{}:/usr/bin:/bin", host.bin().display()))
        .env_remove("PRINTOBSERVER_SERVER")
        .env_remove("PRINTOBSERVER_CREDENTIAL")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // A variable the caller already carries would satisfy every assertion about
    // what the harness was handed without this program handing it anything.
    for entry in &SIGN_INS {
        process.env_remove(entry.config_env());
    }
    for (name, value) in environment {
        process.env(name, value);
    }
    let mut child = {
        let _held = forking();
        process.spawn().expect("the built program runs")
    };
    child
        .stdin
        .take()
        .expect("the program's standard input")
        .write_all(format!("{typed}\n").as_bytes())
        .expect("the answer is written");
    child.wait_with_output().expect("the program finishes")
}

/// The mode bits of one path.
fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::metadata(path)
        .unwrap_or_else(|error| panic!("{} is not there: {error}", path.display()))
        .permissions()
        .mode()
        & 0o777
}

/// The names one directory holds.
fn names_in(directory: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("{} cannot be listed: {error}", directory.display()))
        .map(|entry| {
            entry
                .expect("an entry reads")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

/// An address on this host nothing is listening on.
fn nothing_listening() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    listener
        .local_addr()
        .expect("the bound address")
        .to_string()
}

/// The standard error of one run, as text.
fn said(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Signing in runs the harness's own sign-in with the table's variable naming
/// its directory, on the caller's terminal, and exits with its status.
#[test]
fn signing_in_runs_the_harnesss_own_sign_in_on_the_callers_terminal() {
    for entry in &SIGN_INS {
        let host = Host::with_stand_ins(HARNESS_STATUS);
        let config = host.only_what_signing_in_reads(entry.identity());

        let output = signing_in(&host, &config, TYPED, &[]);

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
        assert_eq!(
            mode(&directory),
            0o700,
            "the harness directory is not private"
        );
        let recording = std::fs::read_to_string(directory.join(SIGN_IN_SEEN))
            .expect("the harness recorded what it was handed");
        assert_eq!(recorded(&recording, "argv"), entry.arguments().join(" "));
        assert_eq!(
            PathBuf::from(recorded(&recording, "directory")),
            directory,
            "the harness was pointed somewhere other than its directory"
        );
        assert_eq!(
            PathBuf::from(recorded(&recording, "cwd")),
            directory,
            "the harness ran somewhere other than inside its own directory"
        );
        let stdin = recorded(&recording, "stdin");
        assert!(
            stdin.starts_with("pipe:") && stdin == recorded(&recording, "parent_stdin"),
            "the harness's standard input is not the caller's own: {recording}"
        );
        assert_eq!(
            std::fs::read_to_string(directory.join(SIGNED_IN)).expect("the sign-in was kept"),
            format!("{SIGN_IN_STATE}\n")
        );

        // Nothing but the sign-in: one invocation of the right program, no
        // socket held while it ran, and nothing a server would have written.
        assert_eq!(
            host.invoked(),
            vec![format!(
                "{} {}",
                entry.program(),
                entry.arguments().join(" ")
            )]
        );
        assert_eq!(recorded(&recording, "parent_sockets"), "0");
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

/// The shared object that records every `connect`, `bind` and `listen` the
/// process it is loaded into makes, with the program that made it.
const RECORDER: &str = r#"
#define _GNU_SOURCE
#include <dlfcn.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/socket.h>
#include <unistd.h>

static void noted(const char *call) {
    const char *path = getenv("PRINTOBSERVER_NETWORK_RECORDING");
    char program[4096];
    char line[4200];
    ssize_t length;
    int written;
    int fd;
    if (path == NULL) {
        return;
    }
    length = readlink("/proc/self/exe", program, sizeof program - 1);
    program[length < 0 ? 0 : length] = '\0';
    written = snprintf(line, sizeof line, "%s %s\n", call, program);
    fd = open(path, O_WRONLY | O_CREAT | O_APPEND, 0600);
    if (fd < 0) {
        return;
    }
    if (written > 0 && write(fd, line, (size_t)written) < 0) {
        written = 0;
    }
    close(fd);
}

int connect(int fd, const struct sockaddr *address, socklen_t length) {
    int (*real)(int, const struct sockaddr *, socklen_t) = dlsym(RTLD_NEXT, "connect");
    noted("connect");
    return real(fd, address, length);
}

int bind(int fd, const struct sockaddr *address, socklen_t length) {
    int (*real)(int, const struct sockaddr *, socklen_t) = dlsym(RTLD_NEXT, "bind");
    noted("bind");
    return real(fd, address, length);
}

int listen(int fd, int backlog) {
    int (*real)(int, int) = dlsym(RTLD_NEXT, "listen");
    noted("listen");
    return real(fd, backlog);
}
"#;

/// Build the recorder under one root, and answer the library's path.
fn recorder(root: &Path) -> PathBuf {
    let source = root.join("recorder.c");
    let library = root.join("recorder.so");
    std::fs::write(&source, RECORDER).expect("the recorder's source is writable");
    let _held = forking();
    let built = Command::new("cc")
        .args(["-shared", "-fPIC", "-o"])
        .arg(&library)
        .arg(&source)
        .arg("-ldl")
        .output()
        .expect("a C compiler runs");
    assert!(
        built.status.success(),
        "the network recorder did not build: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    library
}

/// What the recorder wrote down about this program, and nothing it wrote about
/// the stand-in harness, which is a shell and not this program.
fn this_programs_network_calls(recording: &Path) -> Vec<String> {
    let program = PathBuf::from(env!("CARGO_BIN_EXE_printobserver"))
        .canonicalize()
        .expect("the built program resolves");
    std::fs::read_to_string(recording)
        .unwrap_or_default()
        .lines()
        .filter(|line| line.ends_with(&program.display().to_string()))
        .map(str::to_owned)
        .collect()
}

/// Signing in reads the state directory and the harness and nothing else: a
/// configuration whose machine and ingress values are unreachable or invalid
/// still signs in, and the program connects to nothing and listens on nothing.
#[test]
fn signing_in_reaches_no_printer_and_no_failure_detector() {
    let host = Host::with_stand_ins(0);
    let library = recorder(host.root.path());
    let recording = host.root.path().join("network");
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
            host.state().display(),
            entry.identity()
        ),
    );

    // The recorder is first held over a command that does reach an address, so
    // that a recording saying nothing below is a recorder that looked.
    let client = host.configuration(
        "client.toml",
        &format!("[client]\nserver = \"http://{octoprint}\"\n"),
    );
    let reached = {
        let _held = forking();
        Command::new(env!("CARGO_BIN_EXE_printobserver"))
            .args([
                "status",
                "--print-id",
                "01a08000-0000-7000-8000-000000000001",
            ])
            .arg("--config")
            .arg(&client)
            .env_remove("PRINTOBSERVER_SERVER")
            .env_remove("PRINTOBSERVER_CREDENTIAL")
            .env("LD_PRELOAD", &library)
            .env(RECORDING_ENV, &recording)
            .output()
            .expect("the built program runs")
    };
    assert_eq!(
        reached.status.code(),
        Some(i32::from(Exit::Unreachable.status()))
    );
    assert!(
        this_programs_network_calls(&recording)
            .iter()
            .any(|line| line.starts_with("connect ")),
        "the recorder did not record a command that connects, so it says nothing \
         about one that does not"
    );
    // The control run's recording is removed, so nothing it wrote down can be
    // read as something the sign-in did.
    std::fs::remove_file(&recording).expect("the control recording is removable");

    let output = signing_in(
        &host,
        &config,
        TYPED,
        &[("LD_PRELOAD", &library), (RECORDING_ENV, &recording)],
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "the sign-in did not complete: {}",
        said(&output)
    );
    assert!(
        host.directory(entry).join(SIGNED_IN).is_file(),
        "the harness was not signed in"
    );
    assert_eq!(
        this_programs_network_calls(&recording),
        Vec::<String>::new(),
        "signing in connected to, bound or listened on something"
    );
    assert!(!host.state().join(CLIENT_CONFIG_FILE).exists());
}

/// A harness outside the table is refused before anything runs, naming it and
/// every harness this program can sign in.
#[test]
fn a_harness_outside_the_table_is_refused_before_anything_runs() {
    let host = Host::with_stand_ins(0);
    let config = host.only_what_signing_in_reads("copilot");

    let output = signing_in(&host, &config, TYPED, &[]);

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
        "a harness directory was created for a harness nothing signs in"
    );
}

/// A configuration signing in cannot read the two values out of is refused,
/// naming what is missing, before anything runs.
#[test]
fn a_configuration_without_the_values_signing_in_reads_is_refused() {
    let host = Host::with_stand_ins(0);
    let without_a_harness = host.configuration(
        "config.toml",
        &format!(
            "state_dir = \"{}\"\n\n[supervisor]\n",
            host.state().display()
        ),
    );
    let absent = host.root.path().join("nowhere.toml");

    for (config, named) in [
        (without_a_harness, "supervisor.harness"),
        (absent, "nowhere.toml"),
    ] {
        let output = signing_in(&host, &config, TYPED, &[]);

        assert_eq!(
            output.status.code(),
            Some(i32::from(Exit::Unconfigured.status())),
            "{}",
            said(&output)
        );
        assert!(
            said(&output).contains(named),
            "the refusal does not name {named}: {}",
            said(&output)
        );
    }
    assert_eq!(host.invoked(), Vec::<String>::new(), "a stand-in was run");
}

/// A harness program nobody installed where the caller's path finds it is
/// refused naming the program and the path.
#[test]
fn a_harness_program_not_on_the_path_is_refused_naming_it() {
    let host = Host::bare();
    let entry = &SIGN_INS[0];
    let config = host.only_what_signing_in_reads(entry.identity());

    let output = signing_in(&host, &config, TYPED, &[]);

    assert_eq!(
        output.status.code(),
        Some(i32::from(Exit::Unconfigured.status())),
        "{}",
        said(&output)
    );
    assert!(
        said(&output).contains(&format!("`{}`", entry.program())) && said(&output).contains("PATH"),
        "the refusal does not say what to install where: {}",
        said(&output)
    );
}
