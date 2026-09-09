//! Where the server is and what authenticates to it.
//!
//! Both are configuration, and neither is ever an argument or an option: a
//! command line that could carry an address is one that could be pointed at
//! something that is not the supervisor, and a command line that could carry a
//! credential is one that lands in a shell history and a process table. They
//! are read from a configuration file and from the environment, in that order,
//! with the environment last so that a host may point one invocation somewhere
//! without editing the file every other invocation reads.
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
/// printer needs no option at all.
pub const DEFAULT_CONFIG_PATH: &str = "/etc/printobserver/config.toml";

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
                 address the supervisor answers on",
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
                "this program has no supervisor to talk to: nothing named one. Write \
                 `[client]` with `server = \"{SCHEME}127.0.0.1:8420\"` into {}, or set \
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
fn address_of(offered: &str, from: &str) -> Result<SocketAddr, Unconfigured> {
    let bare = offered.strip_prefix(SCHEME).unwrap_or(offered);
    bare.trim_end_matches('/')
        .parse::<SocketAddr>()
        .map_err(|_| Unconfigured::BadServer {
            offered: offered.to_owned(),
            from: from.to_owned(),
        })
}

/// Read what this program was configured with.
///
/// The file is read when one is named or when the default is there; a default
/// that is not there is a host that configures this program another way rather
/// than a failure. The environment is read afterwards and wins, so one
/// invocation can be pointed elsewhere without editing what every other
/// invocation reads.
///
/// # Errors
///
/// Returns [`Unconfigured`] when a named file cannot be read or parsed, when
/// nothing anywhere names a server, when what names one is not an address, or
/// when what names a credential names an empty one.
pub fn load(named: Option<&Path>) -> Result<ClientConfig, Unconfigured> {
    let path = named.map_or_else(|| PathBuf::from(DEFAULT_CONFIG_PATH), Path::to_path_buf);
    let document = match std::fs::read_to_string(&path) {
        // Read as a document rather than as a value: a file beginning with a
        // table header is a document, and a value parser meets that header as
        // an array and everything after it as content it did not expect.
        Ok(text) => Some(toml::from_str::<toml::Value>(&text).map_err(|error| {
            Unconfigured::Unparsable {
                path: path.clone(),
                detail: without_the_quoted_source(&error),
            }
        })?),
        // A file nobody named and nobody wrote is a host configured another
        // way. A file somebody named and nothing wrote is a mistake, and is
        // refused where it was named.
        Err(error) if named.is_none() && error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(Unconfigured::Unreadable {
                path,
                detail: error.to_string(),
            });
        }
    };

    let mut server: Option<(String, String)> = None;
    let mut credential: Option<(String, String)> = None;
    if let Some(document) = document.as_ref() {
        let named_by = format!("{}", path.display());
        if let Some(value) = in_file(document, "client", "server") {
            server = Some((value, named_by.clone()));
        } else if let Some(value) = document.get("listen").and_then(toml::Value::as_str) {
            // The server's own file. It says where the supervisor was told to
            // listen, which is where the supervisor is.
            server = Some((value.trim().to_owned(), named_by.clone()));
        }
        if let Some(value) = in_file(document, "client", "credential") {
            credential = Some((value, named_by));
        }
    }
    if let Some(value) = in_environment(SERVER_ENV) {
        server = Some((value, SERVER_ENV.to_owned()));
    }
    if let Some(value) = in_environment(CREDENTIAL_ENV) {
        credential = Some((value, CREDENTIAL_ENV.to_owned()));
    }

    let Some((offered, from)) = server else {
        return Err(Unconfigured::NoServer { path });
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
    use super::{Credential, REDACTED};

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
}
