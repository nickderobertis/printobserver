//! What this adapter needs to reach one `OctoPrint` instance, and the key it
//! authenticates with.
//!
//! The API key is the one secret this crate holds, and the guarantee about it is
//! stated in the types rather than in a convention: [`ApiKey`] renders as
//! [`REDACTED`] in both its `Display` and its `Debug`, and every value that
//! carries one — [`OctoPrintConfig`] here, every variant of
//! `PrinterError` in `printobserver-printer-api` — reaches the key only through
//! [`ApiKey::expose`], which is named so that a reader auditing this crate can
//! find every place the value is legible by searching for one word.

use core::fmt;
use core::time::Duration;

/// How long one request to the instance may take before it is unreachable.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// What an API key renders as, wherever a value carrying one is rendered.
pub const REDACTED: &str = "<redacted>";

/// The scheme this adapter speaks.
///
/// `OctoPrint` is reached over plain HTTP on the machine's own network — the
/// supported deployment is one small board beside the printer — so this adapter
/// carries no TLS stack and refuses any other scheme at configuration time
/// rather than failing at the first request.
pub const SCHEME: &str = "http://";

/// The port a base URL naming no port is reached on.
pub const DEFAULT_PORT: u16 = 80;

/// Why a configuration value names no `OctoPrint` instance this adapter can reach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// The API key was empty, so nothing would authenticate.
    EmptyApiKey,
    /// The base URL is not spelled with the one scheme this adapter speaks.
    UnsupportedScheme {
        /// The base URL as it was given.
        base_url: String,
    },
    /// The base URL names no host.
    NoHost {
        /// The base URL as it was given.
        base_url: String,
    },
    /// The base URL names something that is not a port number.
    BadPort {
        /// The text that was not a port.
        port: String,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyApiKey => formatter.write_str(
                "the OctoPrint API key is empty, and this instance authenticates every request",
            ),
            Self::UnsupportedScheme { base_url } => write!(
                formatter,
                "the OctoPrint base URL {base_url:?} is not spelled with {SCHEME}, \
                 which is the one scheme this adapter speaks"
            ),
            Self::NoHost { base_url } => {
                write!(
                    formatter,
                    "the OctoPrint base URL {base_url:?} names no host"
                )
            }
            Self::BadPort { port } => {
                write!(
                    formatter,
                    "the OctoPrint base URL names {port:?}, which is no port"
                )
            }
        }
    }
}

impl core::error::Error for ConfigError {}

/// The API key an `OctoPrint` instance authenticates a caller by.
///
/// Neither rendering of this type shows the value: `Display` and `Debug` both
/// answer [`REDACTED`], so a key cannot reach a log record, a panic message or
/// an error's own text by being formatted. [`ApiKey::expose`] is the one way to
/// read it, and it is called in exactly one place — where the request header is
/// written.
#[derive(Clone, PartialEq, Eq)]
pub struct ApiKey(String);

impl ApiKey {
    /// The key an instance was provisioned with.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::EmptyApiKey`] when the value is empty or is only
    /// whitespace: an instance with authentication enabled answers every such
    /// request with a refusal, and a key that cannot work is worth refusing
    /// where it is configured rather than where it is used.
    pub fn new(value: impl Into<String>) -> Result<Self, ConfigError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(ConfigError::EmptyApiKey);
        }
        Ok(Self(value))
    }

    /// The key itself, for the one caller that writes it into a request header.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ApiKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(REDACTED)
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "ApiKey({REDACTED})")
    }
}

/// Where one `OctoPrint` instance answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// The host name or address the instance answers on.
    host: String,
    /// The port it answers on.
    port: u16,
    /// The path every request of this adapter is prefixed with, empty or
    /// beginning with `/` and never ending with one.
    prefix: String,
}

impl Endpoint {
    /// The endpoint one base URL names.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] when the URL is not spelled with [`SCHEME`],
    /// names no host, or names something that is not a port.
    pub fn parse(base_url: &str) -> Result<Self, ConfigError> {
        let rest =
            base_url
                .trim()
                .strip_prefix(SCHEME)
                .ok_or_else(|| ConfigError::UnsupportedScheme {
                    base_url: base_url.to_owned(),
                })?;
        let (authority, path) = match rest.find('/') {
            Some(index) => rest.split_at(index),
            None => (rest, ""),
        };
        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port)) => (
                host,
                port.parse().map_err(|_| ConfigError::BadPort {
                    port: port.to_owned(),
                })?,
            ),
            None => (authority, DEFAULT_PORT),
        };
        if host.is_empty() {
            return Err(ConfigError::NoHost {
                base_url: base_url.to_owned(),
            });
        }
        Ok(Self {
            host: host.to_owned(),
            port,
            prefix: path.trim_end_matches('/').to_owned(),
        })
    }

    /// The host the instance answers on.
    #[must_use]
    pub fn host(&self) -> &str {
        &self.host
    }

    /// The port the instance answers on.
    #[must_use]
    pub const fn port(&self) -> u16 {
        self.port
    }

    /// One of this adapter's paths, under whatever prefix the base URL named.
    #[must_use]
    pub fn path(&self, suffix: &str) -> String {
        format!("{}{suffix}", self.prefix)
    }
}

impl fmt::Display for Endpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{SCHEME}{}:{}{}",
            self.host, self.port, self.prefix
        )
    }
}

/// Whether the printer behind an instance has a part-cooling fan this adapter
/// may command.
///
/// `OctoPrint` reports nothing about fans and offers no endpoint for one, so
/// there is nothing to discover: whether the machine has a commandable part fan
/// is a fact about the machine that its operator states here. A printer
/// configured [`FanSupport::Absent`] answers
/// `PrinterError::Unsupported` for the fan and sends nothing,
/// which is the answer the error vocabulary exists to distinguish from a
/// command that was sent and did nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FanSupport {
    /// The machine has one, and the fan command set reaches it.
    #[default]
    Commandable,
    /// The machine has none.
    Absent,
}

impl fmt::Display for FanSupport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Commandable => formatter.write_str("commandable"),
            Self::Absent => formatter.write_str("absent"),
        }
    }
}

/// Everything this adapter needs to reach one `OctoPrint` instance.
///
/// Neither rendering of this type shows the API key: `Debug` is written out
/// below rather than derived, so that the redaction is a property of this file
/// rather than of whichever field happens to be declared here.
#[derive(Clone, PartialEq, Eq)]
pub struct OctoPrintConfig {
    /// Where the instance answers.
    endpoint: Endpoint,
    /// The key it authenticates every request by.
    api_key: ApiKey,
    /// How long one request may take.
    timeout: Duration,
    /// Whether the machine has a part-cooling fan this adapter may command.
    fan: FanSupport,
}

impl OctoPrintConfig {
    /// The configuration for one instance, at its default timeout and with a
    /// commandable fan.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] when the base URL names no reachable endpoint or
    /// the key is empty.
    pub fn new(base_url: &str, api_key: impl Into<String>) -> Result<Self, ConfigError> {
        Ok(Self {
            endpoint: Endpoint::parse(base_url)?,
            api_key: ApiKey::new(api_key)?,
            timeout: DEFAULT_TIMEOUT,
            fan: FanSupport::default(),
        })
    }

    /// The same configuration, with another request timeout.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// The same configuration, with another statement about the fan.
    #[must_use]
    pub fn with_fan(mut self, fan: FanSupport) -> Self {
        self.fan = fan;
        self
    }

    /// Where the instance answers.
    #[must_use]
    pub const fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// The key it authenticates every request by.
    #[must_use]
    pub const fn api_key(&self) -> &ApiKey {
        &self.api_key
    }

    /// How long one request may take.
    #[must_use]
    pub const fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Whether the machine has a part-cooling fan this adapter may command.
    #[must_use]
    pub const fn fan(&self) -> FanSupport {
        self.fan
    }
}

impl fmt::Display for OctoPrintConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "OctoPrint at {} (api key {REDACTED}, timeout {:?}, fan {})",
            self.endpoint, self.timeout, self.fan
        )
    }
}

impl fmt::Debug for OctoPrintConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OctoPrintConfig")
            .field("endpoint", &self.endpoint)
            .field("api_key", &self.api_key)
            .field("timeout", &self.timeout)
            .field("fan", &self.fan)
            .finish()
    }
}
