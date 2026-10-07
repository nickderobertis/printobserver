//! Where the server is and what authenticates to it.
//!
//! Both are configuration, and neither is ever an argument or an option: a
//! command line that could carry an address is one that could be pointed at
//! something that is not the supervisor, and a command line that could carry a
//! credential is one that lands in a shell history and a process table. They
//! are read from configuration files and from the environment, with the
//! environment last so that a host may point one invocation somewhere without
//! editing the file every other invocation reads.
//!
//! # Which files, in which order
//!
//! `--config <path>` names the one file read. With none, the operator's own
//! client configuration is read first — the file `printobserver credential
//! issue` writes, under their configuration home
//! ([`crate::locations::operator_client_config`]) — and the server's default
//! file after it for whatever the first left unnamed. `PRINTOBSERVER_SERVER`
//! and `PRINTOBSERVER_CREDENTIAL` win over both, and with both set and no
//! `--config`, no file is read at all: that is how a supervision turn, whose
//! environment carries both, reads nothing.
//!
//! # The file may be the server's own
//!
//! A `[client]` table names the address and the credential outright. A file
//! that has none is read for the address the **server** was told to listen on,
//! which is the same value written down for the other half of the same host —
//! so pointing this program at `/etc/printobserver/config.toml` configures it
//! with nothing added to that file.
//!
//! # A credential is rendered by nothing
//!
//! [`Credential`] has no accessor that answers its text and no rendering that
//! shows it: [`Display`](core::fmt::Display) and [`Debug`](core::fmt::Debug)
//! both answer [`REDACTED`], so it cannot reach an error's own text, a panic
//! message or a log line by being formatted. What sends it is
//! [`Credential::header_value`], which is used once, at the one place a
//! request is written.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

/// Where this program looks for its configuration when nothing names one.
///
/// The file the service installer writes, so a caller on the host beside the
/// printer needs no option at all — which is this platform's answer in
/// [`crate::locations`].
pub const DEFAULT_CONFIG_PATH: &str = crate::locations::HERE.config;

/// The variable naming the server to talk to.
pub const SERVER_ENV: &str = "PRINTOBSERVER_SERVER";

/// The variable carrying the credential to authenticate with.
pub const CREDENTIAL_ENV: &str = "PRINTOBSERVER_CREDENTIAL";

/// What a credential renders as, wherever a value carrying one is rendered.
pub const REDACTED: &str = "<redacted>";

/// The header a credential travels in.
pub const CREDENTIAL_HEADER: &str = "Authorization";

/// The scheme this program reaches a supervisor over.
pub const SCHEME: &str = "http://";

/// The credential this program authenticates with.
///
/// Its text is readable in exactly one place — [`Self::header_value`] — and
/// nowhere else. Neither rendering shows it, so no message this program prints
/// can carry it by having formatted the configuration it was given.
#[derive(Clone, PartialEq, Eq)]
pub struct Credential(String);

impl Credential {
    /// The credential this text names, refusing one that is nothing.
    ///
    /// # Errors
    ///
    /// Returns [`Unconfigured`] when the text is empty or is nothing but
    /// whitespace: a credential nothing has to carry is no credential, and a
    /// program configured with one is configured with nothing.
    pub fn new(value: &str, from: &str) -> Result<Self, Unconfigured> {
        if value.trim().is_empty() {
            return Err(Unconfigured::Credential {
                from: from.to_owned(),
            });
        }
        Ok(Self(value.to_owned()))
    }

    /// What the header carrying it says.
    #[must_use]
    pub fn header_value(&self) -> String {
        format!("Bearer {}", self.0)
    }
}

impl core::fmt::Display for Credential {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(REDACTED)
    }
}

impl core::fmt::Debug for Credential {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(REDACTED)
    }
}

/// What this program was configured with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientConfig {
    /// Where the supervisor is answering.
    pub server: SocketAddr,
    /// What authenticates to it, when the host configured one.
    pub credential: Option<Credential>,
}

impl ClientConfig {
    /// The address, spelled the way a caller writes it down.
    #[must_use]
    pub fn address(&self) -> String {
        format!("{SCHEME}{}", self.server)
    }
}

/// Why this program has no server to talk to.
///
/// Every variant names what to do next, because the one thing a caller in this
/// position cannot work out for themselves is which file to edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unconfigured {
    /// The file this program was pointed at could not be read.
    Unreadable {
        /// The path that was named.
        path: PathBuf,
        /// What went wrong reading it.
        detail: String,
    },
    /// The file is not a document this program can read.
    Unparsable {
        /// The path that was named.
        path: PathBuf,
        /// What could not be parsed, in the parser's own words.
        detail: String,
    },
    /// Nothing anywhere named a server.
    NoServer {
        /// The file that was read, when one was there to read.
        path: PathBuf,
    },
    /// Something named a server this program cannot reach an address in.
    BadServer {
        /// What it said.
        offered: String,
        /// Where it said it.
        from: String,
    },
    /// Something named a credential that is nothing at all.
    Credential {
        /// Where it said it.
        from: String,
    },
}

impl core::fmt::Display for Unconfigured {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Unreadable { path, detail } => write!(
                formatter,
                "the configuration file {} could not be read: {detail}. Point this program \
                 at a file it can read with `--config <path>`, or set {SERVER_ENV} to the \
                 address the supervisor answers on and {CREDENTIAL_ENV} to the credential it \
                 is configured with",
                path.display()
            ),
            Self::Unparsable { path, detail } => write!(
                formatter,
                "the configuration file {} is not a document this program can read: \
                 {detail}. Correct that file, or set {SERVER_ENV} to the address the \
                 supervisor answers on",
                path.display()
            ),
            Self::NoServer { path } => write!(
                formatter,
                "this program has no supervisor to talk to: nothing named one. Run \
                 `printobserver credential issue` to write your own client configuration, \
                 write `[client]` with `server = \"{SCHEME}127.0.0.1:8420\"` into {}, or set \
                 {SERVER_ENV} to that address",
                path.display()
            ),
            Self::BadServer { offered, from } => write!(
                formatter,
                "`{offered}`, named by {from}, is not an address this program can reach: it \
                 takes `{SCHEME}<address>:<port>`, with an address rather than a name. \
                 Correct it there"
            ),
            Self::Credential { from } => write!(
                formatter,
                "the credential named by {from} is empty, and a credential nothing has to \
                 carry is no credential. Give it a value there, or remove it"
            ),
        }
    }
}

impl core::error::Error for Unconfigured {}

/// One parser refusal, with the source it quoted taken back out.
///
/// A parser reporting a syntax error quotes the line it stopped on, and one of
/// the lines of this file carries the credential. What a caller needs is where
/// the error is and what it was; what nothing needs is the text of the line,
/// which is why it does not reach any output of this program.
fn without_the_quoted_source(error: &toml::de::Error) -> String {
    error
        .to_string()
        .lines()
        .filter(|line| !line.trim_start().starts_with('|') && !line.contains(" | "))
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("; ")
}

/// One value read out of a configuration file.
fn in_file(document: &toml::Value, table: &str, key: &str) -> Option<String> {
    document
        .get(table)?
        .get(key)?
        .as_str()
        .map(str::trim)
        .map(str::to_owned)
}

/// One value read out of the environment, when it is set to anything.
fn in_environment(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
}

/// The address one text names.
///
/// # Errors
///
/// Returns [`Unconfigured::BadServer`] when it names none.
pub(crate) fn address_of(offered: &str, from: &str) -> Result<SocketAddr, Unconfigured> {
    let bare = offered.strip_prefix(SCHEME).unwrap_or(offered);
    bare.trim_end_matches('/')
        .parse::<SocketAddr>()
        .map_err(|_| Unconfigured::BadServer {
            offered: offered.to_owned(),
            from: from.to_owned(),
        })
}

/// What one file named, and where, read as a document.
struct Read {
    /// The address it names, and the file's own name for where it said so.
    server: Option<(String, String)>,
    /// The credential it names, and where.
    credential: Option<(String, String)>,
}

/// Read one file for an address and a credential.
///
/// A `[client]` table's `server` wins over a server's own `listen`, which is
/// where the supervisor was told to listen and so where it is.
///
/// # Errors
///
/// Returns [`Unconfigured::Unparsable`] when the file is not a document.
fn read_document(path: &Path, text: &str) -> Result<Read, Unconfigured> {
    // Read as a document rather than as a value: a file beginning with a
    // table header is a document, and a value parser meets that header as
    // an array and everything after it as content it did not expect.
    let document =
        toml::from_str::<toml::Value>(text).map_err(|error| Unconfigured::Unparsable {
            path: path.to_path_buf(),
            detail: without_the_quoted_source(&error),
        })?;
    let named_by = format!("{}", path.display());
    let server = in_file(&document, "client", "server")
        .or_else(|| {
            document
                .get("listen")
                .and_then(toml::Value::as_str)
                .map(|value| value.trim().to_owned())
        })
        .map(|value| (value, named_by.clone()));
    let credential = in_file(&document, "client", "credential").map(|value| (value, named_by));
    Ok(Read { server, credential })
}

/// How one file this program looked for turned out.
enum Looked {
    /// It was read.
    Found(Read),
    /// Nothing is there.
    Absent,
    /// It is somebody else's, and this caller may not read it.
    PassedOver(String),
}

/// Look for one file nobody named.
///
/// # Errors
///
/// Returns [`Unconfigured`] when it is there and cannot be read for any reason
/// but belonging to somebody else, or is not a document.
fn look_for(path: &Path) -> Result<Looked, Unconfigured> {
    match std::fs::read_to_string(path) {
        Ok(text) => read_document(path, &text).map(Looked::Found),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Looked::Absent),
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            Ok(Looked::PassedOver(error.to_string()))
        }
        Err(error) => Err(Unconfigured::Unreadable {
            path: path.to_path_buf(),
            detail: error.to_string(),
        }),
    }
}

/// What the files nobody named supply between them.
#[derive(Debug, Default)]
struct Gathered {
    /// The server, from the first file naming one.
    server: Option<(String, String)>,
    /// The credential, from the first file naming one.
    credential: Option<(String, String)>,
    /// The first file passed over as somebody else's, and why.
    passed_over: Option<(PathBuf, String)>,
}

/// Read files nobody named in order, each filling only what the ones before it
/// left unnamed, and stopping once nothing is left to fill.
///
/// # Errors
///
/// Returns [`Unconfigured`] for the first file [`look_for`] refuses.
fn gather<'a>(paths: impl IntoIterator<Item = &'a PathBuf>) -> Result<Gathered, Unconfigured> {
    let mut gathered = Gathered::default();
    for path in paths {
        if gathered.server.is_some() && gathered.credential.is_some() {
            break;
        }
        match look_for(path)? {
            Looked::Found(read) => {
                gathered.server = gathered.server.or(read.server);
                gathered.credential = gathered.credential.or(read.credential);
            }
            Looked::Absent => {}
            Looked::PassedOver(detail) => {
                if gathered.passed_over.is_none() {
                    gathered.passed_over = Some((path.clone(), detail));
                }
            }
        }
    }
    Ok(gathered)
}

/// Read what this program was configured with.
///
/// With a file named, that file is read and nothing else is. With none, the
/// operator's own client configuration is read first, and the server's default
/// file after it for whatever the first left unnamed; either not being there
/// is a host that configures this program another way rather than a failure.
/// The environment is read afterwards and wins, so one invocation can be
/// pointed elsewhere without editing what every other invocation reads.
///
/// # The operator on their own account
///
/// The default file is the service's own, and it is private to the service's
/// user. So an operator on their own account keeps their credential in their
/// own configuration, or supplies both values through the environment — and
/// when nothing names a file and the environment names both, no file is opened
/// at all: there is nothing either could add. A default file this program is
/// not permitted to read is otherwise passed over the way a missing one is, and
/// is named only when nothing else named a supervisor.
///
/// # Errors
///
/// Returns [`Unconfigured`] when a file that is there cannot be read or parsed
/// (one the caller may not read, nobody having named it, excepted), when
/// nothing anywhere names a server, when what names one is not an address, or
/// when what names a credential names an empty one.
pub fn load(named: Option<&Path>) -> Result<ClientConfig, Unconfigured> {
    let default = PathBuf::from(DEFAULT_CONFIG_PATH);
    let from_environment = (in_environment(SERVER_ENV), in_environment(CREDENTIAL_ENV));
    let mut server: Option<(String, String)> = None;
    let mut credential: Option<(String, String)> = None;
    let mut passed_over = None;
    // What a caller with nothing configured is told to write into: their own
    // client configuration, where there is a home to keep one in.
    let mut reported =
        crate::locations::operator_client_config().unwrap_or_else(|| default.clone());

    if let Some(path) = named {
        // A file somebody named and nothing wrote is a mistake, and is refused
        // where it was named.
        let text = std::fs::read_to_string(path).map_err(|error| Unconfigured::Unreadable {
            path: path.to_path_buf(),
            detail: error.to_string(),
        })?;
        let read = read_document(path, &text)?;
        server = read.server;
        credential = read.credential;
        reported = path.to_path_buf();
    } else if from_environment.0.is_none() || from_environment.1.is_none() {
        let operator = crate::locations::operator_client_config();
        // llmlint: ignore[changed_behavior_has_e2e] The second file read here is the server's default, a compile-time system path no journey can write without root; `gather`'s own test proves the merge over two real files, and `tests/credentials.rs` drives the operator's file through the binary.
        let gathered = gather(operator.iter().chain(std::iter::once(&default)))?;
        server = gathered.server;
        credential = gathered.credential;
        if let Some((path, detail)) = gathered.passed_over {
            reported = path;
            passed_over = Some(detail);
        }
    }
    if let Some(value) = from_environment.0 {
        server = Some((value, SERVER_ENV.to_owned()));
    }
    if let Some(value) = from_environment.1 {
        credential = Some((value, CREDENTIAL_ENV.to_owned()));
    }

    let Some((offered, from)) = server else {
        return Err(match passed_over {
            Some(detail) => Unconfigured::Unreadable {
                path: reported,
                detail,
            },
            None => Unconfigured::NoServer { path: reported },
        });
    };
    Ok(ClientConfig {
        server: address_of(&offered, &from)?,
        credential: credential
            .map(|(value, from)| Credential::new(&value, &from))
            .transpose()?,
    })
}

#[cfg(test)]
mod tests {
    use super::{Credential, REDACTED, gather};

    /// Neither rendering of a credential shows it.
    ///
    /// This is what stops one reaching an error's own text, a panic message or
    /// a log line by having been formatted: there is no rendering that could
    /// carry it, so no site has to remember not to.
    #[test]
    fn neither_rendering_of_a_credential_shows_it() {
        let credential = Credential::new("qz7vk3xhw9mrbt2ycf5jdlgnps46auei", "a test")
            .expect("a credential this long is one");

        assert_eq!(credential.to_string(), REDACTED);
        assert_eq!(format!("{credential:?}"), REDACTED);
        assert!(
            credential
                .header_value()
                .ends_with("qz7vk3xhw9mrbt2ycf5jdlgnps46auei")
        );
    }

    /// The operator's own file is read first and the server's default fills
    /// only what it left unnamed; a file naming both leaves the default unread.
    #[test]
    fn the_operators_file_wins_and_the_default_fills_what_it_left() {
        let root = tempfile::TempDir::new().expect("a test's own directory");
        let operator = root.path().join("client.toml");
        let default = root.path().join("config.toml");
        std::fs::write(&operator, "[client]\ncredential = \"the-operators\"\n").expect("writable");
        std::fs::write(
            &default,
            "listen = \"127.0.0.1:9\"\n[client]\ncredential = \"the-services\"\n",
        )
        .expect("writable");

        let merged = gather([&operator, &default]).expect("both are documents");
        assert_eq!(
            merged.credential.map(|(value, _)| value).as_deref(),
            Some("the-operators")
        );
        assert!(
            merged
                .server
                .is_some_and(|(value, _)| value.contains("127.0.0.1:9")),
            "the default did not supply the server the operator's file left unnamed"
        );

        std::fs::write(
            &operator,
            "[client]\nserver = \"http://127.0.0.1:7\"\ncredential = \"the-operators\"\n",
        )
        .expect("writable");
        std::fs::write(&default, "this is not a document").expect("writable");
        let whole = gather([&operator, &default]).expect("the default is never opened");
        assert_eq!(
            whole.server.map(|(value, _)| value).as_deref(),
            Some("http://127.0.0.1:7")
        );
    }
}
