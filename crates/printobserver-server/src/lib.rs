//! `printobserver-server`.
//!
//! Owns: the long-running service: the HTTP surface the clients call, the
//! `Obico` ingress the failure detector posts to, the supervision loop's
//! lifecycle, and the composition root that chooses which implementation backs
//! each port.
//!
//! May depend on: `printobserver-types`, the four port crates,
//! `printobserver-core`, and the implementation crates — it is a composition
//! root, so naming an implementation is its job rather than a violation.
//!
//! # The shape of this crate, in four sentences
//!
//! [`config`] is the one configuration file and every value it is validated
//! against. [`operations`] is the whole public API declared once, and [`api`]
//! builds the router by folding over it, so the set served and the set declared
//! cannot come apart. [`ingress`] is `Obico`'s own endpoint, which answers
//! before its handling completes and inside a bound this repository declares.
//! [`server`] is the composition root: the one place an implementation crate is
//! named, and the only file here that knows there is an `OctoPrint` at all.
//!
//! # Images are a path, never bytes
//!
//! The server, the command-line program and the supervising agent all run on
//! the one host by design, so an image answer is an absolute path on this
//! host's own filesystem. No route this server serves answers image bytes, and
//! no answer renders or encodes them; see [`wire::ImageAnswer`].
//!
//! # Restarting loses nothing
//!
//! The store and the session directory are the whole of the state, and
//! [`reconcile`] is what adopts them on start.

pub mod api;
pub mod config;
pub mod ingress;
pub mod operations;
pub mod reconcile;
pub mod server;
pub mod wire;

pub use api::{ApiState, router};
pub use config::{
    ASSETS_DIRECTORY, ConfigError, ConfigField, ConfigFile, DEFAULT_INGRESS_ANSWER_BOUND_MS,
    FAN_VOCABULARY, IngressSection, OBICO_POSTING_TIMEOUT_MS, OctoprintSection, ServerConfig,
    SupervisorSection, example_envelope,
};
pub use ingress::{IngressState, QUEUE_DEPTH, TOKEN_HEADER, TOKEN_PARAM};
pub use operations::{
    Effect, INGRESS_PATH, MEDIA_TYPE, Method, OPERATIONS, Operation, READS, VERSION_PREFIX,
    operation,
};
pub use reconcile::{Reconciliation, overdue, reconcile};
pub use server::{
    CONTEXT_COMMAND, PROMPT_FILE, Ports, Running, SCHEMA_FILE, SKILL_FILE, Server, StartError,
};
pub use wire::{
    ActionAnswer, ActionBody, BodyRefusal, ContextAnswer, ErrorAnswer, HistoryAnswer, ImageAnswer,
    IngressAnswer, ManifestAnswer, StatusAnswer,
};
