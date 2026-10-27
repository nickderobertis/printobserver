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
//! versioned prefix admits a request by the operator's credential or a live
//! supervision turn's alone, and holds it to the identity that credential
//! authenticated. [`turns`] is where each turn's credential is minted and held.
//! [`watch`] drives the printer watch, and holds the printer's push channel
//! and the one command the watch sends. [`server`] is the composition root: the one place an implementation crate is
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
pub mod events;
pub mod ingress;
pub mod observation;
pub mod operations;
pub mod reconcile;
pub mod server;
pub mod turns;
pub mod watch;
pub mod wire;

pub use api::{Admission, ApiState, BODY_BOUND, Caller, DRAIN_BOUND, SPOOL_RECENT_EVENTS, router};
pub use config::{
    ASSETS_DIRECTORY, ApiCredential, ApiSection, ConfigError, ConfigField, ConfigFile,
    CredentialVerifier, DEFAULT_INGRESS_ANSWER_BOUND_MS, DEFAULT_OBSERVATION_INTERVAL_S,
    FAN_VOCABULARY, GENERATED_CREDENTIAL_BYTES, IngressSection, OBICO_POSTING_TIMEOUT_MS,
    OctoprintSection, REDACTED, SKILL_INSTALL, ServerConfig, SharedSecret, SignInConfig,
    SpoolsSection, SupervisorSection, VERIFIER_PREFIX, plainly_written,
};
pub use events::{DEFAULT_EVENTS_LIMIT, MAX_EVENTS_LIMIT, MAX_EVENTS_WAIT_S, Stored};
pub use ingress::{IngressState, QUEUE_DEPTH, TOKEN_HEADER, TOKEN_PARAM};
pub use operations::{
    Answer, BESIDE_THE_ACTIONS, COMMAND_TAG, CONTEXT_IMAGE_PATH_FIELD, Declared, Effect,
    IMAGE_PATH_FIELD, INGRESS_PATH, Located, MEDIA_TYPE, Method, OPERATIONS, Operation, Parameter,
    RECOVERY_STATUS, SpoolCommandKind, VERSION_PREFIX, ValueKind, command_for, operation,
};
/// The harness identities this program can sign in, as the adapter declares
/// them: the command-line program reads the table through this crate, which is
/// the one edge it has to the adapter.
pub use printobserver_oneharness::{HARNESS_DIRECTORY, HarnessSignIn, SIGN_INS};
pub use reconcile::{
    ReconcileStores, Reconciliation, StartupOutcome, StartupReconciliationPayload,
    UnobservedAccounting, overdue, reconcile,
};
pub use server::{
    API_CREDENTIAL_FILE, API_CREDENTIAL_VERIFIER_FILE, CLIENT_CONFIG_FILE, CONTEXT_PROGRAM,
    CREDENTIAL_ISSUE, CREDENTIAL_VERIFIER, OperatorCredential, PROMPT_FILE, Ports, Running,
    SCHEMA_FILE, Server, StartError, TURN_PROMPT, agent_config, context_command,
    operator_credential, plaintext_credential_warning,
};
pub use turns::{GENERATED_TURN_CREDENTIAL_BYTES, TurnBinding, TurnCredentials};
pub use wire::{
    ActionAnswer, ActionBody, BodyRefusal, ContextAnswer, ErrorAnswer, EventsAnswer, HistoryAnswer,
    ImageAnswer, IngressAnswer, LookAnswer, ManifestAnswer, ManifestBody, PreflightAnswer,
    PrintEnded, PrintsAnswer, RecoveryAnswer, RecoveryStatusAnswer, SpoolAnswer,
    SpoolCommandAnswer, SpoolsAnswer, StatusAnswer, TimelineAnswer, at_a_glance, claimed_actor,
    reason_of, recovery_of, spool_command_of,
};
