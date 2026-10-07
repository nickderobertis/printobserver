//! The operator's credential: issued here, held by the operator, verified by
//! the server.
//!
//! # The server keeps a verifier, and the operator keeps the credential
//!
//! A supervision turn runs as the service's user and can read every file that
//! user can, so the server holds nothing a turn could authenticate as the
//! operator with: it checks the operator's credential against its verifier, the
//! SHA-256 of it. [`issue`] draws the credential — 32 bytes of the operating
//! system's secure random source, the same draw every credential this program
//! generates takes — writes it into the operator's own client configuration,
//! readable by them alone, and prints the verifier as the line the operator
//! puts into the server's configuration. It never prints the credential.
//!
//! [`verifier`] is for an operator who already holds one: a credential they
//! chose, or the plaintext an older `api.credential` carried. It reads the
//! credential on standard input — never an argument, which a process table and
//! a shell history would keep — and prints its verifier, never echoing it.
//!
//! Neither reaches a server, and neither reads a configuration file.

use std::fmt::Write as _;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use printobserver_server::{ApiCredential, CredentialVerifier};
use printobserver_types::serde_json::json;

use crate::config::{SCHEME, SERVER_ENV, address_of};
use crate::failure::{Exit, Failure};
use crate::render::{Rendering, render};

/// The address an issued configuration names when nothing says otherwise: the
/// one the installer's configuration has the server listen on.
pub const DEFAULT_SERVER: &str = "http://127.0.0.1:8420";

/// The flag that lets `credential issue` replace an operator configuration
/// already there.
pub const REPLACE_OPTION: &str = "--replace";

/// The mode the operator's configuration, and the directory it is in, are
/// created with: theirs alone.
#[cfg(unix)]
const PRIVATE_FILE: u32 = 0o600;

/// The mode the directory holding the operator's configuration is created with.
#[cfg(unix)]
const PRIVATE_DIRECTORY: u32 = 0o700;

/// What one finished credential command prints and exits with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Printed {
    /// What goes to standard output.
    pub out: String,
    /// What the caller's shell sees.
    pub exit: Exit,
}

/// The line the operator puts into the server's configuration.
fn verifier_line(verifier: &CredentialVerifier) -> String {
    format!("api.credential_verifier = \"{verifier}\"")
}

/// What comes after the line, in the text rendering: where it goes, and that
/// the service has to be restarted to read it. Each line a TOML comment, so
/// the whole of what is printed can be pasted into the file as it is.
fn placing(server_config: &str) -> String {
    format!(
        "# Put the line above in the server's configuration, {server_config}, before its first \
         [table] header (or as `credential_verifier = ...` inside its [api] table), then \
         restart the service so it reads it."
    )
}

/// One credential's verifier, in the rendering the caller asked for.
fn verifier_output(
    verifier: &CredentialVerifier,
    machine_readable: bool,
    written_to: Option<&Path>,
) -> String {
    if machine_readable {
        let mut document = json!({ "credential_verifier": verifier.to_string() });
        if let Some(path) = written_to {
            document["client_config"] = json!(path.display().to_string());
        }
        return render(&document, Rendering::asked_for(true));
    }
    let mut out = format!(
        "{}\n{}\n",
        verifier_line(verifier),
        placing(crate::locations::HERE.config)
    );
    if let Some(path) = written_to {
        let _ = writeln!(
            out,
            "# Your credential is in {}, readable by you alone; every command you run reads it \
             from there.",
            path.display()
        );
    }
    out
}

/// Text as the body of a TOML basic string.
///
/// A credential and an address are printable ASCII by construction, so the
/// quote and the backslash are the only two characters that need an escape.
fn toml_basic_string(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Issue the operator a credential.
///
/// Writes `[client]` — `server` and the drawn `credential` — into the
/// operator's own client configuration, or the file `--config` named, and
/// answers the verifier line to print.
///
/// # Errors
///
/// Returns a failure of [`Exit::Unconfigured`] when there is no configuration
/// home to write into, when `PRINTOBSERVER_SERVER` names no address, when a
/// configuration is already there and `--replace` was not given, and when the
/// file cannot be written; and of [`Exit::Refused`] when the random source
/// refuses.
pub fn issue(
    named: Option<&Path>,
    replace: bool,
    machine_readable: bool,
) -> Result<Printed, Failure> {
    let path = match named {
        Some(path) => path.to_path_buf(),
        None => crate::locations::operator_client_config().ok_or_else(|| {
            Failure::of(
                Exit::Unconfigured,
                "there is no configuration home to keep your credential in: set the variable \
                 your platform keeps it under (`HOME` on Linux and macOS, `APPDATA` on \
                 Windows), or name the file to write with `--config <path>`",
            )
        })?,
    };
    let server = std::env::var(SERVER_ENV)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_SERVER.to_owned());
    address_of(&server, SERVER_ENV).map_err(|bad| Failure::of(Exit::Unconfigured, bad))?;
    let server = if server.starts_with(SCHEME) {
        server
    } else {
        format!("{SCHEME}{server}")
    };
    if !replace && path.exists() {
        return Err(Failure::of(
            Exit::Unconfigured,
            format!(
                "{} is already there, and it may carry the credential the server knows you by. \
                 Give `{REPLACE_OPTION}` to issue a new one in its place — after which the old \
                 one stops working once the server is given the new verifier",
                path.display()
            ),
        ));
    }
    let credential = ApiCredential::generate().map_err(|error| {
        Failure::of(
            Exit::Refused,
            format!(
                "no credential could be drawn, because the operating system's random source \
                 refused: {error}"
            ),
        )
    })?;
    write_private(
        &path,
        &format!(
            "[client]\nserver = \"{}\"\ncredential = \"{}\"\n",
            toml_basic_string(&server),
            toml_basic_string(credential.written())
        ),
    )
    .map_err(|error| {
        Failure::of(
            Exit::Unconfigured,
            format!("{} could not be written: {error}", path.display()),
        )
    })?;
    Ok(Printed {
        out: verifier_output(&credential.verifier(), machine_readable, Some(&path)),
        exit: Exit::Success,
    })
}

/// Write the operator's configuration: into a file of its own beside the
/// target, private from the moment it exists, and then moved into place — so a
/// configuration being replaced is never seen half-written, and never seen
/// readable by anybody else.
fn write_private(path: &Path, contents: &str) -> std::io::Result<()> {
    use std::io::Write as _;

    if let Some(directory) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, PRIVATE_DIRECTORY);
        builder.create(directory)?;
    }
    let staged = staging(path);
    let _ = std::fs::remove_file(&staged);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, PRIVATE_FILE);
    let mut file = options.open(&staged)?;
    let written = file
        .write_all(contents.as_bytes())
        .and_then(|()| file.sync_all())
        .and_then(|()| std::fs::rename(&staged, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&staged);
    }
    written
}

/// The file a configuration is staged in before it is moved into place.
fn staging(path: &Path) -> PathBuf {
    let mut name = path.file_name().map(ToOwned::to_owned).unwrap_or_default();
    name.push(".new");
    path.with_file_name(name)
}

/// The verifier of the one credential standard input carries.
///
/// One line terminator after it is set aside, as a shell's `echo` or a
/// person's editor adds one; anything else is the credential.
///
/// # Errors
///
/// Returns a failure of [`Exit::Usage`] when standard input cannot be read or
/// carries nothing a credential can be, in words that never quote it.
pub fn verifier(machine_readable: bool) -> Result<Printed, Failure> {
    let mut held = Vec::new();
    std::io::stdin().read_to_end(&mut held).map_err(|error| {
        Failure::of(
            Exit::Usage,
            format!("standard input could not be read: {error}"),
        )
    })?;
    let text = core::str::from_utf8(&held).map_err(|_| {
        Failure::of(
            Exit::Usage,
            "standard input is not text. Pipe the credential in, on its own",
        )
    })?;
    let text = text
        .strip_suffix("\r\n")
        .or_else(|| text.strip_suffix('\n'))
        .unwrap_or(text);
    let credential = ApiCredential::new(text).map_err(|why| {
        Failure::of(
            Exit::Usage,
            format!("what standard input carries is not a credential: {why}"),
        )
    })?;
    Ok(Printed {
        out: verifier_output(&credential.verifier(), machine_readable, None),
        exit: Exit::Success,
    })
}
