//! `printobserver`.
//!
//! Owns: the `printobserver` command — the single installable artifact of this
//! repository. Its `server` subcommand runs the supervisor; every other
//! subcommand is one request to an already-running one, and is the whole of
//! the surface both the supervising agent and the operator work through.
//!
//! May depend on: `printobserver-server`, `printobserver-sdk` and
//! `printobserver-types`. Never the `OctoPrint` adapter and never the `Obico`
//! adapter: a client command that could name one could reach a printer or a
//! failure detector without the supervisor in between, which is the one thing
//! this program exists to make impossible.
//!
//! # The surface is derived, not written down
//!
//! [`surface`] builds one command per operation the server declares, and each
//! command's options out of that operation's own declared request schema —
//! which for an action is the fields the contracts' own `PrintAction` declares
//! for that variant. Nothing here restates either. A vocabulary that gains a
//! variant gains a command; a variant that gains a field gains an option; and
//! neither can be grown by growing a list beside the parser, because there is
//! no list beside the parser.
//!
//! # Where a server is and what authenticates to it are configuration
//!
//! No client command takes an address or a credential as an argument or an
//! option. [`config`] reads both from a configuration file and the
//! environment, and a credential reaching this program is rendered by nothing:
//! see [`config::Credential`].
//!
//! # Images are a path, never bytes
//!
//! The server answers an absolute path on **its own** filesystem and no route
//! of it answers image bytes, so this program transports none. A path that
//! names no file here is a failure of its own — the rest of the answer is
//! printed and the path is not, because a caller handed a file name that names
//! nothing is worse off than one told why.

pub mod client;
pub mod config;
pub mod failure;
pub mod parse;
pub mod render;
pub mod surface;
pub mod transport;

pub use client::{Outcome, Produced, perform, present, produce, refusal};
pub use config::{
    CREDENTIAL_ENV, ClientConfig, Credential, DEFAULT_CONFIG_PATH, REDACTED, SERVER_ENV,
    Unconfigured,
};
pub use failure::{Exit, Failure};
pub use parse::{Call, Invocation, parse};
pub use render::{Rendering, fields, render};
pub use surface::{
    CONFIG_OPTION, Command, DURATION_FIELD, Field, Form, GLOBAL_OPTIONS, HELP_OPTION, JSON_OPTION,
    MAX_DURATION_SECONDS, MIN_DURATION_SECONDS, SERVE_COMMAND, Supply, VERSION, VERSION_OPTION,
    command, surface, usage, version,
};
pub use transport::{Answered, Unreachable, send};
