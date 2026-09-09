//! The installed service, driven the way the install path installs it.
//!
//! This is the tier that owns the two commands `AGENTS.md`'s "The end-user
//! install path" states after the three routes. It runs the **committed
//! installer** against a throwaway root of its own and asserts four things went
//! in place and nothing was started; it runs the service manager's own verifier
//! over the unit that was written; and it launches the program **exactly as the
//! installed unit's own start command names it** and asks the API a question.
//!
//! Enabling and starting the unit through the service manager is the install
//! path's third command and is the `install-path` job's to establish; what is
//! established here is that the unit the installer wrote is one that manager
//! accepts and whose own start command starts this program.
//!
//! Nothing here uses an HTTP client crate. The one request it makes is written
//! out over a socket, because the point is what the installed program answers
//! rather than what a client library does with it.

use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use tempfile::TempDir;

/// The unit's name, as the install-path section states it. `just check-repo`'s
/// `service-install` holds the tree to that section; this drives what the tree
/// holds.
const UNIT_NAME: &str = "printobserver.service";

/// Where the installer is committed, as that same section states.
const INSTALLER: &str = "scripts/install-service.sh";

/// The programs the installer must not invoke, each shimmed to record being run.
const MUST_NOT_INVOKE: [&str; 3] = ["systemctl", "service", "systemd-run"];

/// The repository root.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// A directory holding shims that record being run and do nothing.
///
/// This is what makes "the installer started nothing" an assertion rather than
/// a hope: an installer that ran `systemctl enable --now` would leave a line in
/// the recording.
fn shims(root: &Path) -> (PathBuf, PathBuf) {
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).expect("a bin directory");
    let recording = root.join("what-was-run");
    for name in MUST_NOT_INVOKE {
        let shim = bin.join(name);
        std::fs::write(
            &shim,
            format!(
                "#!/bin/sh\necho \"{name} $*\" >> \"{}\"\nexit 0\n",
                recording.display()
            ),
        )
        .expect("a shim is writable");
        let mut mode = std::fs::metadata(&shim)
            .expect("the shim is there")
            .permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut mode, 0o755);
        std::fs::set_permissions(&shim, mode).expect("the shim is executable");
    }
    (bin, recording)
}

/// What one run of the installer left behind.
struct Installed {
    /// The root it installed beneath.
    root: PathBuf,
    /// The file the shims record into, which must never come to exist.
    recording: PathBuf,
}

impl Installed {
    /// The unit it wrote.
    fn unit(&self) -> PathBuf {
        self.root.join("etc/systemd/system").join(UNIT_NAME)
    }

    /// The program it put in place.
    fn binary(&self) -> PathBuf {
        self.root.join("usr/local/lib/printobserver/printobserver")
    }

    /// The configuration it installed.
    fn configuration(&self) -> PathBuf {
        self.root.join("etc/printobserver/config.toml")
    }

    /// The state directory it created.
    fn state(&self) -> PathBuf {
        self.root.join("var/lib/printobserver")
    }
}

/// Run the committed installer against a root this journey owns, installing the
/// real `printobserver` program.
fn install(under: &Path) -> Installed {
    let (bin, recording) = shims(under);
    let root = under.join("target-root");
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let run = Command::new(repo_root().join(INSTALLER))
        .arg("--root")
        .arg(&root)
        .arg("--binary")
        .arg(env!("CARGO_BIN_EXE_printobserver"))
        .env("PATH", path)
        .output()
        .expect("the installer runs");
    assert!(
        run.status.success(),
        "the installer failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    Installed { root, recording }
}

/// One `Key=Value` of an installed unit.
fn unit_value(unit: &str, key: &str) -> String {
    unit.lines()
        .find_map(|line| line.strip_prefix(&format!("{key}=")))
        .unwrap_or_else(|| panic!("the installed unit declares no {key}"))
        .trim()
        .to_owned()
}

/// The owner of one path, by name.
fn owner(path: &Path) -> String {
    let listed = Command::new("stat")
        .arg("-c")
        .arg("%U")
        .arg(path)
        .output()
        .expect("stat runs");
    String::from_utf8_lossy(&listed.stdout).trim().to_owned()
}

/// The mode of one path, and whether it carries an access-control entry
/// granting a principal other than its owner anything.
fn permissions(path: &Path) -> (u32, Vec<String>) {
    use std::os::unix::fs::PermissionsExt as _;
    let mode = std::fs::metadata(path)
        .unwrap_or_else(|error| panic!("{} is not there: {error}", path.display()))
        .permissions()
        .mode()
        & 0o777;
    let listed = Command::new("getfacl")
        .arg("--omit-header")
        .arg("--absolute-names")
        .arg(path)
        .output()
        .expect("getfacl runs");
    let base = ["user::", "group::", "other::"];
    let extra = String::from_utf8_lossy(&listed.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter(|line| !base.iter().any(|prefix| line.starts_with(prefix)))
        .map(str::to_owned)
        .collect();
    (mode, extra)
}

/// The installer places four things and starts nothing.
#[test]
fn the_installer_places_four_things_and_starts_nothing() {
    let under = TempDir::new().expect("a journey's own root");
    let installed = install(under.path());

    for (what, path) in [
        ("the program", installed.binary()),
        ("the configuration", installed.configuration()),
        ("the unit", installed.unit()),
    ] {
        assert!(path.is_file(), "{what} is not at {}", path.display());
    }
    assert!(
        installed.state().is_dir(),
        "the state directory is not at {}",
        installed.state().display()
    );

    // The state directory is the service's own user's, and readable by no other
    // principal: the mode denies group and other, and no access-control entry
    // grants anybody else anything.
    let unit = std::fs::read_to_string(installed.unit()).expect("the unit reads");
    assert_eq!(
        owner(&installed.state()),
        unit_value(&unit, "User"),
        "the state directory is not owned by the user the unit runs the service as"
    );
    let (mode, extra) = permissions(&installed.state());
    assert_eq!(
        mode, 0o700,
        "the state directory is mode {mode:o}, which does not deny group and other"
    );
    assert!(
        extra.is_empty(),
        "the state directory carries access-control entries granting another \
         principal: {extra:?}"
    );

    assert!(
        !installed.recording.exists(),
        "the installer started or enabled something: {}",
        std::fs::read_to_string(&installed.recording).unwrap_or_default()
    );
    assert!(
        !installed
            .root
            .join("etc/systemd/system/multi-user.target.wants")
            .exists(),
        "the installer enabled the unit"
    );
}

/// The installed unit is one the service manager accepts.
#[test]
fn the_installed_unit_passes_the_service_managers_own_verifier() {
    let under = TempDir::new().expect("a journey's own root");
    let installed = install(under.path());

    let verified = Command::new("systemd-analyze")
        .arg("verify")
        .arg(installed.unit())
        .output()
        .expect("the service manager's own verifier runs");
    assert!(
        verified.status.success(),
        "the installed unit was refused by its own verifier:\n{}",
        String::from_utf8_lossy(&verified.stderr)
    );

    // A verifier that reported nothing whatever it was handed would satisfy the
    // assertion above and say nothing, so it is driven over a unit that is
    // deliberately not one.
    let malformed = under.path().join("malformed.service");
    std::fs::write(&malformed, "[Service]\nType=nonsense\n").expect("a unit is writable");
    let refused = Command::new("systemd-analyze")
        .arg("verify")
        .arg(&malformed)
        .output()
        .expect("the verifier runs");
    assert!(
        !refused.status.success(),
        "the verifier accepted a unit that is not one, so it says nothing about the \
         installed one"
    );
}

/// The unit's own start command starts a server that answers the API.
#[test]
fn the_units_own_start_command_starts_a_server_that_answers_the_api() {
    let under = TempDir::new().expect("a journey's own root");
    let installed = install(under.path());
    let unit = std::fs::read_to_string(installed.unit()).expect("the unit reads");

    // Read out of the unit rather than written here, so a unit whose start
    // command is wrong fails this journey rather than being masked.
    let start: Vec<String> = unit_value(&unit, "ExecStart")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert_eq!(
        PathBuf::from(&start[0]),
        installed.binary(),
        "the unit's start command names a program the installer did not put there"
    );

    // The installed configuration is a template the operator fills in before
    // enabling the service, so this journey fills in exactly what an operator
    // would and changes nothing else.
    let configuration = PathBuf::from(
        start
            .last()
            .expect("the start command names the configuration it runs under"),
    );
    let answering = silent_host();
    let filled = std::fs::read_to_string(&configuration)
        .expect("the configuration reads")
        .replace("api_key = \"\"", "api_key = \"a-provisioned-key\"")
        .replace(
            "shared_secret = \"\"",
            "shared_secret = \"a-shared-secret\"",
        )
        .replace(
            "url = \"http://127.0.0.1:5000\"",
            &format!("url = \"http://{answering}\""),
        )
        .replace("listen = \"127.0.0.1:8420\"", "listen = \"127.0.0.1:0\"");
    std::fs::write(&configuration, filled).expect("the configuration is writable");

    let mut child = Command::new(&start[0])
        .args(&start[1..])
        .stderr(Stdio::piped())
        .spawn()
        .expect("the unit's own start command runs");
    let address = serving_on(&mut child);

    let answer = ask(&address, &format!("/v1/prints/{}/status", absent_print()));

    assert!(
        answer.contains("HTTP/1.1 404"),
        "the server the unit's own command started did not answer the API:\n{answer}"
    );
    assert!(
        answer.contains("application/json"),
        "the server the unit's own command started answered something that is not \
         JSON:\n{answer}"
    );

    // Stopping it is the signal a service manager stops a unit with, and the
    // program answers it by shutting the server down and exiting successfully —
    // which is what makes `Restart=on-failure` mean what the unit says it does.
    let stopped = Command::new("kill")
        .arg("-TERM")
        .arg(child.id().to_string())
        .status()
        .expect("the signal is sent");
    assert!(stopped.success(), "the signal was not sent");
    let finished = child.wait().expect("the program exits");
    assert!(
        finished.success(),
        "the program did not exit cleanly when it was stopped: {finished:?}"
    );
}

/// An identifier of a print nothing holds, spelled the one way this system
/// spells one: a lowercase hyphenated UUID version 7.
fn absent_print() -> &'static str {
    "01a08000-0000-7000-8000-000000000001"
}

/// Where the started program says it is serving.
fn serving_on(child: &mut Child) -> String {
    let stderr = child.stderr.take().expect("the program's own output");
    let mut lines = BufReader::new(stderr).lines();
    for _ in 0..20 {
        let Some(line) = lines.next() else { break };
        let line = line.expect("the program's output reads");
        if let Some(address) = line.strip_prefix("printobserver is serving on ") {
            return address.trim().to_owned();
        }
        assert!(
            !line.contains("will not start"),
            "the unit's own start command refused to start: {line}"
        );
    }
    panic!("the program never said where it was serving");
}

/// One request, written out over a socket, and the whole answer.
fn ask(address: &str, path: &str) -> String {
    let mut stream = TcpStream::connect(address).expect("the started server accepts a connection");
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    )
    .expect("the request is written");
    let mut answer = String::new();
    stream
        .read_to_string(&mut answer)
        .expect("the answer is read");
    answer
}

/// A host that answers every request and says nothing else.
///
/// The started server asks its configured `OctoPrint` address whether it is the
/// one that answers there, and refuses to start when nothing does. What this
/// journey is about is the unit rather than a printer, so what it points that
/// address at is something that answers.
fn silent_host() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let address = listener.local_addr().expect("the bound address");
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || answer_nothing(stream));
        }
    });
    address
}

/// Read one request and answer an empty document.
fn answer_nothing(mut stream: TcpStream) {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(read) => request.extend_from_slice(&buffer[..read]),
        }
    }
    let _ = stream.write_all(
        b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\
          Connection: close\r\n\r\n{}",
    );
}
