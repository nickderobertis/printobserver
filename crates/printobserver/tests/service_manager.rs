//! The installed service, activated and kept running by its service manager.
//!
//! `service.rs` proves what the installer writes and that the definition's own
//! start command starts a server that answers. What it cannot prove is the two
//! things the operator relies on once they have run the install path's second
//! command and walked away: that the service manager starts the program with
//! nobody launching it, and that it starts it again after it ends abruptly.
//! Those are the service manager's behaviours, so this journey asks the service
//! manager itself — one journey, over whichever manager this host runs services
//! under:
//!
//! 1. the committed installer runs as root, and nothing is running afterwards;
//! 2. the configuration is filled in as an operator would;
//! 3. the definition is activated by the operator's own documented command, read
//!    out of `AGENTS.md`'s pair for this manager;
//! 4. the program comes up, as the service's user, and answers the API — with
//!    this journey never having launched it;
//! 5. the manager reports the definition as one it starts at boot, rather than
//!    one loaded for this session only;
//! 6. the process is killed with `SIGKILL`, and the manager brings a new one up
//!    that answers again;
//! 7. the service is torn down and the host is left as it was found.
//!
//! **Where it runs.** Under systemd it runs inside a container of its own whose
//! first process is systemd: the host's `/usr` read-only, and an `/etc`, `/var`
//! and `/run` of the container's, so the user the installer creates, the unit it
//! writes and the service it enables exist in the container alone and are gone
//! with it. Under launchd there is no such facility, so it runs on the host —
//! which on a continuous-integration runner is a machine of its own — refusing
//! to start on a host already carrying anything it would install, and removing
//! everything it installed afterwards.

#[path = "support/manager.rs"]
mod manager;

use std::io::{Read as _, Write as _};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use manager::{Manager, repo_root};
use printobserver_server::{API_CREDENTIAL_FILE, CLIENT_CONFIG_FILE};
use tempfile::TempDir;

/// Where the installer is committed, as the install-path section states.
const INSTALLER: &str = "scripts/install-service.sh";

/// The places the installer puts things, as the service sees them.
const BINARY: &str = "/usr/local/lib/printobserver/printobserver";
const CONFIGURATION: &str = "/etc/printobserver/config.toml";
const STATE: &str = "/var/lib/printobserver";

/// The user the installer creates when it runs as root.
const SERVICE_USER: &str = "printobserver";

/// How long the manager is given to bring the service up, or back.
const PATIENCE: Duration = Duration::from_secs(120);

/// An identifier of a print nothing holds.
const ABSENT_PRINT: &str = "01a08000-0000-7000-8000-000000000001";

/// Where the journey runs, and how it runs a command as root there.
enum Environment {
    /// A container whose first process is systemd.
    Container {
        /// The container's name.
        name: String,
        /// The image it was started from, removed with it.
        image: String,
        /// Where the image's root file system was staged.
        _staged: TempDir,
    },
    /// This host, under launchd, through a password-free `sudo`.
    Host {
        /// Whether `/var/lib` was here before the installer made it.
        had_var_lib: bool,
    },
}

impl Environment {
    /// Bring the environment for one manager up.
    fn start(manager: Manager) -> Self {
        match manager {
            Manager::Systemd => Self::container(),
            Manager::Launchd => Self::host(),
        }
    }

    /// A container booted into systemd, sharing this host's network so the
    /// service it runs is reachable on loopback.
    fn container() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time is after the epoch")
            .as_nanos();
        let name = format!(
            "printobserver-service-journey-{}-{stamp}",
            std::process::id()
        );
        let image = format!("{name}:root");
        let staged = TempDir::new().expect("a staging directory");
        let archive = stage_root(staged.path());
        let imported = Command::new("docker")
            .args(["import"])
            .arg(&archive)
            .arg(&image)
            .output()
            .unwrap_or_else(|error| {
                panic!(
                    "`docker` could not run, and this journey's systemd runs in a container: \
                     {error}"
                )
            });
        succeeded(&imported, "importing the container's root file system");
        let installer = repo_root()
            .join(INSTALLER)
            .canonicalize()
            .expect("the installer resolves");
        let started = Command::new("docker")
            .args([
                "run",
                "--detach",
                "--name",
                &name,
                "--privileged",
                "--cgroupns=private",
                "--network=host",
                "--tmpfs=/run",
                "--tmpfs=/run/lock",
                "--tmpfs=/tmp",
                "--tmpfs=/usr/local:exec",
                "--volume=/usr:/usr:ro",
                // The certificate authorities this host trusts, which the
                // service's HTTP clients load when they are built.
                "--volume=/etc/ssl:/etc/ssl:ro",
                "--env=container=docker",
            ])
            .arg(format!(
                "--volume={}:/journey/install-service.sh:ro",
                installer.display()
            ))
            .arg(format!(
                "--volume={}:/journey/printobserver:ro",
                env!("CARGO_BIN_EXE_printobserver")
            ))
            .args([&image, "/usr/lib/systemd/systemd"])
            .output()
            .expect("docker runs");
        let environment = Self::Container {
            name,
            image,
            _staged: staged,
        };
        succeeded(
            &started,
            "starting a container whose first process is systemd",
        );
        environment.until(
            Manager::Systemd,
            "systemd to finish starting",
            |environment| {
                let state = environment.root(&["systemctl", "is-system-running"], None);
                let said = String::from_utf8_lossy(&state.stdout).trim().to_owned();
                (said == "running" || said == "degraded").then_some(())
            },
        );
        environment
    }

    /// This host, once it is known to carry nothing the journey would install.
    fn host() -> Self {
        let sudo = Command::new("sudo")
            .args(["-n", "true"])
            .output()
            .expect("sudo runs");
        succeeded(
            &sudo,
            "a password-free sudo, which is how this journey is root under launchd",
        );
        let present = Self::probe(Manager::Launchd);
        let environment = Self::Host {
            had_var_lib: Path::new("/var/lib").exists(),
        };
        assert!(
            present.is_empty(),
            "this host already carries {present:?}, and this journey removes everything \
             it installs; it will not run over an installation it did not make"
        );
        environment
    }

    /// What of an installation this host carries, asked without taking
    /// responsibility for removing it.
    fn probe(manager: Manager) -> Vec<String> {
        let host = std::mem::ManuallyDrop::new(Self::Host { had_var_lib: true });
        host.what_is_installed(manager)
    }

    /// Run one command as root in the environment.
    fn root(&self, argv: &[&str], stdin: Option<&[u8]>) -> Output {
        let mut command = match self {
            Self::Container { name, .. } => {
                let mut command = Command::new("docker");
                command.args(["exec", "--interactive", name]);
                command
            }
            Self::Host { .. } => {
                let mut command = Command::new("sudo");
                command.arg("-n");
                command
            }
        };
        let mut child = command
            .args(argv)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap_or_else(|error| panic!("{argv:?} could not be run as root: {error}"));
        let mut input = child.stdin.take().expect("the command's input");
        if let Some(bytes) = stdin {
            input
                .write_all(bytes)
                .expect("the command's input is written");
        }
        drop(input);
        child.wait_with_output().expect("the command finishes")
    }

    /// Wait, for as long as the manager is given, for one thing to be true of
    /// one manager's service, and say what that manager says of it if it never is.
    fn until<T>(
        &self,
        manager: Manager,
        what: &str,
        mut probe: impl FnMut(&Self) -> Option<T>,
    ) -> T {
        let deadline = Instant::now() + PATIENCE;
        while Instant::now() < deadline {
            if let Some(found) = probe(self) {
                return found;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        panic!(
            "waited {PATIENCE:?} for {what}; {} says:\n{}",
            manager.spelled(),
            diagnosis(self, manager)
        );
    }

    /// Everything of the installation this environment still carries.
    fn what_is_installed(&self, manager: Manager) -> Vec<String> {
        let mut present: Vec<String> = [
            BINARY.to_owned(),
            CONFIGURATION.to_owned(),
            STATE.to_owned(),
            manager.definition(Path::new("/")).display().to_string(),
        ]
        .into_iter()
        .filter(|path| self.root(&["test", "-e", path], None).status.success())
        .collect();
        if self
            .root(&["id", "-u", SERVICE_USER], None)
            .status
            .success()
        {
            present.push(format!("the user {SERVICE_USER}"));
        }
        if state(self, manager).0 {
            present.push(format!("a loaded {} service", manager.spelled()));
        }
        present
    }
}

impl Drop for Environment {
    /// Leave the host as it was found, however the journey ended.
    fn drop(&mut self) {
        match self {
            Self::Container { name, image, .. } => {
                let _ = Command::new("docker")
                    .args(["rm", "--force", name])
                    .output();
                let _ = Command::new("docker")
                    .args(["rmi", "--force", image])
                    .output();
            }
            Self::Host { had_var_lib } => {
                let had_var_lib = *had_var_lib;
                let label = label(Manager::Launchd);
                let _ = self.root(&["launchctl", "bootout", &format!("system/{label}")], None);
                let definition = Manager::Launchd.definition(Path::new("/"));
                let _ = self.root(
                    &[
                        "rm",
                        "-rf",
                        "/usr/local/lib/printobserver",
                        "/etc/printobserver",
                        STATE,
                        &definition.display().to_string(),
                    ],
                    None,
                );
                if !had_var_lib {
                    let _ = self.root(&["rmdir", "/var/lib"], None);
                }
                let _ = self.root(
                    &["dscl", ".", "-delete", &format!("/Users/{SERVICE_USER}")],
                    None,
                );
                let _ = self.root(
                    &["dscl", ".", "-delete", &format!("/Groups/{SERVICE_USER}")],
                    None,
                );
            }
        }
    }
}

/// The root file system of the journey's container, as an archive: the
/// directories and links a Linux root has, and an `/etc` holding root alone, so
/// the user the installer creates is the container's own.
fn stage_root(under: &Path) -> PathBuf {
    let root = under.join("root");
    for directory in [
        "etc/systemd/system",
        "var",
        "tmp",
        "run",
        "root",
        "home",
        "proc",
        "sys",
        "dev",
        "usr",
        "journey",
    ] {
        std::fs::create_dir_all(root.join(directory)).expect("a directory of the image");
    }
    for (link, target) in [
        ("bin", "usr/bin"),
        ("sbin", "usr/sbin"),
        ("lib", "usr/lib"),
        ("lib64", "usr/lib64"),
    ] {
        std::os::unix::fs::symlink(target, root.join(link)).expect("a link of the image");
    }
    for (file, text) in [
        ("etc/passwd", "root:x:0:0:root:/root:/bin/sh\n"),
        ("etc/group", "root:x:0:\n"),
        ("etc/shadow", ""),
        ("etc/gshadow", ""),
        ("etc/machine-id", ""),
        (
            "etc/nsswitch.conf",
            "passwd: files\ngroup: files\nshadow: files\ngshadow: files\n",
        ),
    ] {
        std::fs::write(root.join(file), text).expect("a file of the image");
    }
    let archive = under.join("root.tar");
    succeeded(
        &Command::new("tar")
            .arg("-C")
            .arg(&root)
            .arg("-cf")
            .arg(&archive)
            .arg(".")
            .output()
            .expect("tar runs"),
        "staging the container's root file system",
    );
    archive
}

/// Fail naming what could not be done, with what the command said.
fn succeeded(output: &Output, what: &str) {
    assert!(
        output.status.success(),
        "{what} failed ({:?}):\n{}{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The name the manager knows the service by: the unit's name, or the
/// property list's label — the last word of the install path's start command,
/// less the directory and suffix of a property list.
fn label(manager: Manager) -> String {
    manager
        .definition(Path::new("/"))
        .file_name()
        .and_then(|file| file.to_str())
        .map(|file| file.trim_end_matches(".plist").to_owned())
        .expect("the start command names the service")
}

/// What the manager says of the service: whether it has it loaded at all, and
/// the process it is running it as, if it is running one.
fn state(environment: &Environment, manager: Manager) -> (bool, Option<u32>) {
    match manager {
        Manager::Systemd => {
            let shown = environment.root(
                &[
                    "systemctl",
                    "show",
                    "--property=LoadState,MainPID",
                    &label(manager),
                ],
                None,
            );
            let text = String::from_utf8_lossy(&shown.stdout).into_owned();
            (
                text.contains("LoadState=loaded"),
                text.lines()
                    .find_map(|line| line.strip_prefix("MainPID="))
                    .and_then(|pid| pid.trim().parse().ok())
                    .filter(|pid| *pid != 0),
            )
        }
        Manager::Launchd => {
            let printed = environment.root(
                &["launchctl", "print", &format!("system/{}", label(manager))],
                None,
            );
            (
                printed.status.success(),
                String::from_utf8_lossy(&printed.stdout)
                    .lines()
                    .find_map(|line| line.trim().strip_prefix("pid = "))
                    .and_then(|pid| pid.trim().parse().ok()),
            )
        }
    }
}

/// The process the manager is running the service as, if it is running one.
fn running(environment: &Environment, manager: Manager) -> Option<u32> {
    state(environment, manager).1
}

/// Whether the manager reports the activated service as one it starts at boot.
///
/// systemd says `enabled` of a unit linked into a boot target through
/// `/etc`, and `enabled-runtime` of one enabled for this boot only. launchd
/// reports a service loaded from its boot directory by that path, and lists a
/// service the operator switched off in the domain's disabled set; one loaded
/// from anywhere else is gone at the next boot.
fn starts_at_boot(environment: &Environment, manager: Manager) -> Result<(), String> {
    match manager {
        Manager::Systemd => {
            let answer = environment.root(&["systemctl", "is-enabled", &label(manager)], None);
            let said = String::from_utf8_lossy(&answer.stdout).trim().to_owned();
            (said == "enabled")
                .then_some(())
                .ok_or_else(|| format!("systemctl is-enabled says `{said}`"))
        }
        Manager::Launchd => {
            let label = label(manager);
            let printed =
                environment.root(&["launchctl", "print", &format!("system/{label}")], None);
            let text = String::from_utf8_lossy(&printed.stdout).into_owned();
            let path = manager.definition(Path::new("/"));
            if !text
                .lines()
                .any(|line| line.trim() == format!("path = {}", path.display()))
            {
                return Err(format!(
                    "launchd does not report it loaded from {}:\n{text}",
                    path.display()
                ));
            }
            let disabled = environment.root(&["launchctl", "print-disabled", "system"], None);
            let listed = String::from_utf8_lossy(&disabled.stdout).into_owned();
            let switched_off = listed.lines().any(|line| {
                line.contains(&format!("\"{label}\""))
                    && (line.contains("=> disabled") || line.contains("=> true"))
            });
            (!switched_off)
                .then_some(())
                .ok_or_else(|| format!("launchd lists it as disabled:\n{listed}"))
        }
    }
}

/// A host that answers every request with an empty document, which is what the
/// service is pointed at as its `OctoPrint`: it refuses to start when nothing
/// answers there, and this journey is about the manager rather than a printer.
fn silent_host() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let address = listener.local_addr().expect("the bound address");
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            std::thread::spawn(move || {
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
            });
        }
    });
    address
}

/// Read a file as root, as the operator reads the service's own files.
fn read_as_root(environment: &Environment, path: &str) -> Option<String> {
    let read = environment.root(&["cat", path], None);
    read.status
        .success()
        .then(|| String::from_utf8_lossy(&read.stdout).into_owned())
}

/// Where the running service says it is serving and the credential it serves
/// under — what it wrote for the clients beside it — once it answers the API.
fn answering(environment: &Environment) -> Option<String> {
    let client: toml::Value = toml::from_str(&read_as_root(
        environment,
        &format!("{STATE}/{CLIENT_CONFIG_FILE}"),
    )?)
    .ok()?;
    let server = client["client"]["server"]
        .as_str()?
        .strip_prefix("http://")?;
    let credential = read_as_root(environment, &format!("{STATE}/{API_CREDENTIAL_FILE}"))?;
    let mut stream = TcpStream::connect(server).ok()?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .ok()?;
    write!(
        stream,
        "GET /v1/prints/{ABSENT_PRINT}/status HTTP/1.1\r\nHost: {server}\r\n\
         Accept: application/json\r\nAuthorization: Bearer {}\r\nConnection: close\r\n\r\n",
        credential.trim()
    )
    .ok()?;
    let mut answer = String::new();
    stream.read_to_string(&mut answer).ok()?;
    answer.starts_with("HTTP/1.1 404").then_some(answer)
}

/// Whether one process runs as the user the installer created, compared by
/// user id rather than by a name a process listing may shorten.
fn runs_as_the_service_user(environment: &Environment, pid: u32) -> bool {
    let listed = environment.root(&["ps", "-o", "uid=", "-p", &pid.to_string()], None);
    let user = environment.root(&["id", "-u", SERVICE_USER], None);
    let (running, created) = (
        String::from_utf8_lossy(&listed.stdout).trim().to_owned(),
        String::from_utf8_lossy(&user.stdout).trim().to_owned(),
    );
    !created.is_empty() && running == created
}

/// Everything the manager says about the service, for a failure message.
fn diagnosis(environment: &Environment, manager: Manager) -> String {
    let label = label(manager);
    let asked: Vec<Vec<String>> = match manager {
        Manager::Systemd => vec![
            vec![
                "systemctl".into(),
                "status".into(),
                "--no-pager".into(),
                label.clone(),
            ],
            vec![
                "journalctl".into(),
                "--unit".into(),
                label,
                "--no-pager".into(),
                "--lines=40".into(),
            ],
        ],
        Manager::Launchd => vec![
            vec![
                "launchctl".into(),
                "print".into(),
                format!("system/{label}"),
            ],
            vec!["cat".into(), format!("{STATE}/printobserver.log")],
        ],
    };
    asked
        .iter()
        .map(|argv| {
            let words: Vec<&str> = argv.iter().map(String::as_str).collect();
            let said = environment.root(&words, None);
            format!(
                "$ {}\n{}{}",
                words.join(" "),
                String::from_utf8_lossy(&said.stdout),
                String::from_utf8_lossy(&said.stderr)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The service, installed, activated, killed and brought back by its manager.
fn journey(manager: Manager) {
    let environment = Environment::start(manager);
    let (installer, binary) = match &environment {
        Environment::Container { .. } => (
            PathBuf::from("/journey/install-service.sh"),
            PathBuf::from("/journey/printobserver"),
        ),
        Environment::Host { .. } => (
            repo_root()
                .join(INSTALLER)
                .canonicalize()
                .expect("the installer resolves"),
            PathBuf::from(env!("CARGO_BIN_EXE_printobserver")),
        ),
    };

    install_and_fill_in(&environment, manager, &installer, &binary);
    let first = activate(&environment, manager);
    let second = kill_and_see_it_back(&environment, manager, first);
    assert_ne!(first, second);
    tear_down(environment, manager);
}

/// Steps 1 and 2: the committed installer as root, nothing running afterwards,
/// and the configuration filled in as an operator would.
fn install_and_fill_in(
    environment: &Environment,
    manager: Manager,
    installer: &Path,
    binary: &Path,
) {
    let spelled = manager.spelled();
    // 1. The committed installer, as root, and nothing running afterwards.
    let ran = environment.root(
        &[
            "sh",
            &installer.display().to_string(),
            "--binary",
            &binary.display().to_string(),
        ],
        None,
    );
    succeeded(&ran, "the committed installer");
    assert_eq!(
        running(environment, manager),
        None,
        "the installer left a {spelled} service running"
    );

    // 2. The configuration, filled in as an operator would.
    let filled = read_as_root(environment, CONFIGURATION)
        .expect("the installer wrote a configuration")
        .replace("api_key = \"\"", "api_key = \"a-provisioned-key\"")
        .replace(
            "shared_secret = \"\"",
            "shared_secret = \"a-shared-secret\"",
        )
        .replace(
            "url = \"http://127.0.0.1:5000\"",
            &format!("url = \"http://{}\"", silent_host()),
        )
        .replace("listen = \"127.0.0.1:8420\"", "listen = \"127.0.0.1:0\"");
    succeeded(
        &environment.root(
            &["sh", "-c", "cat > \"$1\"", "sh", CONFIGURATION],
            Some(filled.as_bytes()),
        ),
        "writing the filled-in configuration",
    );
}

/// Steps 3 to 5: the operator's documented command, the program up and answering
/// as the service's user with nobody having launched it, and the definition one
/// the manager starts at boot. Answers the process the manager started.
fn activate(environment: &Environment, manager: Manager) -> u32 {
    let spelled = manager.spelled();
    // 3. The operator's own documented command. This journey is already root,
    // so the `sudo` it opens with is where the operator becomes root and
    // nothing more.
    let pair = manager.pair();
    let documented = pair.last().expect("the pair states a start command");
    let words: Vec<&str> = documented
        .strip_prefix("sudo ")
        .unwrap_or(documented)
        .split_whitespace()
        .collect();
    succeeded(
        &environment.root(&words, None),
        &format!("the documented command `{documented}`"),
    );

    // 4. The program comes up as the service's user and answers, launched by
    // nothing but the manager.
    let first = environment.until(
        manager,
        &format!("{spelled} to bring the service up"),
        |environment| {
            let pid = running(environment, manager)?;
            answering(environment).map(|_| pid)
        },
    );
    assert!(
        runs_as_the_service_user(environment, first),
        "the {spelled} service does not run as the user the installer created"
    );

    // 5. It is one the manager starts at boot.
    if let Err(why) = starts_at_boot(environment, manager) {
        panic!("the activated {spelled} service is not one started at boot: {why}");
    }

    first
}

/// Step 6: the process killed with `SIGKILL`, and the manager bringing a new one
/// up that answers again. Answers that new process.
fn kill_and_see_it_back(environment: &Environment, manager: Manager, first: u32) -> u32 {
    let spelled = manager.spelled();
    // 6. Ended abruptly, and brought back.
    succeeded(
        &environment.root(&["kill", "-KILL", &first.to_string()], None),
        "killing the service",
    );
    let deadline = Instant::now() + PATIENCE;
    let second = loop {
        let now = running(environment, manager);
        if let Some(pid) = now
            .filter(|pid| *pid != first)
            .filter(|_| answering(environment).is_some())
        {
            break pid;
        }
        assert!(
            Instant::now() < deadline,
            "{spelled} did not bring the service back after it was killed:\n{}",
            diagnosis(environment, manager)
        );
        std::thread::sleep(Duration::from_millis(500));
    };
    assert!(
        runs_as_the_service_user(environment, second),
        "the service {spelled} brought back does not run as the service's user"
    );

    second
}

/// Step 7: the service torn down by its manager, and the host left as it was
/// found.
fn tear_down(environment: Environment, manager: Manager) {
    let spelled = manager.spelled();
    // 7. Torn down, and the host as it was found.
    match manager {
        Manager::Systemd => succeeded(
            &environment.root(&["systemctl", "disable", "--now", &label(manager)], None),
            "disabling the service",
        ),
        Manager::Launchd => succeeded(
            &environment.root(
                &[
                    "launchctl",
                    "bootout",
                    &format!("system/{}", label(manager)),
                ],
                None,
            ),
            "booting the service out",
        ),
    }
    assert_eq!(
        running(&environment, manager),
        None,
        "the {spelled} service is still running after it was torn down"
    );
    let (teardown_of, had_var_lib) = match &environment {
        Environment::Container { name, image, .. } => (Some((name.clone(), image.clone())), true),
        Environment::Host { had_var_lib } => (None, *had_var_lib),
    };
    drop(environment);
    if let Some((name, image)) = teardown_of {
        for (what, argv) in [
            ("container", ["container", "inspect", name.as_str()]),
            ("image", ["image", "inspect", image.as_str()]),
        ] {
            assert!(
                !Command::new("docker")
                    .args(argv)
                    .output()
                    .expect("docker runs")
                    .status
                    .success(),
                "the journey's {what} is still on this host"
            );
        }
    } else {
        let present = Environment::probe(manager);
        assert!(
            present.is_empty(),
            "the journey left {present:?} on this host"
        );
        assert_eq!(
            Path::new("/var/lib").exists(),
            had_var_lib,
            "the journey left /var/lib other than it found it"
        );
    }
}

/// The installed service is started by its manager with nobody launching it,
/// and started again by its manager after it ends abruptly.
#[test]
fn the_service_manager_starts_the_service_and_brings_it_back() {
    journey(Manager::host());
}
