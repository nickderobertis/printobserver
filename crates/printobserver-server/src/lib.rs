//! `printobserver-server`.
//!
//! Owns: the long-running service: the HTTP surface the clients call, the
//! `Obico` ingress the failure detector posts to, the supervision loop's
//! lifecycle, the composition root that chooses which implementation backs
//! each port, and the one event kind a restart writes — the startup
//! reconciliation [`reconcile`] alone records.
//!
//! May depend on: `printobserver-types`, the three port crates,
//! `printobserver-core`, and the implementation crates — it is a composition
//! root, so naming an implementation is its job rather than a violation.
//!
//! # The shape of this crate, in four sentences
//!
//! [`config`] is the one configuration file and every value it is validated
//! against. [`operations`] is the whole public API declared once, and [`api`]
//! builds the router by folding over it, so the set served and the set declared
//! cannot come apart. [`ingress`] is `Obico`'s own endpoint, which answers
//! before its handling completes and inside a bound this repository declares,
//! and admits a post by its shared secret alone; every route beneath the
//! versioned prefix admits a request by the API credential alone.
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

pub use api::{ApiState, BODY_BOUND, router};
pub use config::{
    ASSETS_DIRECTORY, ApiCredential, ApiSection, ConfigError, ConfigField, ConfigFile,
    DEFAULT_INGRESS_ANSWER_BOUND_MS, FAN_VOCABULARY, GENERATED_CREDENTIAL_BYTES, IngressSection,
    OBICO_POSTING_TIMEOUT_MS, OctoprintSection, REDACTED, ServerConfig, SharedSecret, SignInConfig,
    SupervisorSection, plainly_written,
};
pub use ingress::{IngressState, QUEUE_DEPTH, TOKEN_HEADER, TOKEN_PARAM};
pub use operations::{
    Answer, BESIDE_THE_ACTIONS, CONTEXT_IMAGE_PATH_FIELD, Declared, Effect, IMAGE_PATH_FIELD,
    INGRESS_PATH, Located, MEDIA_TYPE, Method, OPERATIONS, Operation, Parameter, VERSION_PREFIX,
    ValueKind, operation,
};
/// The harness identities this program can sign in, as the adapter declares
/// them: the command-line program reads the table through this crate, which is
/// the one edge it has to the adapter.
pub use printobserver_oneharness::{HARNESS_DIRECTORY, HarnessSignIn, SIGN_INS};
pub use reconcile::{
    ReconcileStores, Reconciliation, StartupOutcome, StartupReconciliationPayload, overdue,
    reconcile,
};
pub use server::{
    API_CREDENTIAL_FILE, CLIENT_CONFIG_FILE, CONTEXT_PROGRAM, PROMPT_FILE, Ports, Running,
    SCHEMA_FILE, SKILL_FILE, Server, StartError, TURN_PROMPT, context_command,
};
pub use wire::{
    ActionAnswer, ActionBody, BodyRefusal, ContextAnswer, ErrorAnswer, HistoryAnswer, ImageAnswer,
    IngressAnswer, ManifestAnswer, ManifestBody, PrintsAnswer, StatusAnswer, reason_of,
};
