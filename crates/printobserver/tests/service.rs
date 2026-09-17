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

#[path = "support/harness.rs"]
mod harness;
#[path = "support/manager.rs"]
mod manager;

use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use printobserver::config::{CREDENTIAL_ENV, DEFAULT_CONFIG_PATH, SERVER_ENV};
use printobserver::failure::Exit;
use printobserver_core::store::PrintStore as _;
use printobserver_server::{
    API_CREDENTIAL_FILE, CLIENT_CONFIG_FILE, HARNESS_DIRECTORY, HarnessSignIn, INGRESS_PATH,
    SIGN_INS, TOKEN_PARAM,
};
use tempfile::TempDir;

#[cfg(target_os = "linux")]
use harness::{ANSWERED, SIGN_IN_SEEN};
use manager::{Manager, policy, repo_root};

use harness::{
    SIGN_IN_STATE, SIGNED_IN, TURN_SEEN, assessment_answer, forking, recorded, stand_in,
};

/// The tracer the operator's own route is observed through: every file access
/// the program makes to the paths it is watching, each of which it fails as a
/// file this user is not permitted to read. macOS has no `strace`, and there the
/// interposer observes the same accesses from inside the program.
#[cfg(not(target_os = "macos"))]
const TRACER: &str = "strace";

/// Where the installer is committed, as the install-path section states.
const INSTALLER: &str = "scripts/install-service.sh";

/// Both service managers the installer writes a definition for.
const MANAGERS: [Manager; 2] = [Manager::Systemd, Manager::Launchd];

/// What `uname -s` answers on a host one manager runs services under, which is
/// what the installer chooses between them by.
fn system_of(manager: Manager) -> &'static str {
    match manager {
        Manager::Systemd => "Linux",
        Manager::Launchd => "Darwin",
    }
}

/// The programs the installer must not invoke, each shimmed to record being run.
fn must_not_invoke() -> Vec<String> {
    policy()["service"]["may_not_invoke"]
        .as_array()
        .expect("the policy names what the installer may not invoke")
        .iter()
        .map(|program| program.as_str().expect("a program name").to_owned())
        .collect()
}

/// Write one executable shell script.
fn executable(path: &Path, body: &str) {
    std::fs::write(path, body).expect("a shim is writable");
    let mut mode = std::fs::metadata(path)
        .expect("the shim is there")
        .permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut mode, 0o755);
    std::fs::set_permissions(path, mode).expect("the shim is executable");
}

/// A directory holding shims that record being run and do nothing.
///
/// This is what makes "the installer started nothing" an assertion rather than
/// a hope: an installer that ran `systemctl enable --now` or `launchctl
/// bootstrap` would leave a line in the recording. Where the manager asked for
/// is not this host's own, a `uname` answering that manager's system is put
/// beside them, which is the one fact the installer chooses its branch by.
fn shims(root: &Path, manager: Manager) -> (PathBuf, PathBuf) {
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).expect("a bin directory");
    let recording = root.join("what-was-run");
    for name in must_not_invoke() {
        executable(
            &bin.join(&name),
            &format!(
                "#!/bin/sh\necho \"{name} $*\" >> \"{}\"\nexit 0\n",
                recording.display()
            ),
        );
    }
    if manager != Manager::host() {
        executable(
            &bin.join("uname"),
            &format!(
                "#!/bin/sh\ncase \"${{1:-}}\" in -m) echo x86_64 ;; *) echo {} ;; esac\n",
                system_of(manager)
            ),
        );
    }
    (bin, recording)
}

/// What one run of the installer left behind.
struct Installed {
    /// The root it installed beneath.
    root: PathBuf,
    /// The file the shims record into, which must never come to exist.
    recording: PathBuf,
    /// The service manager it wrote a definition for.
    manager: Manager,
}

impl Installed {
    /// The service definition it wrote.
    fn definition_file(&self) -> PathBuf {
        self.manager.definition(&self.root)
    }

    /// The service definition it wrote, read.
    fn definition(&self) -> Definition {
        Definition::read(self.manager, &self.definition_file())
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

/// What a service definition says, whichever manager's vocabulary it is in.
struct Definition {
    /// The command it starts the service with.
    start: Vec<String>,
    /// The user it runs the service as.
    user: String,
    /// The home it gives the service.
    home: String,
    /// The directory it starts the service in.
    working_directory: String,
    /// The whole of it, as written.
    text: String,
    /// The property list, where it is one.
    list: Option<plist::Dictionary>,
}

impl Definition {
    /// Read one manager's definition as that manager reads it.
    fn read(manager: Manager, path: &Path) -> Self {
        let text = std::fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("{} does not read: {error}", path.display()));
        match manager {
            Manager::Systemd => Self {
                start: unit_value(&text, "ExecStart")
                    .split_whitespace()
                    .map(str::to_owned)
                    .collect(),
                user: unit_value(&text, "User"),
                home: unit_value(&text, "Environment")
                    .strip_prefix("HOME=")
                    .expect("the unit gives the service a home")
                    .to_owned(),
                working_directory: unit_value(&text, "WorkingDirectory"),
                text,
                list: None,
            },
            Manager::Launchd => {
                let list = plist::Value::from_file(path)
                    .unwrap_or_else(|error| {
                        panic!("{} is not a property list: {error}", path.display())
                    })
                    .into_dictionary()
                    .expect("the property list is a dictionary");
                let string = |value: Option<&plist::Value>, what: &str| {
                    value
                        .and_then(plist::Value::as_string)
                        .unwrap_or_else(|| panic!("the property list declares no {what}"))
                        .to_owned()
                };
                Self {
                    start: list
                        .get("ProgramArguments")
                        .and_then(plist::Value::as_array)
                        .expect("the property list declares ProgramArguments")
                        .iter()
                        .map(|argument| string(Some(argument), "argument"))
                        .collect(),
                    user: string(list.get("UserName"), "UserName"),
                    home: string(
                        list.get("EnvironmentVariables")
                            .and_then(plist::Value::as_dictionary)
                            .and_then(|environment| environment.get("HOME")),
                        "HOME",
                    ),
                    working_directory: string(list.get("WorkingDirectory"), "WorkingDirectory"),
                    text,
                    list: Some(list),
                }
            }
        }
    }

    /// Whether it carries one setting `repo-policy.toml` states as a
    /// `{ key, value }`, in its own manager's vocabulary: a `Key=value` line of
    /// a unit, or a key of a property list holding that value.
    fn carries(&self, setting: &toml::Value) -> bool {
        let key = setting["key"].as_str().expect("a setting names its key");
        match &self.list {
            None => {
                let value = setting["value"]
                    .as_str()
                    .expect("a unit's value is a string");
                self.text
                    .lines()
                    .any(|line| line.trim() == format!("{key}={value}"))
            }
            Some(list) => list
                .get(key)
                .is_some_and(|found| *found == as_plist(&setting["value"])),
        }
    }
}

/// One `repo-policy.toml` value as the property list value it states.
fn as_plist(value: &toml::Value) -> plist::Value {
    match value {
        toml::Value::Boolean(flag) => plist::Value::Boolean(*flag),
        toml::Value::Integer(number) => plist::Value::Integer((*number).into()),
        toml::Value::String(text) => plist::Value::String(text.clone()),
        toml::Value::Table(table) => plist::Value::Dictionary(
            table
                .iter()
                .map(|(key, value)| (key.clone(), as_plist(value)))
                .collect(),
        ),
        other => panic!("the policy states a setting no property list carries: {other}"),
    }
}

/// Run the committed installer against a root this journey owns.
fn install(under: &Path) -> Installed {
    let (installed, run) = installing(under, &["--binary", env!("CARGO_BIN_EXE_printobserver")]);
    assert!(
        run.status.success(),
        "the installer failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    installed
}

/// Run the committed installer with the arguments given, and answer what it did.
fn installing(under: &Path, arguments: &[&str]) -> (Installed, std::process::Output) {
    installing_for(under, Manager::host(), arguments)
}

/// Run the committed installer as a host of one service manager runs it.
fn installing_for(
    under: &Path,
    manager: Manager,
    arguments: &[&str],
) -> (Installed, std::process::Output) {
    let (bin, recording) = shims(under, manager);
    let root = under.join("target-root");
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let run = Command::new(repo_root().join(INSTALLER))
        .arg("--root")
        .arg(&root)
        .args(arguments)
        .env("PATH", path)
        .output()
        .expect("the installer runs");
    (
        Installed {
            root,
            recording,
            manager,
        },
        run,
    )
}

/// A directory holding a `printobserver` on PATH, which is what any of the
/// install path's three routes leaves behind.
fn program_on_path(under: &Path) -> PathBuf {
    let bin = under.join("bin");
    std::fs::create_dir_all(&bin).expect("a bin directory");
    let placed = bin.join("printobserver");
    std::fs::copy(env!("CARGO_BIN_EXE_printobserver"), &placed).expect("the program is copyable");
    let mut mode = std::fs::metadata(&placed)
        .expect("the program is there")
        .permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut mode, 0o755);
    std::fs::set_permissions(&placed, mode).expect("the program is executable");
    bin
}

/// One `Key=Value` of an installed unit.
fn unit_value(unit: &str, key: &str) -> String {
    unit.lines()
        .find_map(|line| line.strip_prefix(&format!("{key}=")))
        .unwrap_or_else(|| panic!("the installed unit declares no {key}"))
        .trim()
        .to_owned()
}

/// The user id owning one path.
fn owner(path: &Path) -> u32 {
    std::os::unix::fs::MetadataExt::uid(
        &std::fs::metadata(path)
            .unwrap_or_else(|error| panic!("{} is not there: {error}", path.display())),
    )
}

/// The user id of one user, by name.
fn uid_of(user: &str) -> u32 {
    let listed = Command::new("id")
        .arg("-u")
        .arg(user)
        .output()
        .expect("id runs");
    String::from_utf8_lossy(&listed.stdout)
        .trim()
        .parse()
        .unwrap_or_else(|_| panic!("{user} is no user of this host"))
}

/// The permission bits of one path.
fn mode_of(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::metadata(path)
        .unwrap_or_else(|error| panic!("{} is not there: {error}", path.display()))
        .permissions()
        .mode()
        & 0o777
}

/// The mode of one path, and the long listing that says whether it carries an
/// access-control list granting some principal more than its mode bits do.
///
/// Read off `ls -ld`, which marks a file carrying an access-control list with a
/// `+` straight after its mode, so this needs nothing beyond the coreutils every
/// supported host has — and not the `acl` package, which neither this
/// repository's setup nor a stock host provides.
fn permissions(path: &Path) -> (u32, String) {
    let mode = mode_of(path);
    let listed = Command::new("ls")
        .arg("-ld")
        .arg("--")
        .arg(path)
        .output()
        .expect("ls runs");
    assert!(
        listed.status.success(),
        "{} could not be listed: {}",
        path.display(),
        String::from_utf8_lossy(&listed.stderr)
    );
    let listing = String::from_utf8_lossy(&listed.stdout).trim().to_owned();
    (mode, listing)
}

/// Whether a long listing marks its file as carrying an access-control list.
fn carries_an_access_control_list(listing: &str) -> bool {
    listing
        .split_whitespace()
        .next()
        .is_some_and(|mode| mode.ends_with('+'))
}

/// The installer places four things and starts nothing.
#[test]
fn the_installer_places_four_things_and_starts_nothing() {
    let under = TempDir::new().expect("a journey's own root");
    let installed = install(under.path());

    for (what, path) in [
        ("the program", installed.binary()),
        ("the configuration", installed.configuration()),
        ("the service definition", installed.definition_file()),
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
    assert_eq!(
        owner(&installed.state()),
        uid_of(&installed.definition().user),
        "the state directory is not owned by the user the definition runs the service as"
    );
    let (mode, listing) = permissions(&installed.state());
    assert_eq!(
        mode, 0o700,
        "the state directory is mode {mode:o}, which does not deny group and other"
    );
    assert!(
        !carries_an_access_control_list(&listing),
        "the state directory carries an access-control list granting another \
         principal: {listing}"
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

/// A host for which the installer has no service definition is refused by
/// name, before anything is placed or started.
#[test]
fn an_unsupported_host_is_refused_before_installation() {
    let under = TempDir::new().expect("a journey's own root");
    let bin = under.path().join("bin");
    std::fs::create_dir(&bin).expect("a bin directory");
    executable(&bin.join("uname"), "#!/bin/sh\necho FreeBSD\n");
    let root = under.path().join("target-root");
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let run = Command::new(repo_root().join(INSTALLER))
        .args(["--root", root.to_str().expect("a UTF-8 root")])
        .env("PATH", path)
        .output()
        .expect("the installer runs");

    assert!(!run.status.success(), "the installer accepted FreeBSD");
    let said = String::from_utf8_lossy(&run.stderr);
    assert!(
        said.contains("FreeBSD"),
        "the refusal did not name the host: {said}"
    );
    assert!(!root.exists(), "the refused installer placed files");
}

/// Values that would become launchd property-list markup are refused before
/// the installer writes a definition or invokes a service manager.
#[test]
fn launchd_paths_that_are_property_list_markup_are_refused() {
    let under = TempDir::new().expect("a journey's own root");
    let program = under.path().join("print&observer");
    std::fs::copy(env!("CARGO_BIN_EXE_printobserver"), &program).expect("the program is copied");
    let (installed, run) = installing_for(
        under.path(),
        Manager::Launchd,
        &["--binary", program.to_str().expect("a UTF-8 path")],
    );

    assert!(
        !run.status.success(),
        "the installer accepted property-list markup"
    );
    let said = String::from_utf8_lossy(&run.stderr);
    assert!(
        said.contains("ampersand or an angle bracket"),
        "the refusal did not name the unsafe markup: {said}"
    );
    assert!(
        !installed.root.exists(),
        "the refused installer placed files"
    );
    assert!(
        !installed.recording.exists(),
        "the refused installer invoked a service manager"
    );
}

/// A program discovered on PATH crosses the same property-list boundary as an
/// explicit `--binary` value before the installer writes anything.
#[test]
fn a_launchd_program_on_a_markup_path_is_refused() {
    let under = TempDir::new().expect("a journey's own root");
    let (bin, recording) = shims(under.path(), Manager::Launchd);
    let program_dir = under.path().join("program&path");
    std::fs::create_dir(&program_dir).expect("the program directory is writable");
    let program = program_dir.join("printobserver");
    std::fs::copy(env!("CARGO_BIN_EXE_printobserver"), &program).expect("the program is copied");
    let mut permissions = std::fs::metadata(&program)
        .expect("the program is there")
        .permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
    std::fs::set_permissions(&program, permissions).expect("the program is executable");
    let root = under.path().join("target-root");
    let path = format!(
        "{}:{}:{}",
        bin.display(),
        program_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let run = Command::new(repo_root().join(INSTALLER))
        .args(["--root", root.to_str().expect("a UTF-8 root")])
        .env("PATH", path)
        .output()
        .expect("the installer runs");

    assert!(
        !run.status.success(),
        "the installer accepted XML markup from PATH"
    );
    let said = String::from_utf8_lossy(&run.stderr);
    assert!(
        said.contains("ampersand or an angle bracket"),
        "the refusal did not name the unsafe markup: {said}"
    );
    assert!(!root.exists(), "the refused installer placed files");
    assert!(
        !recording.exists(),
        "the refused installer invoked a service manager"
    );
}

/// A launchd account failure identifies the exact directory-service write,
/// while retaining the service manager's own diagnostic beside it — and the
/// records written before the failing one are removed, so the next run does not
/// find a half-made user and take it for a whole one.
#[test]
fn a_launchd_user_creation_failure_names_the_dscl_operation() {
    let under = TempDir::new().expect("a journey's own root");
    let (bin, _) = shims(under.path(), Manager::Launchd);
    let deleted = under.path().join("dscl-deleted");
    let created = under.path().join("dscl-created");
    executable(
        &bin.join("id"),
        "#!/bin/sh\ncase \"$#:$1\" in 1:-u) echo 0; exit 0 ;; *) exit 1 ;; esac\n",
    );
    // Before anything is created the directory answers no read; after the
    // first write it answers every read, as a directory holding the records
    // would, so the rollback finds them to remove.
    executable(
        &bin.join("dscl"),
        &format!(
            "#!/bin/sh\ncase \"$*\" in *UserShell*) echo 'directory service refused UserShell' \
             >&2; exit 1 ;; *'-read'*) [ -f \"{created}\" ] ;; \
             *'-create'*) touch \"{created}\" ;; *'-delete'*) echo \"dscl $*\" >> \"{deleted}\" ;; \
             *) exit 0 ;; esac\n",
            created = created.display(),
            deleted = deleted.display()
        ),
    );
    let root = under.path().join("target-root");
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let run = Command::new(repo_root().join(INSTALLER))
        .args([
            "--root",
            root.to_str().expect("a UTF-8 root"),
            "--binary",
            env!("CARGO_BIN_EXE_printobserver"),
            "--user",
            "missing-service-user",
        ])
        .env("PATH", path)
        .output()
        .expect("the installer runs");

    assert!(
        !run.status.success(),
        "the installer ignored a failed dscl write"
    );
    let said = String::from_utf8_lossy(&run.stderr);
    assert!(
        said.contains("dscl . -create /Users/missing-service-user UserShell"),
        "the refusal did not name the failing operation: {said}"
    );
    assert!(
        said.contains("directory service refused UserShell"),
        "the refusal hid dscl's own diagnostic: {said}"
    );
    let removed = std::fs::read_to_string(&deleted).unwrap_or_default();
    for record in [
        "/Users/missing-service-user",
        "/Groups/missing-service-user",
    ] {
        assert!(
            removed
                .lines()
                .any(|line| line == format!("dscl . -delete {record}")),
            "the half-made {record} was left standing:\n{removed}"
        );
    }
    assert!(
        said.contains("the partial records for missing-service-user were removed"),
        "the refusal did not say the partial records were removed: {said}"
    );
}

/// A macOS system user is given the first free id at or above 400 over the
/// directory a real Mac lists — one carrying `nobody -2`, `nogroup -1` and
/// `_unknown -99`, which the first hosted round refused as malformed.
#[test]
fn a_launchd_system_user_is_created_at_the_first_free_reserved_id() {
    let under = TempDir::new().expect("a journey's own root");
    let (bin, _) = shims(under.path(), Manager::Launchd);
    let created = under.path().join("dscl-created");
    executable(
        &bin.join("id"),
        "#!/bin/sh\ncase \"$#:$1\" in 1:-u) echo 0; exit 0 ;; *) exit 1 ;; esac\n",
    );
    executable(
        &bin.join("dscl"),
        &format!(
            "#!/bin/sh\ncase \"$*\" in\n\
             *'-read'*) exit 1 ;;\n\
             *'-list /Users UniqueID'*) printf '%s\\n' 'nobody -2' '_unknown -99' 'root 0' \
             'daemon 1' '_www 70' 'taken 400' ;;\n\
             *'-list /Groups PrimaryGroupID'*) printf '%s\\n' 'nobody -2' 'nogroup -1' \
             '_unknown -99' 'wheel 0' 'staff 20' 'taken 401' ;;\n\
             *'-create'*) echo \"dscl $*\" >> \"{}\" ;;\n\
             esac\nexit 0\n",
            created.display()
        ),
    );
    // The user the shim creates exists nowhere on this host, so handing the
    // state directory to it is answered by a shim rather than by the kernel.
    executable(&bin.join("chown"), "#!/bin/sh\nexit 0\n");
    let root = under.path().join("target-root");
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let run = Command::new(repo_root().join(INSTALLER))
        .args([
            "--root",
            root.to_str().expect("a UTF-8 root"),
            "--binary",
            env!("CARGO_BIN_EXE_printobserver"),
            "--user",
            "missing-service-user",
        ])
        .env("PATH", path)
        .output()
        .expect("the installer runs");

    assert!(
        run.status.success(),
        "the installer refused a healthy directory listing: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let recorded = std::fs::read_to_string(&created).expect("the shim recorded what was created");
    for expected in [
        "dscl . -create /Groups/missing-service-user PrimaryGroupID 402",
        "dscl . -create /Users/missing-service-user UniqueID 402",
        "dscl . -create /Users/missing-service-user PrimaryGroupID 402",
        "dscl . -create /Users/missing-service-user IsHidden 1",
    ] {
        assert!(
            recorded.lines().any(|line| line == expected),
            "the user was not created as `{expected}`:\n{recorded}"
        );
    }
}

/// Every read used to choose a macOS system-user id fails closed with the
/// operation's own diagnostic, and exhausting the reserved range refuses the
/// installation rather than choosing a login user's id.
#[test]
fn launchd_system_user_id_selection_failures_are_actionable() {
    let occupied = (400..500)
        .map(|number| format!("occupied-{number} {number}"))
        .collect::<Vec<_>>()
        .join("\n");
    let cases = [
        (
            "group-already-there",
            "#!/bin/sh\ncase \"$*\" in *'-read /Groups/missing-service-user'*) exit 0 ;; \
             *) echo \"dscl $*\" >&2; exit 1 ;; esac\n"
                .to_owned(),
            "there is a group missing-service-user but no user missing-service-user",
            None,
        ),
        (
            "users-list",
            "#!/bin/sh\necho 'users directory unavailable' >&2\nexit 1\n".to_owned(),
            "dscl . -list /Users UniqueID",
            Some("users directory unavailable"),
        ),
        (
            "groups-list",
            "#!/bin/sh\ncase \"$*\" in *'/Users '*) exit 0 ;; *) echo 'groups directory unavailable' >&2; exit 1 ;; esac\n".to_owned(),
            "dscl . -list /Groups PrimaryGroupID",
            Some("groups directory unavailable"),
        ),
        (
            "reserved-ids-exhausted",
            format!("#!/bin/sh\ncase \"$*\" in *'-read'*) exit 1 ;; esac\nprintf '%s\\n' '{occupied}'\n"),
            "every macOS system-user id from 400 to 499 is already taken",
            None,
        ),
        (
            "malformed-id",
            "#!/bin/sh\ncase \"$*\" in *'-read'*) exit 1 ;; esac\necho 'damaged not-a-number'\n"
                .to_owned(),
            "directory service returned a malformed user or group id",
            None,
        ),
    ];

    for (name, dscl, expected, own_diagnostic) in cases {
        let under = TempDir::new().expect("a journey's own root");
        let (bin, _) = shims(under.path(), Manager::Launchd);
        executable(
            &bin.join("id"),
            "#!/bin/sh\ncase \"$#:$1\" in 1:-u) echo 0; exit 0 ;; 2:-u) exit 1 ;; *) exit 1 ;; esac\n",
        );
        executable(&bin.join("dscl"), &dscl);
        let root = under.path().join("target-root");
        let path = format!(
            "{}:{}",
            bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let run = Command::new(repo_root().join(INSTALLER))
            .args([
                "--root",
                root.to_str().expect("a UTF-8 root"),
                "--binary",
                env!("CARGO_BIN_EXE_printobserver"),
                "--user",
                "missing-service-user",
            ])
            .env("PATH", path)
            .output()
            .expect("the installer runs");

        assert!(!run.status.success(), "{name}: the installer succeeded");
        let said = String::from_utf8_lossy(&run.stderr);
        assert!(
            said.contains(expected),
            "{name}: the refusal did not name the failed selection: {said}"
        );
        if let Some(diagnostic) = own_diagnostic {
            assert!(
                said.contains(diagnostic),
                "{name}: the refusal hid dscl's own diagnostic: {said}"
            );
        }
        assert!(
            !root.exists(),
            "{name}: the refused installer placed service files"
        );
    }
}

/// Each service manager's branch of the installer writes the definition the
/// platform list, the install path and the policy say it should, and starts
/// nothing.
///
/// Both branches are driven on every host: the one this host is not is reached
/// through a `uname` answering the other system, which is the one fact the
/// installer chooses by. So the property list a Mac is given is read here as a
/// property list — its label, its start command, its user, and the two settings
/// that start the service unattended — and a definition that would not start
/// at boot or would not come back after an abrupt end is refused on this host
/// rather than only on a runner.
#[test]
fn each_managers_definition_says_what_the_install_path_and_the_policy_state() {
    let agents = std::fs::read_to_string(repo_root().join("AGENTS.md")).expect("AGENTS.md reads");
    let whoami = Command::new("id").arg("-un").output().expect("id runs");
    let invoking = String::from_utf8_lossy(&whoami.stdout).trim().to_owned();
    for manager in MANAGERS {
        let under = TempDir::new().expect("a journey's own root");
        let (installed, run) = installing_for(
            under.path(),
            manager,
            &["--binary", env!("CARGO_BIN_EXE_printobserver")],
        );
        let spelled = manager.spelled();
        let said = String::from_utf8_lossy(&run.stderr);
        assert!(
            run.status.success(),
            "the {spelled} installer failed: {said}"
        );

        // The platform list targets the install path at a platform of this
        // manager, and the installer names that manager's own start command.
        assert!(
            agents.lines().any(|line| line.starts_with("- `")
                && line.contains(&format!("service manager `{spelled}`, install path: yes"))),
            "AGENTS.md's supported-platform list targets no `{spelled}` platform"
        );
        let pair = manager.pair();
        assert_eq!(pair.len(), 2, "the `{spelled}` pair is not two commands");
        assert!(
            said.contains(&pair[1]),
            "the {spelled} installer does not name `{}` as what to run next: {said}",
            pair[1]
        );

        // The four things, the definition where the policy says that manager
        // loads definitions from at boot, and nothing started.
        for (what, path) in [
            ("the program", installed.binary()),
            ("the configuration", installed.configuration()),
            ("the service definition", installed.definition_file()),
        ] {
            assert!(
                path.is_file(),
                "{spelled}: {what} is not at {}",
                path.display()
            );
        }
        assert!(installed.state().is_dir(), "{spelled}: no state directory");
        let other = MANAGERS
            .into_iter()
            .find(|one| *one != manager)
            .expect("two managers");
        assert!(
            !other.definition(&installed.root).exists(),
            "the {spelled} installer wrote a {} definition too",
            other.spelled()
        );
        assert!(
            !installed.recording.exists(),
            "the {spelled} installer started or enabled something: {}",
            std::fs::read_to_string(&installed.recording).unwrap_or_default()
        );
        let directory = manager.policy()["unit_directory"]
            .as_str()
            .expect("a directory")
            .to_owned();
        assert_eq!(
            installed.definition_file().parent().expect("a directory"),
            installed.root.join(directory.trim_start_matches('/')),
            "the {spelled} definition is not in the directory that manager loads at boot"
        );
        if manager == Manager::Launchd {
            assert!(
                pair[1].ends_with(&format!(
                    " {directory}/{}",
                    installed
                        .definition_file()
                        .file_name()
                        .expect("a file")
                        .to_string_lossy()
                )),
                "the launchd start command loads something other than what was written: {}",
                pair[1]
            );
        }
        assert_the_definition_says_what_is_stated(&installed, &invoking);
    }
}

/// What one manager's installed definition says, held to what is stated: the
/// installed program on the installed configuration, the invoking user, the state
/// directory and a home under it, the label its start command loads, and the two
/// settings `repo-policy.toml` states for starting it unattended.
fn assert_the_definition_says_what_is_stated(installed: &Installed, invoking: &str) {
    let manager = installed.manager;
    let spelled = manager.spelled();
    let definition = installed.definition();
    assert_eq!(
        definition.start,
        [
            installed.binary().display().to_string(),
            "server".to_owned(),
            "--config".to_owned(),
            installed.configuration().display().to_string(),
        ],
        "the {spelled} definition does not start the installed program on the \
         installed configuration"
    );
    assert_eq!(definition.user, invoking, "{spelled}: the service's user");
    let state = installed.state().display().to_string();
    assert_eq!(
        definition.working_directory, state,
        "{spelled}: working directory"
    );
    assert!(
        definition.home.starts_with(&format!("{state}/")),
        "{spelled}: the home {} is not under the state directory",
        definition.home
    );
    if let Some(list) = &definition.list {
        assert_eq!(
            list.get("Label").and_then(plist::Value::as_string),
            installed
                .definition_file()
                .file_stem()
                .and_then(|stem| stem.to_str()),
            "the property list's label is not the name the start command loads"
        );
    }
    for behaviour in ["at_boot", "restart"] {
        let setting = &manager.policy()[behaviour];
        assert!(
            definition.carries(setting),
            "the {spelled} definition does not carry `{setting}`, the setting \
             repo-policy.toml states for `{behaviour}`:\n{}",
            definition.text
        );
    }
}

/// The installed definition is one the service manager's own verifier accepts.
#[test]
fn the_installed_definition_passes_the_service_managers_own_verifier() {
    let under = TempDir::new().expect("a journey's own root");
    let installed = install(under.path());
    let manager_verifier: &[&str] = match installed.manager {
        Manager::Systemd => &["systemd-analyze", "verify"],
        Manager::Launchd => &["plutil", "-lint"],
    };

    let verified = Command::new(manager_verifier[0])
        .args(&manager_verifier[1..])
        .arg(installed.definition_file())
        .output()
        .expect("the service manager's own verifier runs");
    assert!(
        verified.status.success(),
        "the installed definition was refused by its own verifier:\n{}{}",
        String::from_utf8_lossy(&verified.stdout),
        String::from_utf8_lossy(&verified.stderr)
    );

    // A verifier that reported nothing whatever it was handed would satisfy the
    // assertion above and say nothing, so it is driven over a definition that
    // is deliberately not one.
    let malformed = under.path().join(match installed.manager {
        Manager::Systemd => "malformed.service",
        Manager::Launchd => "malformed.plist",
    });
    std::fs::write(
        &malformed,
        match installed.manager {
            Manager::Systemd => "[Service]\nType=nonsense\n",
            Manager::Launchd => {
                "<?xml version=\"1.0\"?>\n<plist version=\"1.0\"><dict><key>Label\n"
            }
        },
    )
    .expect("a definition is writable");
    let refused = Command::new(manager_verifier[0])
        .args(&manager_verifier[1..])
        .arg(&malformed)
        .output()
        .expect("the verifier runs");
    assert!(
        !refused.status.success(),
        "the verifier accepted a definition that is not one, so it says nothing about \
         the installed one"
    );
}

/// Fill in the installed configuration template exactly as an operator would.
///
/// The two values the template leaves blank, an `OctoPrint` address that
/// answers, and a port the operating system chooses — and nothing else, so what
/// the started service runs under is the file the installer wrote.
fn fill_in(configuration: &Path) {
    let answering = silent_host();
    let filled = std::fs::read_to_string(configuration)
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
    std::fs::write(configuration, filled).expect("the configuration is writable");
}

/// The configuration the running server wrote for the clients beside it.
///
/// It names the address that was actually bound rather than the one that was
/// configured, which is what lets a supervision turn reach a server started on
/// a port the operating system chose — and the credential in force, which is
/// what lets it authenticate without anybody copying a secret. Carrying that,
/// it is private to the service's own user.
fn client_configuration(state: &Path, address: &str, credential: &str) -> PathBuf {
    let path = state.join(CLIENT_CONFIG_FILE);
    let written: toml::Value = toml::from_str(
        &std::fs::read_to_string(&path)
            .expect("the server wrote the configuration its clients read"),
    )
    .expect("the configuration the server wrote is a document");
    assert_eq!(
        written["client"]["server"].as_str(),
        Some(format!("http://{address}").as_str()),
        "the configuration the server wrote does not name the address it bound"
    );
    assert_eq!(
        written["client"]["credential"].as_str(),
        Some(credential),
        "the configuration the server wrote does not carry the credential in force"
    );
    let mode = mode_of(&path);
    assert_eq!(
        mode, 0o600,
        "the configuration carrying the credential is mode {mode:o}"
    );
    path
}

/// A supervisor the installed unit's own start command started, over the state
/// directory the installer created and the configuration an operator filled in.
struct Started {
    /// The journey's own root, removed when this is dropped.
    root: TempDir,
    /// What the installer put in place.
    installed: Installed,
    /// The program the unit's start command started.
    child: Child,
    /// Where it said it is serving.
    address: String,
    /// A print in its store, as an alert would have opened it.
    print_id: String,
}

impl Drop for Started {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Started {
    /// The credential the service generated, read the way root reads it.
    fn credential(&self) -> String {
        std::fs::read_to_string(self.installed.state().join(API_CREDENTIAL_FILE))
            .expect("the service generated its credential into its state directory")
    }
}

/// Install, fill in the configuration, and run the unit's own start command.
fn started_by_the_unit() -> Started {
    let root = TempDir::new().expect("a journey's own root");
    let installed = install(root.path());

    // Read out of the definition rather than written here, so a definition whose
    // start command is wrong fails this journey rather than being masked.
    let start = installed.definition().start;
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
    fill_in(&configuration);

    // A print in the state directory the installer created, so the context read
    // below has something to read. This is what an alert would have opened.
    let print_id = a_print_in(&installed.state());

    let mut child = Command::new(&start[0])
        .args(&start[1..])
        .stderr(Stdio::piped())
        .spawn()
        .expect("the unit's own start command runs");
    let address = serving_on(&mut child);
    Started {
        root,
        installed,
        child,
        address,
        print_id,
    }
}

/// The unit's own start command starts a server that answers the API.
#[test]
fn the_units_own_start_command_starts_a_server_that_answers_the_api() {
    let mut started = started_by_the_unit();
    let (installed, address, print_id) = (
        &started.installed,
        started.address.clone(),
        started.print_id.clone(),
    );

    // The template names no credential, so the service generated one before it
    // listened, private to its own user.
    let credential = started.credential();
    let mode = mode_of(&installed.state().join(API_CREDENTIAL_FILE));
    assert_eq!(mode, 0o600, "the generated credential is mode {mode:o}");

    let refused = ask(
        &address,
        &format!("/v1/prints/{}/status", absent_print()),
        None,
    );
    assert!(
        refused.contains("HTTP/1.1 401")
            && refused
                .to_ascii_lowercase()
                .contains("www-authenticate: bearer"),
        "the server the unit's own command started answered a caller that presented no \
         credential:\n{refused}"
    );

    let answer = ask(
        &address,
        &format!("/v1/prints/{}/status", absent_print()),
        Some(&credential),
    );

    assert!(
        answer.contains("HTTP/1.1 404"),
        "the server the unit's own command started did not answer the API:\n{answer}"
    );
    assert!(
        answer.contains("application/json"),
        "the server the unit's own command started answered something that is not \
         JSON:\n{answer}"
    );

    // The command a supervision turn runs to read its print's context is this
    // program's own, and it reads from the server this program started. It is
    // given nothing but the configuration file: the server wrote the address it
    // bound and the credential in force into its own state directory, which is
    // where every client beside it reads both from.
    let client_config = client_configuration(&installed.state(), &address, &credential);
    let read = Command::new(env!("CARGO_BIN_EXE_printobserver"))
        .args([
            "context",
            "--config",
            &client_config.display().to_string(),
            "--print-id",
            &print_id,
        ])
        .env_remove(SERVER_ENV)
        .env_remove(CREDENTIAL_ENV)
        .output()
        .expect("the context read runs");
    assert!(
        read.status.success(),
        "the context read failed: {}",
        String::from_utf8_lossy(&read.stderr)
    );
    let printed = String::from_utf8_lossy(&read.stdout);
    assert!(
        printed.contains(&print_id),
        "the context read answered a context about another print: {printed}"
    );

    let refused = Command::new(env!("CARGO_BIN_EXE_printobserver"))
        .args([
            "context",
            "--config",
            &client_config.display().to_string(),
            "--print-id",
            absent_print(),
        ])
        .env_remove(SERVER_ENV)
        .env_remove(CREDENTIAL_ENV)
        .output()
        .expect("the context read runs");
    assert!(
        !refused.status.success(),
        "a context read of a print nothing holds succeeded"
    );
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains(absent_print()),
        "the refusal does not name the print: {}",
        String::from_utf8_lossy(&refused.stderr)
    );

    carries_no_credential(installed, &credential);

    // Stopping it is the signal a service manager stops a unit with, and the
    // program answers it by shutting the server down and exiting successfully —
    // which is what makes `Restart=on-failure` mean what the unit says it does.
    let stopped = Command::new("kill")
        .arg("-TERM")
        .arg(started.child.id().to_string())
        .status()
        .expect("the signal is sent");
    assert!(stopped.success(), "the signal was not sent");
    let finished = started.child.wait().expect("the program exits");
    assert!(
        finished.success(),
        "the program did not exit cleanly when it was stopped: {finished:?}"
    );
}

/// Neither file the installer wrote carries the credential in force.
///
/// They are the ones a person reads, copies and pastes into a question, and the
/// credential the service serves under is in neither.
fn carries_no_credential(installed: &Installed, credential: &str) {
    for (what, path) in [
        ("unit", installed.definition_file()),
        ("configuration", installed.configuration()),
    ] {
        assert!(
            !std::fs::read_to_string(&path)
                .expect("the installed file reads")
                .contains(credential),
            "the installed {what} carries the credential in force"
        );
    }
}

/// What one run of this program, watched by the tracer, did and said.
struct Watched {
    /// What it exited with.
    code: Option<i32>,
    /// Everything it printed, on either stream.
    said: String,
    /// Every access it made to a watched path, as the tracer recorded it:
    /// empty when it made none, and naming the path and `EACCES` when it did.
    touched: String,
}

/// The macOS observation of the same accesses, shared with the journeys tier.
#[cfg(target_os = "macos")]
#[path = "support/interposer.rs"]
mod interposer;

/// Run this program as an operator on their own account would.
///
/// Under the tracer, watching every path given: each access to one is recorded,
/// and each is failed with the answer the kernel gives a user who may not read
/// the service's own files — which is what those files are to such a user. The
/// environment this journey itself runs under is taken away first, so what the
/// program is configured with is only what is given here. On macOS the
/// interposer loaded into the program records and fails the same accesses,
/// and only this launcher differs.
fn as_the_operator(
    started: &Started,
    run: &str,
    arguments: &[&str],
    environment: &[(&str, &str)],
) -> Watched {
    let mut watched = vec![
        PathBuf::from(DEFAULT_CONFIG_PATH),
        started.installed.configuration(),
        started.installed.state(),
    ];
    watched.extend(
        std::fs::read_dir(started.installed.state())
            .expect("the state directory lists")
            .flatten()
            .map(|entry| entry.path()),
    );
    let (output, touched) = watching(started, run, arguments, environment, &watched);
    Watched {
        code: output.status.code(),
        said: String::from_utf8_lossy(&output.stdout).into_owned()
            + &String::from_utf8_lossy(&output.stderr),
        touched,
    }
}

/// The program run as the operator under `strace`, and every watched access it
/// recorded.
#[cfg(not(target_os = "macos"))]
fn watching(
    started: &Started,
    run: &str,
    arguments: &[&str],
    environment: &[(&str, &str)],
    watched: &[PathBuf],
) -> (std::process::Output, String) {
    let trace = started.root.path().join(format!("{run}.trace"));
    let mut command = Command::new(TRACER);
    command
        .args([
            "-f",
            "-qq",
            "-e",
            "trace=%file",
            "-e",
            "inject=%file:error=EACCES",
        ])
        .arg("-o")
        .arg(&trace);
    for path in watched {
        command.arg("-P").arg(path);
    }
    command
        .arg("--")
        .arg(env!("CARGO_BIN_EXE_printobserver"))
        .args(arguments)
        .env_remove(SERVER_ENV)
        .env_remove(CREDENTIAL_ENV);
    for (name, value) in environment {
        command.env(name, value);
    }
    let output = command.output().unwrap_or_else(|error| {
        panic!(
            "`{TRACER}` could not run, and what the operator's route touches rests on it: {error}"
        )
    });
    let touched = std::fs::read_to_string(&trace)
        .unwrap_or_else(|error| panic!("`{TRACER}` wrote nothing: {error}"));
    (output, touched)
}

/// The program run as the operator with the interposer loaded, and every
/// watched access it recorded.
#[cfg(target_os = "macos")]
fn watching(
    started: &Started,
    run: &str,
    arguments: &[&str],
    environment: &[(&str, &str)],
    watched: &[PathBuf],
) -> (std::process::Output, String) {
    let scratch = started.root.path().join(format!("{run}.interposed"));
    std::fs::create_dir_all(&scratch).expect("the run's own scratch directory");
    let mut command = Command::new(env!("CARGO_BIN_EXE_printobserver"));
    command
        .args(arguments)
        .env_remove(SERVER_ENV)
        .env_remove(CREDENTIAL_ENV);
    for (name, value) in environment {
        command.env(name, value);
    }
    let observed = interposer::interposed(command, &scratch, watched);
    let touched = observed.touched();
    (observed.output, touched)
}

/// An operator on their own account authenticates by the documented route.
///
/// They can read neither the service's configuration nor its state directory,
/// so they read the credential once as root and supply it with the address:
/// through the two variables, or through a `[client]` table in a file only they
/// can read, named with `--config`. Either reads the print, and neither touches
/// a file of the service's — each of which is, to this run, a file it is not
/// permitted to read.
#[test]
fn an_operator_on_their_own_account_authenticates_by_the_documented_route() {
    let started = started_by_the_unit();
    let credential = started.credential();
    let server = format!("http://{}", started.address);
    let reading = ["context", "--print-id", started.print_id.as_str()];

    let by_the_environment = as_the_operator(
        &started,
        "environment",
        &reading,
        &[(SERVER_ENV, &server), (CREDENTIAL_ENV, &credential)],
    );
    assert_eq!(
        by_the_environment.code,
        Some(0),
        "the operator's read through the two variables failed: {}",
        by_the_environment.said
    );
    assert!(by_the_environment.said.contains(&started.print_id));
    assert!(
        by_the_environment.touched.trim().is_empty(),
        "the operator's read through the two variables touched the service's own files:\n{}",
        by_the_environment.touched
    );

    let own = started.root.path().join("operator");
    std::fs::create_dir_all(&own).expect("the operator's own directory");
    let own_file = own.join("printobserver.toml");
    std::fs::write(
        &own_file,
        format!("[client]\nserver = \"{server}\"\ncredential = \"{credential}\"\n"),
    )
    .expect("the operator's own file is writable");
    std::fs::set_permissions(
        &own_file,
        std::os::unix::fs::PermissionsExt::from_mode(0o600),
    )
    .expect("the operator's own file is made private");
    let own_path = own_file.display().to_string();
    let mut named = reading.to_vec();
    named.extend(["--config", own_path.as_str()]);
    let by_their_own_file = as_the_operator(&started, "own-file", &named, &[]);
    assert_eq!(
        by_their_own_file.code,
        Some(0),
        "the operator's read through their own file failed: {}",
        by_their_own_file.said
    );
    assert!(by_their_own_file.said.contains(&started.print_id));
    assert!(
        by_their_own_file.touched.trim().is_empty(),
        "the operator's read through their own file touched the service's own files:\n{}",
        by_their_own_file.touched
    );

    // Given the address alone, the program does look for the default file — and
    // meets it unreadable, and goes on to the environment rather than stopping
    // there. The supervisor then refuses it, and it says where the credential
    // it was not given is read from, printing none.
    let address_alone = as_the_operator(
        &started,
        "address-alone",
        &reading,
        &[(SERVER_ENV, &server)],
    );
    assert!(
        address_alone.touched.contains(DEFAULT_CONFIG_PATH)
            && address_alone.touched.contains("EACCES"),
        "the run never met the default file unreadable, so it proves nothing about one:\n{}",
        address_alone.touched
    );
    assert_eq!(
        address_alone.code,
        Some(i32::from(Exit::Unconfigured.status())),
        "a read given no credential was not refused as unconfigured: {}",
        address_alone.said
    );
    assert!(
        address_alone
            .said
            .contains("`credential` in the `[client]` table")
            && address_alone.said.contains(CREDENTIAL_ENV),
        "a read given no credential does not say where the credential is read from: {}",
        address_alone.said
    );

    let wrong = as_the_operator(
        &started,
        "wrong",
        &reading,
        &[
            (SERVER_ENV, &server),
            (CREDENTIAL_ENV, "not-the-credential-in-force"),
        ],
    );
    assert_eq!(
        wrong.code,
        Some(i32::from(Exit::Unconfigured.status())),
        "a read under a wrong credential was not refused as unconfigured: {}",
        wrong.said
    );
    assert!(
        wrong.said.contains(CREDENTIAL_ENV)
            && !wrong.said.contains(&credential)
            && !wrong.said.contains("not-the-credential-in-force"),
        "a read under a wrong credential said the wrong thing: {}",
        wrong.said
    );
}

/// The installed configuration template documents the credential and carries
/// none, and the service definition carries none either.
#[test]
fn the_installed_files_document_the_credential_and_carry_none() {
    let under = TempDir::new().expect("a journey's own root");
    let installed = install(under.path());
    let configuration =
        std::fs::read_to_string(installed.configuration()).expect("the configuration reads");
    let definition =
        std::fs::read_to_string(installed.definition_file()).expect("the service definition reads");

    let commented: Vec<&str> = configuration
        .lines()
        .filter(|line| line.trim_start().starts_with('#'))
        .collect();
    assert!(
        commented.iter().any(|line| line.contains("[api]"))
            && commented.iter().any(|line| line.contains("api-credential")),
        "the configuration template does not document the API credential"
    );
    let parsed: toml::Value = toml::from_str(&configuration).expect("the template is a document");
    assert!(
        parsed.get("api").is_none(),
        "the configuration template carries an API credential"
    );
    assert!(
        !definition.to_ascii_lowercase().contains("credential"),
        "the installed service definition carries a credential"
    );
}

/// One print in a state directory, as an alert would have opened it.
fn a_print_in(state: &Path) -> String {
    let store = printobserver_store_sqlite::SqliteStore::open(state).expect("the store opens");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("a runtime");
    runtime
        .block_on(store.open_print(Some(4211), Some("benchy.gcode".to_owned())))
        .expect("a print opens")
        .id
        .to_string()
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
fn ask(address: &str, path: &str, credential: Option<&str>) -> String {
    let mut stream = TcpStream::connect(address).expect("the started server accepts a connection");
    let authenticating = credential.map_or_else(String::new, |credential| {
        format!("Authorization: Bearer {credential}\r\n")
    });
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\n{authenticating}Connection: close\r\n\r\n"
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
fn answer_nothing(stream: TcpStream) {
    answer_with(stream, "application/json", b"{}");
}

/// Read one request and answer one body of one type.
fn answer_with(mut stream: TcpStream, content_type: &str, body: &[u8]) {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(read) => request.extend_from_slice(&buffer[..read]),
        }
    }
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
}

/// A host serving one snapshot, where a failure alert says its image is.
fn image_host() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let address = listener.local_addr().expect("the bound address");
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || {
                let mut snapshot = vec![0xff, 0xd8, 0xff];
                snapshot.extend(b"printobserver-service-snapshot".repeat(8));
                answer_with(stream, "image/jpeg", &snapshot);
            });
        }
    });
    address
}

/// The shared secret [`fill_in`] gives the ingress.
const SHARED_SECRET: &str = "a-shared-secret";

/// Post the committed `Obico` failure alert to a running server's ingress, its
/// image pointed at a host this journey serves, and answer the status line.
fn post_a_failure_alert(address: &str) -> String {
    let mut alert: printobserver_types::serde_json::Value =
        printobserver_types::serde_json::from_str(include_str!(
            "../../printobserver-obico/samples/obico/failure-alert.json"
        ))
        .expect("the committed sample is JSON");
    alert["img_url"] =
        printobserver_types::serde_json::json!(format!("http://{}/snapshot.jpg", image_host()));
    let body = alert.to_string();
    let mut stream = TcpStream::connect(address).expect("the ingress accepts a connection");
    write!(
        stream,
        "POST {INGRESS_PATH}?{TOKEN_PARAM}={SHARED_SECRET} HTTP/1.1\r\nHost: {address}\r\n\
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

/// The harness the installed configuration names, as the adapter's table
/// declares it.
fn configured_harness(configuration: &Path) -> &'static HarnessSignIn {
    let document: toml::Value =
        toml::from_str(&std::fs::read_to_string(configuration).expect("the configuration reads"))
            .expect("the configuration is a document");
    let named = document["supervisor"]["harness"]
        .as_str()
        .expect("the configuration names a harness");
    SIGN_INS
        .iter()
        .find(|entry| entry.identity() == named)
        .unwrap_or_else(|| panic!("the installed configuration names `{named}`, outside the table"))
}

/// A server this journey started, stopped and reaped however the journey ends.
///
/// A failed assertion unwinds past the journey's own graceful stop, and a
/// server left running holds its port and its state directory long after the
/// journey that started it is gone.
struct Reaped(Child);

impl Drop for Reaped {
    fn drop(&mut self) {
        if let Ok(None) = self.0.try_wait() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

/// Wait for one file to be there, for as long as a supervision turn is given.
fn eventually(path: &Path, server: &mut Child) -> String {
    let deadline = Instant::now() + Duration::from_secs(120);
    while Instant::now() < deadline {
        if let Ok(text) = std::fs::read_to_string(path) {
            return text;
        }
        if let Ok(Some(exited)) = server.try_wait() {
            panic!("the server exited before a turn ran: {exited:?}");
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    panic!("no supervision turn wrote {} in time", path.display());
}

/// A started server hands every supervision turn the harness directory it
/// created, and the sign-in the sign-in command left there is what the turn
/// reads.
///
/// Driven the way the README orders it on an installed root: the unit's own
/// start command, a stand-in for the harness program on the path the service
/// runs with, `printobserver sign-in` against the installed configuration, and
/// then a real failure alert through the ingress.
#[test]
fn a_started_server_hands_every_turn_the_directory_the_sign_in_wrote() {
    let under = TempDir::new().expect("a journey's own root");
    let installed = install(under.path());
    let definition = installed.definition();
    let start = definition.start;
    let configuration = PathBuf::from(start.last().expect("a configuration"));
    fill_in(&configuration);
    let entry = configured_harness(&configuration);
    let bin = under.path().join("harness-bin");
    let invocations = under.path().join("invocations");
    stand_in(&bin, entry, &invocations, 0, &assessment_answer());
    let path = format!("{}:/usr/bin:/bin", bin.display());
    let home = definition.home;
    let directory = installed
        .state()
        .canonicalize()
        .expect("the state directory resolves")
        .join(HARNESS_DIRECTORY)
        .join(entry.identity());
    assert!(
        !directory.exists(),
        "the harness directory was there before anything created it"
    );

    let mut server = Reaped({
        let _held = forking();
        Command::new(&start[0])
            .args(&start[1..])
            .current_dir(installed.state())
            .env("PATH", &path)
            .env("HOME", &home)
            .env_remove(entry.config_env())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the unit's own start command runs")
    });
    let address = serving_on(&mut server.0);

    let mode = mode_of(&directory);
    assert_eq!(
        mode, 0o700,
        "the server created the directory mode {mode:o}"
    );

    let signed_in = {
        let _held = forking();
        Command::new(installed.binary())
            .args(["sign-in", "--config"])
            .arg(&configuration)
            .env("PATH", &path)
            .env("HOME", &home)
            .env_remove(entry.config_env())
            .stdin(Stdio::null())
            .output()
            .expect("the sign-in runs")
    };
    assert!(
        signed_in.status.success(),
        "the sign-in failed: {}",
        String::from_utf8_lossy(&signed_in.stderr)
    );

    let answered = post_a_failure_alert(&address);
    assert!(
        answered.contains("202"),
        "the ingress did not take the alert: {answered}"
    );
    let turn = eventually(&directory.join(TURN_SEEN), &mut server.0);

    assert_eq!(
        PathBuf::from(recorded(&turn, "directory")),
        directory,
        "the turn was pointed somewhere other than the directory the sign-in wrote"
    );
    assert_eq!(recorded(&turn, "state"), SIGN_IN_STATE);
    assert_eq!(
        std::fs::read_to_string(directory.join(SIGNED_IN)).expect("the sign-in is kept"),
        format!("{SIGN_IN_STATE}\n")
    );

    let stopped = Command::new("kill")
        .arg("-TERM")
        .arg(server.0.id().to_string())
        .status()
        .expect("the signal is sent");
    assert!(stopped.success(), "the signal was not sent");
    server.0.wait().expect("the server exits");
}

/// Every `[Service]` directive of the installed unit the journey below knows
/// what to do about: the ones it applies, and the ones that restrict nothing.
///
/// A directive the unit gains that is not here fails the journey, so a unit
/// cannot grow a restriction this substitute silently does not apply.
#[cfg(target_os = "linux")]
const UNIT_SERVICE_DIRECTIVES: [&str; 12] = [
    "Type",
    "User",
    "Environment",
    "ExecStart",
    "WorkingDirectory",
    "Restart",
    "RestartSec",
    "NoNewPrivileges",
    "PrivateTmp",
    "ProtectHome",
    "ProtectSystem",
    "ReadWritePaths",
];

/// What the operator types at the stand-in harness's prompt in the journey.
#[cfg(target_os = "linux")]
const TYPED_IN_THE_JOURNEY: &str = "the-code-the-operator-typed";

/// The whole journey, as `sh` runs it as root inside a mount namespace of its
/// own.
///
/// It stands in for the service manager and for `sudo -u`, and for nothing
/// else. It gives the journey a private `/etc` so the user the installer
/// creates exists in this journey alone; runs the committed installer; puts the
/// harness program where the service user's path finds it; and then applies
/// the unit's restrictions to itself — every home hidden, every mount read-only,
/// the state directory alone written — and audits them as the service user
/// before it runs the documented sign-in and the unit's own start command under
/// them. Everything it learns it prints as `key=value` lines for the journey to
/// assert, because under those restrictions it may write nowhere else.
#[cfg(target_os = "linux")]
const SERVICE_JOURNEY: &str = r#"
set -u
PATH=/usr/sbin:/usr/bin:/sbin:/bin
refuse() { echo "refused=$1"; exit 1; }
ROOT="$JOURNEY_UNDER/target-root"

# A private /etc, so that the user the installer creates is this journey's own.
mount -t tmpfs tmpfs /mnt || refuse "a tmpfs could not be mounted"
mkdir /mnt/etc
cp -a /etc/. /mnt/etc/ 2>/dev/null
[ -f /mnt/etc/shadow ] || : > /mnt/etc/shadow
[ -f /mnt/etc/gshadow ] || : > /mnt/etc/gshadow
mount --bind /mnt/etc /etc || refuse "a private /etc could not be mounted"

# The install path's own installer, as root, exactly as it is committed.
"$JOURNEY_INSTALLER" --root "$ROOT" --binary "$JOURNEY_BINARY" >&2 ||
    refuse "the installer failed"
UNIT="$ROOT/etc/systemd/system/printobserver.service"
sed -n '/^\[Service\]/,/^\[/p' "$UNIT" | grep '=' | sed 's/^/unit_/'
directive() { sed -n "s/^$1=//p" "$UNIT" | head -n 1; }
SERVICE_USER=$(directive User)
STATE=$(directive ReadWritePaths)
START=$(directive ExecStart)
CONFIG=${START##* }
UNIT_HOME=$(directive Environment | sed -n 's/^HOME=//p')
PASSWD_HOME=$(getent passwd "$SERVICE_USER" | cut -d: -f6)
echo "passwd=$(getent passwd "$SERVICE_USER")"
echo "harness=$(sed -n 's/^harness = "\(.*\)"$/\1/p' "$CONFIG")"

# The harness program, installed where the service user's path finds it.
mkdir -p "$ROOT/usr/local/bin"
install -m 0755 "$JOURNEY_STAND_INS"/* "$ROOT/usr/local/bin/"

sed -i -e 's|^api_key = ""|api_key = "a-provisioned-key"|' \
    -e 's|^shared_secret = ""|shared_secret = "a-shared-secret"|' \
    -e "s|^url = \"http://127.0.0.1:5000\"|url = \"http://$JOURNEY_OCTOPRINT\"|" \
    -e 's|^listen = "127.0.0.1:8420"|listen = "127.0.0.1:0"|' "$CONFIG"
mkdir -p /etc/printobserver
: > /etc/printobserver/config.toml
mount --bind "$CONFIG" /etc/printobserver/config.toml || refuse "the configuration could not be bound"
mkfifo /mnt/server-stderr

# The unit's restrictions. ProtectHome: every home is a directory nobody may
# enter. ProtectSystem=strict with ReadWritePaths, and PrivateTmp made stricter:
# every mount but the kernel's own is read-only, and the state directory alone
# is written.
mount --bind "$STATE" "$STATE" || refuse "the state directory could not be bound"
for hidden in /home /root /run/user; do
    if [ -d "$hidden" ]; then
        mount -t tmpfs -o mode=000 tmpfs "$hidden" || refuse "$hidden could not be hidden"
    fi
done
awk '{print $5}' /proc/self/mountinfo | sort -u | while read -r point; do
    case "$point" in
        /proc | /proc/* | /sys | /sys/* | /dev | /dev/pts | "$STATE") ;;
        *) mount -o remount,bind,ro "$point" 2>/dev/null ;;
    esac
done
as_service() {
    setpriv --reuid="$SERVICE_USER" --regid="$(id -g "$SERVICE_USER")" --init-groups \
        --no-new-privs env -i "PATH=$ROOT/usr/local/bin:/usr/local/bin:/usr/bin:/bin" "$@"
}

# The audit, as the service user under those restrictions.
CANDIDATES=$(
    {
        awk '{print $5}' /proc/self/mountinfo
        printf '%s\n' / /tmp /var/tmp /etc /dev/shm "$JOURNEY_UNDER" "$ROOT" \
            "$ROOT/etc/printobserver" "$ROOT/usr/local/bin" "$ROOT/usr/local/lib/printobserver" \
            "$STATE" "$UNIT_HOME" "$PASSWD_HOME" "$JOURNEY_OPERATOR_HOME"
    } | grep -v -e '^/proc' -e '^/sys' -e '^/dev$' -e '^/dev/pts' | sort -u
)
as_service "HOME=$UNIT_HOME" "CANDIDATES=$CANDIDATES" sh -c '
    echo "audit_user=$(id -un)"
    echo "audit_no_new_privs=$(sed -n "s/^NoNewPrivs:[[:space:]]*//p" /proc/self/status)"
    echo "audit_home=${HOME:-}"
    for hidden in /home /root /run/user; do
        ls -a "$hidden" >/dev/null 2>&1 && echo "audit_readable=$hidden"
        touch "$hidden/.printobserver-probe" 2>/dev/null && echo "audit_writable=$hidden"
    done
    printf "%s\n" "$CANDIDATES" | while read -r candidate; do
        if touch "$candidate/.printobserver-probe" 2>/dev/null; then
            rm -f "$candidate/.printobserver-probe"
            echo "audit_writable=$candidate"
        fi
        case "$candidate" in
            /home/* | /root/* | /run/user/*)
                ls -a "$candidate" >/dev/null 2>&1 && echo "audit_readable=$candidate" ;;
        esac
    done
'
echo "state=$STATE"
echo "state_owner=$(stat -c '%U %a' "$STATE")"
echo "environment_home_owner=$(stat -c '%U %a' "$UNIT_HOME")"
echo "passwd_home_owner=$(stat -c '%U %a' "$PASSWD_HOME" 2>/dev/null)"

# `sudo -u` keeps the operator's working directory, which the service user cannot read.
cd /home || refuse "the operator's working directory could not be entered"
SAID=$(printf '%s\n' "$JOURNEY_TYPED" | as_service "HOME=$PASSWD_HOME" \
    "$ROOT/usr/local/lib/printobserver/printobserver" sign-in 2>&1)
echo "sign_in_status=$?"
printf '%s\n' "$SAID" | sed 's/^/sign_in_said=/'
HARNESS_DIR="$STATE/harness/$(sed -n 's/^harness = "\(.*\)"$/\1/p' "$CONFIG")"
sed 's/^/sign_in_/' "$HARNESS_DIR/sign-in-seen" 2>/dev/null

cd "$(directive WorkingDirectory)" || refuse "the unit's working directory could not be entered"
# Started directly rather than through `as_service`, so that `$!` is the server
# itself rather than a subshell around it, and stopped however this ends.
setpriv --reuid="$SERVICE_USER" --regid="$(id -g "$SERVICE_USER")" --init-groups \
    --no-new-privs env -i "PATH=$ROOT/usr/local/bin:/usr/local/bin:/usr/bin:/bin" \
    "HOME=$UNIT_HOME" $START >/dev/null 2>/mnt/server-stderr &
SERVER=$!
trap 'kill "$SERVER" 2>/dev/null' EXIT
exec 3</mnt/server-stderr
while IFS= read -r line <&3; do
    case "$line" in
        "printobserver is serving on "*)
            echo "serving=${line#printobserver is serving on }"
            break
            ;;
        *"will not start"*) refuse "$line" ;;
    esac
done
cat <&3 >&2 &
waited=0
while [ ! -f "$HARNESS_DIR/turn-seen" ] && [ "$waited" -lt 1200 ]; do
    sleep 0.1
    waited=$((waited + 1))
done
kill "$SERVER"
wait "$SERVER"
sed 's/^/turn_/' "$HARNESS_DIR/turn-seen" 2>/dev/null
echo "harness_dir=$HARNESS_DIR"
echo "harness_dir_owner=$(stat -c '%U %a' "$HARNESS_DIR")"
echo "state_file_owner=$(stat -c '%U %a' "$HARNESS_DIR/signed-in")"
echo "turn_file_owner=$(stat -c '%U %a' "$HARNESS_DIR/turn-seen" 2>/dev/null)"
echo "finished=yes"
"#;

/// How this host lets the journey be root inside a mount namespace of its own.
///
/// A transient unit would apply the unit's restrictions for real, but its
/// `User=` has to name a user in the host's own user database, and this journey
/// does not create a system user on the machine it runs on — a developer's
/// included. So the user the installer creates exists in the journey's private
/// `/etc` alone, and the restrictions are applied by this journey and audited
/// by it. An unprivileged user namespace does that where the host allows one;
/// where it does not, a password-free `sudo` enters a private mount namespace
/// instead. A host with neither cannot run this journey, and it says so.
#[cfg(target_os = "linux")]
fn namespace_launcher() -> Vec<String> {
    let owned = |words: &[&str]| {
        words
            .iter()
            .map(|word| (*word).to_owned())
            .collect::<Vec<_>>()
    };
    let unprivileged = owned(&[
        "unshare",
        "--user",
        "--map-root-user",
        "--map-auto",
        "--mount",
        "--propagation",
        "private",
        "--fork",
    ]);
    let privileged = owned(&[
        "sudo",
        "-n",
        "unshare",
        "--mount",
        "--propagation",
        "private",
        "--fork",
    ]);
    for launcher in [unprivileged, privileged] {
        let _held = forking();
        let probed = Command::new(&launcher[0])
            .args(&launcher[1..])
            .args([
                "sh",
                "-c",
                "mount -t tmpfs tmpfs /mnt && useradd --help >/dev/null",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if probed.is_ok_and(|status| status.success()) {
            return launcher;
        }
    }
    panic!(
        "this host allows neither an unprivileged user namespace that can mount nor a \
         password-free sudo, so this journey cannot apply the installed unit's \
         restrictions to itself"
    );
}

/// Every value one `key=value` report carries under one key.
#[cfg(target_os = "linux")]
fn reported<'a>(report: &'a [(String, String)], key: &str) -> Vec<&'a str> {
    report
        .iter()
        .filter(|(found, _)| found == key)
        .map(|(_, value)| value.as_str())
        .collect()
}

/// The one value one report carries under one key.
#[cfg(target_os = "linux")]
fn reported_once<'a>(report: &'a [(String, String)], key: &str) -> &'a str {
    match reported(report, key).as_slice() {
        [value] => value,
        other => panic!(
            "the journey reported {key} {} times: {other:?}",
            other.len()
        ),
    }
}

/// Whether one path is the state directory or inside it.
#[cfg(target_os = "linux")]
fn inside(path: &str, state: &str) -> bool {
    path == state || path.starts_with(&format!("{state}/"))
}

/// Run [`SERVICE_JOURNEY`] on a root this journey owns, post a real failure
/// alert when the server it started says where it is serving, and answer
/// everything it reported once it has finished and what it installed is gone.
#[cfg(target_os = "linux")]
fn run_the_service_journey() -> Vec<(String, String)> {
    let under = TempDir::new().expect("a journey's own root");
    // The service's user has to be able to reach the root this journey installs
    // beneath, as it can reach `/` on a real machine.
    std::fs::set_permissions(
        under.path(),
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .expect("the journey's root is traversable");
    let stand_ins = under.path().join("stand-ins");
    for entry in &SIGN_INS {
        stand_in(
            &stand_ins,
            entry,
            Path::new("/dev/null"),
            0,
            &assessment_answer(),
        );
    }
    let launcher = namespace_launcher();
    let installer = repo_root()
        .join(INSTALLER)
        .canonicalize()
        .expect("the installer resolves");
    let operator_home = std::env::var("HOME").unwrap_or_else(|_| "/home".to_owned());

    let mut journey = {
        let _held = forking();
        Command::new(&launcher[0])
            .args(&launcher[1..])
            .arg("env")
            .arg(format!("JOURNEY_UNDER={}", under.path().display()))
            .arg(format!("JOURNEY_INSTALLER={}", installer.display()))
            .arg(format!(
                "JOURNEY_BINARY={}",
                env!("CARGO_BIN_EXE_printobserver")
            ))
            .arg(format!("JOURNEY_STAND_INS={}", stand_ins.display()))
            .arg(format!("JOURNEY_OCTOPRINT={}", silent_host()))
            .arg(format!("JOURNEY_TYPED={TYPED_IN_THE_JOURNEY}"))
            .arg(format!("JOURNEY_OPERATOR_HOME={operator_home}"))
            .args(["sh", "-c", SERVICE_JOURNEY])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the journey's namespace starts")
    };
    let stderr = journey.stderr.take().expect("the journey's own error");
    let said = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = BufReader::new(stderr).read_to_string(&mut text);
        text
    });
    let mut report: Vec<(String, String)> = Vec::new();
    for line in BufReader::new(journey.stdout.take().expect("the journey's own output")).lines() {
        let line = line.expect("the journey's output reads");
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key == "serving" {
            let answered = post_a_failure_alert(value);
            assert!(
                answered.contains("202"),
                "the ingress did not take the alert: {answered}"
            );
        }
        report.push((key.to_owned(), value.to_owned()));
    }
    let finished = journey.wait().expect("the journey finishes");
    let said = said.join().unwrap_or_default();
    let cleaned = {
        let _held = forking();
        Command::new(&launcher[0])
            .args(&launcher[1..])
            .args(["rm", "-rf"])
            .arg(under.path().join("target-root"))
            .status()
    };
    assert!(
        finished.success() && reported(&report, "finished") == ["yes"],
        "the journey did not finish: {:?}\n{report:#?}\n{said}",
        reported(&report, "refused")
    );
    assert!(
        cleaned.is_ok_and(|status| status.success()),
        "what the journey installed could not be removed"
    );
    report
}

/// What the journey reported about the installed unit and the user it runs
/// as, held to what the installer writes; answers the state directory and that
/// user.
#[cfg(target_os = "linux")]
fn assert_the_unit_and_its_user(report: &[(String, String)]) -> (&str, &str) {
    // The unit is the one the installer writes, and every restriction it
    // carries is one this journey applied.
    let unit: Vec<(&str, &str)> = report
        .iter()
        .filter_map(|(key, value)| key.strip_prefix("unit_").map(|name| (name, value.as_str())))
        .collect();
    for (name, _) in &unit {
        assert!(
            UNIT_SERVICE_DIRECTIVES.contains(name),
            "the unit carries `{name}`, and this journey does not know what it restricts"
        );
    }
    let directive = |name: &str| {
        unit.iter()
            .find(|(found, _)| *found == name)
            .map_or_else(|| panic!("the unit carries no {name}"), |(_, value)| *value)
    };
    assert_eq!(directive("ProtectHome"), "true");
    assert_eq!(directive("ProtectSystem"), "strict");
    assert_eq!(directive("NoNewPrivileges"), "true");
    assert_eq!(directive("PrivateTmp"), "true");
    let state = reported_once(report, "state");
    assert_eq!(directive("ReadWritePaths"), state);

    // The user is the one the installer creates, and no home it is given is
    // under /home.
    let service_user = directive("User");
    assert_eq!(service_user, "printobserver");
    let passwd: Vec<&str> = reported_once(report, "passwd").split(':').collect();
    assert_eq!(passwd[0], service_user);
    let passwd_home = passwd[5];
    let unit_home = directive("Environment")
        .strip_prefix("HOME=")
        .expect("the unit names the service's home");
    for (what, home, owner) in [
        (
            "the user's own",
            passwd_home,
            reported_once(report, "passwd_home_owner"),
        ),
        (
            "the unit's",
            unit_home,
            reported_once(report, "environment_home_owner"),
        ),
    ] {
        assert!(
            !home.starts_with("/home") && home != state && inside(home, state),
            "{what} home {home} is not a directory under the state directory {state}"
        );
        assert_eq!(
            owner,
            format!("{service_user} 700"),
            "{what} home is not the service user's alone"
        );
    }
    (state, service_user)
}

/// What the journey reported about auditing its own restrictions as the
/// service user: every one relevant to where a sign-in may be kept was applied.
#[cfg(target_os = "linux")]
fn assert_the_audit(report: &[(String, String)], state: &str, service_user: &str) {
    assert_eq!(reported_once(report, "audit_user"), service_user);
    assert_eq!(reported_once(report, "audit_no_new_privs"), "1");
    assert_eq!(
        reported(report, "audit_readable"),
        Vec::<&str>::new(),
        "a home is readable to the service user"
    );
    let writable = reported(report, "audit_writable");
    assert!(
        writable.contains(&state),
        "the state directory is not writable to the service user"
    );
    assert!(
        writable.iter().all(|path| inside(path, state)),
        "the service user can write outside the state directory: {writable:?}"
    );
}

/// What the journey reported about the documented sign-in and the supervision
/// turn the unit's own start command ran after it.
#[cfg(target_os = "linux")]
fn assert_the_sign_in_and_the_turn(report: &[(String, String)], state: &str, service_user: &str) {
    let entry = SIGN_INS
        .iter()
        .find(|entry| entry.identity() == reported_once(report, "harness"))
        .expect("the installed configuration names a harness in the table");
    let harness_dir = reported_once(report, "harness_dir");
    assert_eq!(
        harness_dir,
        format!("{state}/{HARNESS_DIRECTORY}/{}", entry.identity())
    );
    assert_eq!(reported_once(report, "sign_in_status"), "0", "{report:#?}");
    assert!(
        reported(report, "sign_in_said")
            .iter()
            .any(|line| line.ends_with(&format!("{ANSWERED}{TYPED_IN_THE_JOURNEY}"))),
        "the sign-in was not answered on its terminal: {report:#?}"
    );
    assert_eq!(reported_once(report, "sign_in_user"), service_user);
    assert_eq!(reported_once(report, "sign_in_directory"), harness_dir);
    assert_eq!(reported_once(report, "sign_in_cwd"), harness_dir);
    assert!(inside(reported_once(report, "sign_in_home"), state));
    // What the sign-in recorded was read out of the harness directory, beside
    // the state file it wrote.
    assert_eq!(
        reported_once(report, "sign_in_argv"),
        entry.arguments().join(" "),
        "{SIGN_IN_SEEN} beside the state file does not record the harness's own sign-in"
    );

    // The supervision turn the unit's own start command ran.
    assert_eq!(reported_once(report, "turn_user"), service_user);
    assert_eq!(reported_once(report, "turn_directory"), harness_dir);
    assert_eq!(reported_once(report, "turn_state"), SIGN_IN_STATE);
    assert!(inside(reported_once(report, "turn_home"), state));
    // The state file the sign-in left and the file the turn wrote beside it are
    // both in the harness directory, and all three are the service user's alone.
    for (what, key, mode) in [
        ("the harness directory", "harness_dir_owner", "700"),
        (SIGNED_IN, "state_file_owner", "600"),
        (TURN_SEEN, "turn_file_owner", "600"),
    ] {
        assert_eq!(
            reported_once(report, key),
            format!("{service_user} {mode}"),
            "{what} in {harness_dir} is not the service user's alone"
        );
    }
}

/// The procedure the README documents works on a fresh install, under the
/// installed unit's own user and its restrictions.
///
/// On a root the committed installer installed into — as root, so that the
/// user it creates is the one the service runs as — the harness program is put
/// where that user's path finds it, `printobserver sign-in` is run as that user
/// the way `sudo -u` runs it, and the unit's own start command then runs a
/// supervision turn from a real failure alert. Both run with every home hidden,
/// every mount read-only and the state directory alone writable, which the
/// journey audits as that user before it relies on it.
#[test]
#[cfg(target_os = "linux")]
fn the_documented_sign_in_works_under_the_units_own_user_and_restrictions() {
    let report = run_the_service_journey();

    let (state, service_user) = assert_the_unit_and_its_user(&report);
    assert_the_audit(&report, state, service_user);
    assert_the_sign_in_and_the_turn(&report, state, service_user);
}

/// The installer takes the program the install path's routes left on PATH.
///
/// Every one of the three routes puts `printobserver` on the caller's path and
/// none of them tells this script where: finding it there is what makes the
/// install path two commands rather than three.
#[test]
fn the_installer_takes_the_program_the_routes_left_on_path() {
    let under = TempDir::new().expect("a journey's own root");
    // `program_on_path` writes into the same `bin` the shims go in, so the
    // installer meets both on one PATH: the program it should take, and the
    // `systemctl` it must not run.
    program_on_path(under.path());
    let (installed, run) = installing(under.path(), &[]);

    assert!(
        run.status.success(),
        "the installer did not find the program on PATH: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(installed.binary().is_file());
    assert!(!installed.recording.exists(), "something was started");
}

/// An installer that can find no program says so, and puts nothing in place.
#[test]
fn an_installer_that_can_find_no_program_says_so() {
    let under = TempDir::new().expect("a journey's own root");
    // No `program_on_path`, and this repository's own program is under `target`
    // rather than on anybody's PATH, so what the installer meets is a machine
    // none of the three routes has been taken on.
    let (installed, run) = installing(under.path(), &[]);

    assert!(
        !run.status.success(),
        "the installer installed a program it never found"
    );
    let said = String::from_utf8_lossy(&run.stderr);
    assert!(
        said.contains("--binary") && said.contains("PATH"),
        "the refusal says nothing a caller can act on: {said}"
    );
    assert!(
        !installed.definition_file().exists(),
        "a unit was written for a program that was never found"
    );
}

/// A reinstall leaves the operator's own configuration exactly as it is.
///
/// The values an operator fills in are the `OctoPrint` key and the ingress
/// secret, and a reinstall that wrote the template back over them would be an
/// upgrade that silently unconfigured the service.
#[test]
fn a_reinstall_leaves_the_operators_own_configuration_alone() {
    let under = TempDir::new().expect("a journey's own root");
    let installed = install(under.path());
    let filled = std::fs::read_to_string(installed.configuration())
        .expect("the configuration reads")
        .replace("api_key = \"\"", "api_key = \"the-operators-own-key\"");
    std::fs::write(installed.configuration(), &filled).expect("the configuration is writable");

    let again = install(under.path());

    assert_eq!(
        std::fs::read_to_string(again.configuration()).expect("the configuration reads"),
        filled,
        "a reinstall wrote the template back over the operator's own values"
    );
}

/// A user name the service manager could not run as is refused.
#[test]
fn a_user_name_the_service_manager_could_not_run_as_is_refused() {
    let under = TempDir::new().expect("a journey's own root");
    for offered in ["bad name", "Printobserver", "-leading-hyphen"] {
        let (_, run) = installing(
            under.path(),
            &[
                "--binary",
                env!("CARGO_BIN_EXE_printobserver"),
                "--user",
                offered,
            ],
        );
        assert!(
            !run.status.success(),
            "`{offered}` was taken as a user the service runs as"
        );
        assert!(
            String::from_utf8_lossy(&run.stderr).contains("is not a system user name"),
            "`{offered}` was refused without saying why"
        );
    }
}

/// A value a unit file has no escape for is refused where it is given.
#[test]
fn a_value_a_unit_file_has_no_escape_for_is_refused() {
    let run = Command::new(repo_root().join(INSTALLER))
        .arg("--root")
        .arg("/tmp/a\"quoted\"root")
        .output()
        .expect("the installer runs");

    assert!(!run.status.success(), "a root carrying a quote was taken");
    let said = String::from_utf8_lossy(&run.stderr);
    assert!(
        said.contains("no escape for"),
        "the refusal says nothing about why it cannot be written"
    );
    assert!(
        said.contains("Pass a path without quotes or backslashes"),
        "the refusal gives no corrective action: {said}"
    );
}
