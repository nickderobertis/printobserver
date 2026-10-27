//! The one configuration file a running supervisor reads, and its validation.
//!
//! # Every field this program takes is declared once, here
//!
//! [`ConfigField`] is that declaration: one variant per configured value, each
//! carrying the dotted key it is spelled under. Two things read it rather than
//! restating it — the refusal a bad value produces names its field through it,
//! and this crate's own tier walks it, driving a server against a configuration
//! in which each field in turn is unacceptable. A field added to the shapes
//! below and not to [`ConfigField::ALL`] fails that tier's own coverage check,
//! so the walk cannot fall behind the type.
//!
//! # Validation happens at startup, and names the field
//!
//! A value that cannot work is refused where it is configured rather than at
//! the first request that needs it, and the refusal carries the field's own key
//! so an operator editing the file knows which line to change. That includes
//! the two values only the machine can rule on: an `OctoPrint` address nothing
//! answers at and a key that instance refuses are both configuration mistakes,
//! and [`crate::Server::start`] refuses them naming
//! [`ConfigField::OctoprintUrl`] and [`ConfigField::OctoprintApiKey`].
//!
//! # The ingress answer bound
//!
//! `Obico`'s webhook notification plugin posts best-effort with a short timeout
//! and does not retry, so this server answers its ingress before the handling
//! completes and inside a bound it declares here. The shipped default is
//! [`DEFAULT_INGRESS_ANSWER_BOUND_MS`] and the timeout it must stay below is
//! [`OBICO_POSTING_TIMEOUT_MS`]; `AGENTS.md`'s "The Obico ingress answer bound"
//! records that number, where it was read and why the default is where it is,
//! and `just check-repo` refuses a tree in which the two disagree.
//!
//! # The operator's credential, by its verifier
//!
//! Every request beneath the versioned prefix carries one credential. The
//! operator's is checked against a [`CredentialVerifier`] — the SHA-256 of the
//! credential, written `sha256:<64 lowercase hex>` — which `api.credential_verifier`
//! names, so the server holds nothing a supervision turn reading its files
//! could authenticate with. The plaintext lives with the operator, in the
//! client configuration `printobserver credential issue` writes.
//!
//! `api.credential`, a plaintext credential, is still read for an installation
//! that wrote one, and admitted until it is replaced; the composition root warns
//! on every start that it is there. Either way a credential is held in a type
//! neither rendering of which shows it, and is compared in constant time.
//!
//! # The periodic observation
//!
//! `supervisor.observation_interval_s` is how often every active print is given
//! a periodic observation and its agent a turn on it, whether or not the
//! detector has alerted. Left out it is
//! [`DEFAULT_OBSERVATION_INTERVAL_S`] — two minutes — and zero is refused,
//! because an interval of nothing is a turn started the moment the last one
//! returns.
//!
//! # The printer watch
//!
//! `supervisor.printer_poll_s` is how often the printer watch reads the machine
//! — from 5 to 300 seconds, fifteen when left out — whether or not a print is
//! active, and `supervisor.unattended_window_s` how long after a print ends
//! `interrupted` or `failed` the printer heating or moving on its own is
//! recorded — from 60 to 86400 seconds, an hour when left out. A value outside
//! either range is refused naming its key.
//!
//! # The camera and `Obico`'s own API
//!
//! Both are optional. `camera.snapshot_url` is where a look fetches a fresh
//! frame; without it a look carries the printer's state and no frame.
//! `obico.url` and `obico.access_token` are given together or not at all: they
//! are how a print the detector paused and the agent adjusted is acknowledged
//! to `Obico`, which re-arms its detection. Each address must be an `http` or
//! `https` URL, and the token is held in a type whose debug form never shows
//! it, so no refusal or log line this program writes can carry it.
//! `obico.ml_api_url` is the failure detector's own service, whose
//! unauthenticated health check is asked during every print; without it the
//! detector's health reads `unchecked`. `vision.blank_frame_max_stddev` is the
//! grey-level spread at or below which a frame is blank — from 1 to 128, 36.5
//! when left out.
//!
//! # Recovery
//!
//! `[recovery]` states the printer's own facts a recovery action is checked
//! against — `away_from_bed` (`z_plus` or `z_minus`), `max_z_mm`,
//! `max_lift_mm` (50 when left out), `clearance_margin_mm` (5 when left out),
//! `park_xy`, `z_home_xy` and `z_home_sweeps_bed`. Left out, every recovery
//! action is refused as not configured; written, every key without a default
//! is required. `[policy.recovery]` says who may ask for which: `operator`,
//! `agent` and `asserting`, by default every action, none, and the operator
//! alone. Granting `recover_home_z` to the agent, or letting the agent's word
//! count as clearance, is refused naming the key, and so is every value
//! outside the range its key admits.
//!
//! # The spool check
//!
//! `[spools]` is optional, and so is every key in it: `preflight` is what a
//! start does when the loaded spool is short, unverified or the file has no
//! analysis — `refuse`, `warn` or `off` — `buffer_percent` and `buffer_min_g`
//! the margin asked beyond what a print needs, and `analysis_wait_seconds` how
//! long a start waits for `OctoPrint`'s analysis of a file that has none yet.
//! Each left out takes the supervision core's own default, and a negative
//! figure or a mode outside the three is refused naming its key.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use printobserver_core::{
    ActionKind, ActorClass, AwayFromBed, DEFAULT_CLEARANCE_MARGIN_MM, DEFAULT_MAX_LIFT_MM,
    PreflightMode, RecoveryActionKind, RecoveryConfig, RecoveryGrants, SafetyEnvelope, SpoolPolicy,
};
use printobserver_obico::{
    AccessToken, BlankFrameBound, MAX_BLANK_FRAME_MAX_STDDEV, MIN_BLANK_FRAME_MAX_STDDEV, ObicoApi,
};
use printobserver_octoprint::{FanSupport, OctoPrintConfig};
use printobserver_oneharness::{HarnessIdentity, ModelName};
use printobserver_types::schemars::JsonSchema;
use printobserver_types::serde::{Deserialize, Serialize};
use printobserver_vision_api::WebAddress;

/// The answer bound this repository ships, in milliseconds.
///
/// A fifth of [`OBICO_POSTING_TIMEOUT_MS`]. The margin is deliberate: the
/// supported host is a small ARM board beside the printer, and a bound set just
/// under the producer's timeout would be exceeded by the first slow moment on
/// it — after which `Obico` drops the alert, because its posting does not retry.
pub const DEFAULT_INGRESS_ANSWER_BOUND_MS: u64 = 1_000;

/// How often every active print is observed, in seconds, when the
/// configuration names no interval: the core's own default.
pub const DEFAULT_OBSERVATION_INTERVAL_S: u64 =
    printobserver_core::DEFAULT_OBSERVATION_INTERVAL.as_secs();

/// How often the printer watch reads the machine, in seconds, when the
/// configuration names no poll: the core's own default.
pub const DEFAULT_PRINTER_POLL_S: u64 = printobserver_core::DEFAULT_PRINTER_POLL.as_secs();

/// The least and the most seconds `supervisor.printer_poll_s` admits.
pub const PRINTER_POLL_RANGE_S: (u64, u64) = (5, 300);

/// How long after an unfinished ending activity is recorded, in seconds, when
/// the configuration names no window: the core's own default.
pub const DEFAULT_UNATTENDED_WINDOW_S: u64 =
    printobserver_core::DEFAULT_UNATTENDED_WINDOW.as_secs();

/// The least and the most seconds `supervisor.unattended_window_s` admits.
pub const UNATTENDED_WINDOW_RANGE_S: (u64, u64) = (60, 86_400);

/// The timeout `Obico`'s own webhook notification plugin posts under.
///
/// Read from the `Obico` release the `obico-fixture` node stands up — revision
/// `49c0bc7001a3fd8d56297fc3032ba287bfe1d50b`,
/// `backend/notifications/plugins/webhook/__init__.py`, whose `execute_webhook`
/// takes `timeout: float = 5.0` and calls `raise_for_status` with no retry.
/// This repository's claim about an external producer; what reconciles it
/// against a live `Obico` is the scheduled `Obico` tier.
pub const OBICO_POSTING_TIMEOUT_MS: u64 = 5_000;

/// The directory under the state directory this server materializes the
/// supervising agent's prompt template and assessment schema into. The skill
/// is not among them: it is installed with [`SKILL_INSTALL`] and configured by
/// `supervisor.skill_path`.
pub const ASSETS_DIRECTORY: &str = "assets";

/// How the supervising agent's skill is installed, and what names it after.
///
/// This program carries no skill of its own: the skill is the Agent Skill this
/// repository publishes, installed as any other is, and every refusal of
/// `supervisor.skill_path` says so, because it is the one instruction an
/// operator whose configuration predates that needs.
pub const SKILL_INSTALL: &str = "the skill is installed with `gh skill install \
     nickderobertis/printobserver printobserver --dir <dir>`, and \
     `supervisor.skill_path` names the installed `SKILL.md`, \
     `<dir>/printobserver/SKILL.md`";

/// Every value this program is configured with, and the key it is spelled under.
///
/// The walk in this crate's tier is over this array, and the array is compared
/// against the configuration shapes' own declared properties, so neither can
/// grow a field the other does not know about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ConfigField {
    /// Where the store, the images and the sessions live.
    StateDir,
    /// The address the API and the ingress are served on.
    Listen,
    /// Where the `OctoPrint` instance answers.
    OctoprintUrl,
    /// The key that instance authenticates every request by.
    OctoprintApiKey,
    /// Whether the machine has a part-cooling fan this server may command.
    OctoprintFan,
    /// The operator's safety envelope.
    SafetyEnvelope,
    /// The harness identity supervision turns run on.
    Harness,
    /// The model turns are pinned to, when one is pinned.
    Model,
    /// The `PrintObserver` skill sent as every turn's system prompt.
    SkillPath,
    /// The prompt template one turn fills.
    PromptTemplatePath,
    /// How often every active print is observed, in seconds.
    ObservationIntervalS,
    /// How often the printer watch reads the machine, in seconds.
    PrinterPollS,
    /// How long after an unfinished ending the printer acting on its own is
    /// recorded, in seconds.
    UnattendedWindowS,
    /// How long the ingress may take to answer, in milliseconds.
    IngressAnswerBoundMs,
    /// The shared secret the ingress requires of every post.
    IngressSharedSecret,
    /// A plaintext operator credential, read for an installation that wrote
    /// one until it is replaced by its verifier.
    ApiCredential,
    /// The verifier the operator's credential is checked against.
    ApiCredentialVerifier,
    /// Where a fresh frame of the print is fetched from, when a camera is
    /// configured.
    CameraSnapshotUrl,
    /// Where `Obico`'s own API answers, when it is configured.
    ObicoUrl,
    /// The bearer token `Obico`'s API is reached with.
    ObicoAccessToken,
    /// Where the failure detector's own service answers its health check.
    ObicoMlApiUrl,
    /// The grey-level spread at or below which a camera frame is blank.
    VisionBlankFrameMaxStddev,
    /// What a start does when the spool check finds a problem.
    SpoolsPreflight,
    /// The margin asked beyond a print's requirement, as a percentage of it.
    SpoolsBufferPercent,
    /// The least margin asked, in grams.
    SpoolsBufferMinG,
    /// How long a start waits for a file's analysis, in seconds.
    SpoolsAnalysisWaitSeconds,
    /// Which way along Z moves the nozzle away from the bed.
    RecoveryAwayFromBed,
    /// The highest Z the machine reaches, in millimetres.
    RecoveryMaxZMm,
    /// The longest lift one recovery may ask for, in millimetres.
    RecoveryMaxLiftMm,
    /// How far above the tallest part the nozzle must be, in millimetres.
    RecoveryClearanceMarginMm,
    /// Where a park moves the head, in X and Y.
    RecoveryParkXy,
    /// Where the machine homes Z, in X and Y.
    RecoveryZHomeXy,
    /// Whether homing Z brings a gantry down over the whole bed.
    RecoveryZHomeSweepsBed,
    /// The recovery actions the operator may ask for.
    PolicyRecoveryOperator,
    /// The recovery actions the agent may ask for.
    PolicyRecoveryAgent,
    /// The actor classes whose clearance assertions count.
    PolicyRecoveryAsserting,
}

impl ConfigField {
    /// Every field this program takes, and there is no other.
    pub const ALL: [Self; 36] = [
        Self::StateDir,
        Self::Listen,
        Self::OctoprintUrl,
        Self::OctoprintApiKey,
        Self::OctoprintFan,
        Self::SafetyEnvelope,
        Self::Harness,
        Self::Model,
        Self::SkillPath,
        Self::PromptTemplatePath,
        Self::ObservationIntervalS,
        Self::PrinterPollS,
        Self::UnattendedWindowS,
        Self::IngressAnswerBoundMs,
        Self::IngressSharedSecret,
        Self::ApiCredential,
        Self::ApiCredentialVerifier,
        Self::CameraSnapshotUrl,
        Self::ObicoUrl,
        Self::ObicoAccessToken,
        Self::ObicoMlApiUrl,
        Self::VisionBlankFrameMaxStddev,
        Self::SpoolsPreflight,
        Self::SpoolsBufferPercent,
        Self::SpoolsBufferMinG,
        Self::SpoolsAnalysisWaitSeconds,
        Self::RecoveryAwayFromBed,
        Self::RecoveryMaxZMm,
        Self::RecoveryMaxLiftMm,
        Self::RecoveryClearanceMarginMm,
        Self::RecoveryParkXy,
        Self::RecoveryZHomeXy,
        Self::RecoveryZHomeSweepsBed,
        Self::PolicyRecoveryOperator,
        Self::PolicyRecoveryAgent,
        Self::PolicyRecoveryAsserting,
    ];

    /// The dotted key this field is spelled under in the configuration file.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::StateDir => "state_dir",
            Self::Listen => "listen",
            Self::OctoprintUrl => "octoprint.url",
            Self::OctoprintApiKey => "octoprint.api_key",
            Self::OctoprintFan => "octoprint.fan",
            Self::SafetyEnvelope => "safety",
            Self::Harness => "supervisor.harness",
            Self::Model => "supervisor.model",
            Self::SkillPath => "supervisor.skill_path",
            Self::PromptTemplatePath => "supervisor.prompt_template_path",
            Self::ObservationIntervalS => "supervisor.observation_interval_s",
            Self::PrinterPollS => "supervisor.printer_poll_s",
            Self::UnattendedWindowS => "supervisor.unattended_window_s",
            Self::IngressAnswerBoundMs => "ingress.answer_bound_ms",
            Self::IngressSharedSecret => "ingress.shared_secret",
            Self::ApiCredential => "api.credential",
            Self::ApiCredentialVerifier => "api.credential_verifier",
            Self::CameraSnapshotUrl => "camera.snapshot_url",
            Self::ObicoUrl => "obico.url",
            Self::ObicoAccessToken => "obico.access_token",
            Self::ObicoMlApiUrl => "obico.ml_api_url",
            Self::VisionBlankFrameMaxStddev => "vision.blank_frame_max_stddev",
            Self::SpoolsPreflight => "spools.preflight",
            Self::SpoolsBufferPercent => "spools.buffer_percent",
            Self::SpoolsBufferMinG => "spools.buffer_min_g",
            Self::SpoolsAnalysisWaitSeconds => "spools.analysis_wait_seconds",
            Self::RecoveryAwayFromBed => "recovery.away_from_bed",
            Self::RecoveryMaxZMm => "recovery.max_z_mm",
            Self::RecoveryMaxLiftMm => "recovery.max_lift_mm",
            Self::RecoveryClearanceMarginMm => "recovery.clearance_margin_mm",
            Self::RecoveryParkXy => "recovery.park_xy",
            Self::RecoveryZHomeXy => "recovery.z_home_xy",
            Self::RecoveryZHomeSweepsBed => "recovery.z_home_sweeps_bed",
            Self::PolicyRecoveryOperator => "policy.recovery.operator",
            Self::PolicyRecoveryAgent => "policy.recovery.agent",
            Self::PolicyRecoveryAsserting => "policy.recovery.asserting",
        }
    }
}

impl core::fmt::Display for ConfigField {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(self.key())
    }
}

/// Why this server will not start under the configuration it was given.
///
/// Every variant names the field it is about, because the answer an operator
/// needs is which line of the file to change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// The file could not be read at all.
    Unreadable {
        /// The path that was named.
        path: PathBuf,
        /// What went wrong reading it.
        detail: String,
    },
    /// The file is not the document this program takes.
    Unparsable {
        /// The path that was named.
        path: PathBuf,
        /// What could not be parsed, in the parser's own words.
        detail: String,
    },
    /// One field carries a value this program cannot accept.
    Field {
        /// The field it is about.
        field: ConfigField,
        /// Why the value cannot be accepted.
        detail: String,
    },
}

impl ConfigError {
    /// The field this refusal is about, when it is about one.
    #[must_use]
    pub const fn field(&self) -> Option<ConfigField> {
        match self {
            Self::Field { field, .. } => Some(*field),
            _ => None,
        }
    }

    /// One field's refusal, in its own words.
    pub(crate) fn about(field: ConfigField, detail: impl Into<String>) -> Self {
        Self::Field {
            field,
            detail: detail.into(),
        }
    }
}

impl core::fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Unreadable { path, detail } => write!(
                formatter,
                "the configuration file {} could not be read: {detail}",
                path.display()
            ),
            Self::Unparsable { path, detail } => write!(
                formatter,
                "the configuration file {} could not be read as this program's \
                 configuration: {detail}",
                path.display()
            ),
            Self::Field { field, detail } => write!(
                formatter,
                "the configuration field `{field}` cannot be accepted: {detail}"
            ),
        }
    }
}

impl core::error::Error for ConfigError {}

/// Where the `OctoPrint` instance is and what it is like, as written down.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "printobserver_types::serde",
    deny_unknown_fields,
    rename_all = "snake_case"
)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct OctoprintSection {
    /// The base URL the instance answers on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// The key it authenticates every request by.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// Whether the machine has a part-cooling fan this server may command.
    #[serde(default = "commandable")]
    pub fan: String,
}

/// The vocabulary [`OctoprintSection::fan`] takes, spelled as the operator
/// writes it.
pub const FAN_VOCABULARY: [(&str, FanSupport); 2] = [
    ("commandable", FanSupport::Commandable),
    ("absent", FanSupport::Absent),
];

/// The fan setting a configuration naming none takes.
fn commandable() -> String {
    FAN_VOCABULARY[0].0.to_owned()
}

impl Default for OctoprintSection {
    fn default() -> Self {
        Self {
            url: None,
            api_key: None,
            fan: commandable(),
        }
    }
}

/// How the supervising agent is reached, as written down.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "printobserver_types::serde",
    deny_unknown_fields,
    rename_all = "snake_case"
)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct SupervisorSection {
    /// The harness identity turns run on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness: Option<String>,
    /// The model turns are pinned to, when one is pinned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The installed skill's `SKILL.md`, whose prose is sent as every turn's
    /// system prompt; the agent runs from the directory holding it. Required:
    /// this program carries no skill, and an absent one refuses the start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_path: Option<PathBuf>,
    /// The prompt template one turn fills, when the operator supplies one
    /// instead of the committed template this program carries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_template_path: Option<PathBuf>,
    /// How often every active print is given a periodic observation, and its
    /// agent a turn on it, in seconds. Left out, two minutes.
    #[serde(default = "default_observation_interval_s")]
    pub observation_interval_s: u64,
    /// How often the printer watch reads the machine, whether or not a print
    /// is active, in seconds: from 5 to 300. Left out, fifteen seconds.
    #[serde(default = "default_printer_poll_s")]
    pub printer_poll_s: u64,
    /// How long after a print ends `interrupted` or `failed` the printer
    /// heating or moving on its own is recorded, in seconds: from 60 to
    /// 86400. Left out, an hour.
    #[serde(default = "default_unattended_window_s")]
    pub unattended_window_s: u64,
}

/// The printer poll a configuration naming none takes.
const fn default_printer_poll_s() -> u64 {
    DEFAULT_PRINTER_POLL_S
}

/// The unattended window a configuration naming none takes.
const fn default_unattended_window_s() -> u64 {
    DEFAULT_UNATTENDED_WINDOW_S
}

/// The observation interval a configuration naming none takes.
const fn default_observation_interval_s() -> u64 {
    DEFAULT_OBSERVATION_INTERVAL_S
}

impl Default for SupervisorSection {
    fn default() -> Self {
        Self {
            harness: None,
            model: None,
            skill_path: None,
            prompt_template_path: None,
            observation_interval_s: DEFAULT_OBSERVATION_INTERVAL_S,
            printer_poll_s: DEFAULT_PRINTER_POLL_S,
            unattended_window_s: DEFAULT_UNATTENDED_WINDOW_S,
        }
    }
}

/// What the `Obico` ingress requires and how fast it answers, as written down.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "printobserver_types::serde",
    deny_unknown_fields,
    rename_all = "snake_case"
)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct IngressSection {
    /// The shared secret every post must carry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared_secret: Option<String>,
    /// How long the ingress may take to answer, in milliseconds.
    #[serde(default = "default_answer_bound_ms")]
    pub answer_bound_ms: u64,
}

/// The answer bound a configuration naming none takes.
const fn default_answer_bound_ms() -> u64 {
    DEFAULT_INGRESS_ANSWER_BOUND_MS
}

impl Default for IngressSection {
    fn default() -> Self {
        Self {
            shared_secret: None,
            answer_bound_ms: DEFAULT_INGRESS_ANSWER_BOUND_MS,
        }
    }
}

/// What a caller of the versioned API authenticates with, as written down.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(
    crate = "printobserver_types::serde",
    deny_unknown_fields,
    rename_all = "snake_case"
)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct ApiSection {
    /// A plaintext operator credential. Superseded by `credential_verifier`:
    /// still admitted until it is replaced, with a warning on every start,
    /// because a supervision turn can read the file it is written in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential: Option<String>,
    /// The verifier the operator's credential is checked against:
    /// `sha256:` and the 64 lowercase hexadecimal digits of the SHA-256 of the
    /// credential's UTF-8 bytes, as `printobserver credential issue` prints it.
    /// Left out, the server reads `api-credential.verifier` in its state
    /// directory, and admits no operator at all when that is absent too.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_verifier: Option<String>,
}

/// Where a fresh frame of the print is fetched from, as written down.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(
    crate = "printobserver_types::serde",
    deny_unknown_fields,
    rename_all = "snake_case"
)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct CameraSection {
    /// An `http` or `https` URL answering one still image of the print, such
    /// as a `go2rtc` `frame.jpeg` address. Left out, a look carries no frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_url: Option<String>,
}

/// Where `Obico`'s own API answers, as written down.
///
/// Its debug form never shows the token.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(
    crate = "printobserver_types::serde",
    deny_unknown_fields,
    rename_all = "snake_case"
)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct ObicoSection {
    /// The server's own `http` or `https` address, such as
    /// `http://127.0.0.1:3334`. Given with `access_token` or not at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// A bearer token for its API, issued by the instance's own OAuth
    /// administration. Left out with the address, a print the detector paused
    /// and the supervisor resumed is not acknowledged to `Obico`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    /// The failure detector's own service — `Obico`'s `ml_api` — as an `http`
    /// or `https` address, whose unauthenticated `/hc/` is asked during every
    /// print whether detection is up. Left out, nothing checks it and its
    /// health reads `unchecked`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ml_api_url: Option<String>,
}

/// How a camera frame is judged, as written down.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(
    crate = "printobserver_types::serde",
    deny_unknown_fields,
    rename_all = "snake_case"
)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct VisionSection {
    /// The grey-level standard deviation at or below which a frame is blank
    /// and is fetched again: from 1 to 128. Left out, 36.5.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blank_frame_max_stddev: Option<f64>,
}

impl core::fmt::Debug for ObicoSection {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("ObicoSection")
            .field("url", &self.url)
            .field(
                "access_token",
                &self.access_token.as_ref().map(|_| REDACTED),
            )
            .field("ml_api_url", &self.ml_api_url)
            .finish()
    }
}

/// How spools are checked before a print, as written down.
///
/// Every key is optional, and one left out takes the supervision core's own
/// default. The mode is read as text rather than as the core's own type so
/// that a mode outside the three is refused naming its key rather than by the
/// parser in words of its own.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(
    crate = "printobserver_types::serde",
    deny_unknown_fields,
    rename_all = "snake_case"
)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct SpoolsSection {
    /// What a start does when the loaded spool is short, unverified, or the
    /// file has no analysis: `refuse`, `warn` or `off`. Left out, `refuse`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preflight: Option<String>,
    /// The margin asked beyond what a print needs, as a percentage of it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buffer_percent: Option<f64>,
    /// The least margin asked, in grams, however small the print.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buffer_min_g: Option<f64>,
    /// How long a start waits for `OctoPrint`'s analysis of a file that has
    /// none yet, in whole seconds; zero waits not at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analysis_wait_seconds: Option<i64>,
}

/// The printer's own facts a recovery is checked against, as written down.
///
/// Left out altogether, or written with no key, no recovery action can be
/// checked and every one is refused as not configured. Written with any key,
/// every key without a default is required. Each is read as written so that a
/// value that cannot work is refused naming its key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(
    crate = "printobserver_types::serde",
    deny_unknown_fields,
    rename_all = "snake_case"
)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct RecoverySection {
    /// Which way along Z moves the nozzle away from the bed: `z_plus` or
    /// `z_minus`.
    // llmlint: ignore[invalid_states_unrepresentable] suppressions.toml has the reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub away_from_bed: Option<String>,
    /// The highest Z the machine reaches, in millimetres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_z_mm: Option<f64>,
    /// The longest lift one recovery may ask for, in millimetres; 50 when
    /// left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_lift_mm: Option<f64>,
    /// How far above the tallest part the nozzle must be, in millimetres; 5
    /// when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clearance_margin_mm: Option<f64>,
    /// Where a park moves the head, as `[x, y]` in millimetres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub park_xy: Option<Vec<f64>>,
    /// Where the machine homes Z, as `[x, y]` in millimetres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub z_home_xy: Option<Vec<f64>>,
    /// Whether homing Z brings a gantry down over the whole bed.
    // llmlint: ignore[invalid_states_unrepresentable] suppressions.toml has the reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub z_home_sweeps_bed: Option<bool>,
}

/// Who may ask for which recovery action, as written down under
/// `[policy.recovery]`. Each list left out takes its default: the operator
/// every action, the agent none, and only the operator's assertions counting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(
    crate = "printobserver_types::serde",
    deny_unknown_fields,
    rename_all = "snake_case"
)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct RecoveryPolicySection {
    /// The recovery actions the operator may ask for, by name.
    // llmlint: ignore[invalid_states_unrepresentable] suppressions.toml has the reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator: Option<Vec<String>>,
    /// The recovery actions the agent may ask for, by name. `recover_home_z`
    /// is refused here.
    // llmlint: ignore[invalid_states_unrepresentable] suppressions.toml has the reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<Vec<String>>,
    /// The actor classes whose word that the nozzle or the bed is clear
    /// counts. `agent` is refused here.
    // llmlint: ignore[invalid_states_unrepresentable] suppressions.toml has the reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asserting: Option<Vec<String>>,
}

/// The policies this program is configured with beside the safety envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(
    crate = "printobserver_types::serde",
    deny_unknown_fields,
    rename_all = "snake_case"
)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct PolicySection {
    /// Who may ask for which recovery action.
    #[serde(default)]
    pub recovery: RecoveryPolicySection,
}

/// The whole configuration file, exactly as it is written down.
///
/// This is the parsed document rather than the validated configuration:
/// [`ServerConfig::load`] is what turns one into the other, and every value a
/// later step could find wrong is ruled on there.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "printobserver_types::serde",
    deny_unknown_fields,
    rename_all = "snake_case"
)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct ConfigFile {
    /// Where the store, the images and the sessions live.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_dir: Option<PathBuf>,
    /// The address the API and the ingress are served on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub listen: Option<String>,
    /// Where the `OctoPrint` instance is.
    #[serde(default)]
    pub octoprint: OctoprintSection,
    /// The operator's safety envelope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safety: Option<SafetyEnvelope>,
    /// How the supervising agent is reached.
    #[serde(default)]
    pub supervisor: SupervisorSection,
    /// What the `Obico` ingress requires.
    #[serde(default)]
    pub ingress: IngressSection,
    /// What a caller of the versioned API authenticates with.
    #[serde(default)]
    pub api: ApiSection,
    /// Where a fresh frame of the print is fetched from.
    #[serde(default)]
    pub camera: CameraSection,
    /// Where `Obico`'s own API answers.
    #[serde(default)]
    pub obico: ObicoSection,
    /// How a camera frame is judged.
    #[serde(default)]
    pub vision: VisionSection,
    /// How spools are checked before a print.
    #[serde(default)]
    pub spools: SpoolsSection,
    /// The printer's own facts a recovery is checked against.
    #[serde(default)]
    pub recovery: RecoverySection,
    /// The policies beside the safety envelope.
    #[serde(default)]
    pub policy: PolicySection,
}

/// The shared secret the ingress requires of every post.
///
/// Neither rendering of this type shows the value, so it cannot reach a log
/// record, a panic message or an error's own text by being formatted — and the
/// comparison that admits a post lives here rather than at the endpoint, so
/// there is no way to read the secret out in order to compare it.
#[derive(Clone, PartialEq, Eq)]
pub struct SharedSecret(String);

/// What a secret renders as, wherever a value carrying one is rendered.
pub const REDACTED: &str = "<redacted>";

impl SharedSecret {
    /// The secret this text names.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] when the text is empty or is nothing but
    /// whitespace: anything that can post to the ingress can pause a printer,
    /// and a secret nothing has to carry is no secret.
    pub fn new(value: &str) -> Result<Self, ConfigError> {
        if value.trim().is_empty() {
            return Err(ConfigError::about(
                ConfigField::IngressSharedSecret,
                "it is empty, and anything that can post to the ingress can pause a printer",
            ));
        }
        Ok(Self(value.to_owned()))
    }

    /// Whether a post carried this secret.
    ///
    /// Compared over the whole of both values with the two lengths mixed in,
    /// rather than by returning at the first difference or at a length
    /// mismatch, so that how long this takes says nothing about how much of the
    /// secret a caller guessed.
    #[must_use]
    pub fn matches(&self, offered: Option<&str>) -> bool {
        let Some(offered) = offered else {
            return false;
        };
        let expected = self.0.as_bytes();
        let mut difference = expected.len() ^ offered.len();
        for (index, byte) in offered.bytes().enumerate() {
            let against = expected[index % expected.len().max(1)];
            difference |= usize::from(byte ^ against);
        }
        difference == 0
    }
}

impl core::fmt::Display for SharedSecret {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(REDACTED)
    }
}

impl core::fmt::Debug for SharedSecret {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "SharedSecret({REDACTED})")
    }
}

/// How many random bytes a credential this server generates is drawn from.
pub const GENERATED_CREDENTIAL_BYTES: usize = 32;

/// One credential in plaintext: a legacy `api.credential`, the legacy file a
/// server generated into its state directory, or one `printobserver credential
/// issue` draws for the operator's own configuration.
///
/// Neither rendering of this type shows the value. Its text is read in exactly
/// one place — [`Self::written`], for the operator's own client configuration,
/// which exists to carry it, and for the verifier computed from it — and the
/// one comparison a presented credential is admitted by is [`Self::admits`], so
/// there is no second comparison anywhere that could return early.
#[derive(Clone)]
pub struct ApiCredential(String);

impl ApiCredential {
    /// The credential this text names.
    ///
    /// # Errors
    ///
    /// Answers why the text cannot be a credential, in words that never quote
    /// it: empty or nothing but whitespace, or carrying a control character or
    /// a character outside printable ASCII, or beginning or ending with a space
    /// — none of which an `Authorization` header carries intact.
    pub fn new(value: &str) -> Result<Self, &'static str> {
        if value.trim().is_empty() {
            return Err(
                "it is empty, and anything that can reach this server's API can command a printer",
            );
        }
        if !value.bytes().all(|byte| (b' '..=b'~').contains(&byte)) {
            return Err(
                "it carries a control character or a character outside printable ASCII, which \
                 no `Authorization` header carries intact",
            );
        }
        if value.starts_with(' ') || value.ends_with(' ') {
            return Err(
                "it begins or ends with a space, which an `Authorization` header does not carry",
            );
        }
        // llmlint: ignore[boundary_inputs_validated] The refusal set above is exactly the contract this server's API credential is planned against, shared with the nodes that build on it: empty or whitespace, a control or non-printable-ASCII byte, a leading or trailing space. A length floor would refuse operator credentials that contract admits, so it is recorded as a follow-up rather than added here; the default an operator gets without configuring one is the generated 32 random bytes.
        Ok(Self(value.to_owned()))
    }

    /// A fresh credential, drawn from the operating system's own
    /// cryptographically secure random source.
    ///
    /// [`GENERATED_CREDENTIAL_BYTES`] bytes, written as unpadded URL-safe
    /// base64 so an operator can copy it and a header can carry it as it is.
    ///
    /// # Errors
    ///
    /// Answers the random source's own refusal, when it has none to give.
    pub fn generate() -> Result<Self, getrandom::Error> {
        use base64::Engine as _;

        let mut drawn = [0_u8; GENERATED_CREDENTIAL_BYTES];
        getrandom::fill(&mut drawn)?;
        Ok(Self(
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(drawn),
        ))
    }

    /// Whether a presented credential is this one.
    ///
    /// The lengths are compared first, which says how long the credential is and
    /// nothing else; the bytes are then compared in constant time, so how long
    /// this takes says nothing about how much of it a caller guessed.
    #[must_use]
    pub fn admits(&self, presented: &[u8]) -> bool {
        use subtle::ConstantTimeEq as _;

        let expected = self.0.as_bytes();
        expected.len() == presented.len() && bool::from(expected.ct_eq(presented))
    }

    /// The text, for the one file whose purpose is to carry it — the
    /// operator's own client configuration — and nothing else.
    #[must_use]
    pub fn written(&self) -> &str {
        &self.0
    }

    /// The verifier this credential is checked by.
    #[must_use]
    pub fn verifier(&self) -> CredentialVerifier {
        CredentialVerifier::of(&self.0)
    }
}

impl PartialEq for ApiCredential {
    fn eq(&self, other: &Self) -> bool {
        self.admits(other.0.as_bytes())
    }
}

impl Eq for ApiCredential {}

impl core::fmt::Display for ApiCredential {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(REDACTED)
    }
}

impl core::fmt::Debug for ApiCredential {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "ApiCredential({REDACTED})")
    }
}

/// What a credential verifier is written with before its digest.
pub const VERIFIER_PREFIX: &str = "sha256:";

/// How many hexadecimal digits a verifier's digest is written in.
const VERIFIER_DIGITS: usize = 64;

/// What one operator credential is checked against: the SHA-256 of its UTF-8
/// bytes.
///
/// It is not a secret, so its rendering shows it — written as
/// [`VERIFIER_PREFIX`] and the digest in lowercase hexadecimal, which is the
/// one spelling [`Self::parse`] accepts. It is still printed by nothing but the
/// two commands that exist to print it, and logged by nothing. A verifier of a
/// short or guessable credential can be searched offline, which is why the
/// credential it verifies is drawn from 32 random bytes when this program
/// draws it.
#[derive(Clone, PartialEq, Eq)]
pub struct CredentialVerifier([u8; 32]);

impl CredentialVerifier {
    /// The verifier of one credential.
    #[must_use]
    pub fn of(credential: &str) -> Self {
        use sha2::Digest as _;

        Self(sha2::Sha256::digest(credential.as_bytes()).into())
    }

    /// The verifier this text writes down.
    ///
    /// # Errors
    ///
    /// Answers why the text is not one: anything but [`VERIFIER_PREFIX`]
    /// followed by exactly 64 lowercase hexadecimal digits.
    pub fn parse(text: &str) -> Result<Self, &'static str> {
        const SHAPE: &str = "it is not `sha256:` followed by the 64 lowercase hexadecimal digits                              of a SHA-256 digest, which is what `printobserver credential issue`                              and `printobserver credential verifier` print";
        let digits = text.strip_prefix(VERIFIER_PREFIX).ok_or(SHAPE)?;
        if digits.len() != VERIFIER_DIGITS
            || !digits
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(SHAPE);
        }
        let mut digest = [0_u8; 32];
        for (index, pair) in digits.as_bytes().chunks(2).enumerate() {
            let high = hex_value(pair[0]);
            let low = hex_value(pair[1]);
            digest[index] = (high << 4) | low;
        }
        Ok(Self(digest))
    }

    /// Whether a presented credential is the one this verifies.
    ///
    /// The presented bytes are digested and the two digests compared in
    /// constant time, so how long this takes says nothing about how much of
    /// the credential a caller guessed.
    #[must_use]
    pub fn admits(&self, presented: &[u8]) -> bool {
        use sha2::Digest as _;
        use subtle::ConstantTimeEq as _;

        let offered: [u8; 32] = sha2::Sha256::digest(presented).into();
        bool::from(self.0.ct_eq(&offered))
    }
}

/// One lowercase hexadecimal digit's value, for a digit already checked.
const fn hex_value(digit: u8) -> u8 {
    if digit.is_ascii_digit() {
        digit - b'0'
    } else {
        digit - b'a' + 10
    }
}

impl core::fmt::Display for CredentialVerifier {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(VERIFIER_PREFIX)?;
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl core::fmt::Debug for CredentialVerifier {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "CredentialVerifier({self})")
    }
}

/// One running server's validated configuration.
///
/// Every value here is one no later step has to re-examine, and where a
/// validated type for one already exists it is that type rather than the text
/// it was written as: the `OctoPrint` instance is the adapter's own
/// configuration, which redacts the key it carries; the harness and the model
/// are the supervisor adapter's own names; and the ingress secret is a value
/// neither rendering of which shows.
#[derive(Debug, Clone, PartialEq)]
pub struct ServerConfig {
    /// Where the store, the images and the sessions live.
    pub state_dir: PathBuf,
    /// The address the API and the ingress are served on.
    pub listen: SocketAddr,
    /// Everything the printer adapter needs to reach the instance.
    pub octoprint: OctoPrintConfig,
    /// The operator's safety envelope.
    pub safety: SafetyEnvelope,
    /// The harness identity turns run on.
    pub harness: HarnessIdentity,
    /// The model turns are pinned to, when one is pinned.
    pub model: Option<ModelName>,
    /// The installed skill's `SKILL.md`, whose prose is sent as every turn's
    /// system prompt. The agent runs from the directory holding it, which is
    /// where the skill's own `reference/` documents sit.
    pub skill_path: PathBuf,
    /// The prompt template one turn fills, when the operator supplied one.
    pub prompt_template_path: Option<PathBuf>,
    /// How often every active print is given a periodic observation.
    pub observation_interval: core::time::Duration,
    /// How often the printer watch reads the machine.
    pub printer_poll: core::time::Duration,
    /// How long after an unfinished ending the printer acting on its own is
    /// recorded.
    pub unattended_window: core::time::Duration,
    /// The grey-level spread at or below which a camera frame is blank.
    pub blank_frame_max_stddev: BlankFrameBound,
    /// Where the failure detector's own service answers its health check,
    /// when configured.
    pub obico_ml_api_url: Option<WebAddress>,
    /// How long the ingress may take to answer.
    pub ingress_answer_bound: core::time::Duration,
    /// The shared secret every post to the ingress must carry.
    pub ingress_shared_secret: SharedSecret,
    /// A plaintext operator credential the configuration still carries, which
    /// is admitted — with a warning on every start — until it is replaced by
    /// its verifier.
    pub api_credential: Option<ApiCredential>,
    /// The verifier the operator's credential is checked against, when the
    /// configuration names one. Absent, the composition root reads the one in
    /// the state directory.
    pub api_credential_verifier: Option<CredentialVerifier>,
    /// Where a fresh frame of the print is fetched from, when configured.
    pub camera_snapshot_url: Option<WebAddress>,
    /// `Obico`'s own API, when configured; its debug form omits the token.
    pub obico_api: Option<ObicoApi>,
    /// How spools are checked before a print, and the margin asked for.
    pub spools: SpoolPolicy,
    /// The printer's own facts a recovery is checked against, when stated.
    pub recovery: Option<RecoveryConfig>,
    /// Who may ask for which recovery action.
    pub recovery_grants: RecoveryGrants,
}

impl ServerConfig {
    /// Read and validate the one configuration file this program takes.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Unreadable`] when the file is not there,
    /// [`ConfigError::Unparsable`] when it is not this program's configuration,
    /// and [`ConfigError::Field`] naming the one field whose value cannot be
    /// accepted. The two values only the machine can rule on — the `OctoPrint`
    /// address and key — are ruled on by [`crate::Server::start`], which is
    /// where a printer exists to ask.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        Self::parse(&read(path)?, path)
    }

    /// The same, over a document already read.
    ///
    /// # Errors
    ///
    /// The same as [`ServerConfig::load`], less the unreadable file.
    pub fn parse(text: &str, path: &Path) -> Result<Self, ConfigError> {
        let file: ConfigFile = toml::from_str(text).map_err(|error| ConfigError::Unparsable {
            path: path.to_path_buf(),
            detail: error.to_string(),
        })?;
        Self::validate(file)
    }

    /// Rule on every value the document carries.
    ///
    /// Absence is ruled on here rather than by the parser, because a parser's
    /// own "missing field" says which key it wanted and not which field of this
    /// program that is. Every refusal below names the dotted key an operator
    /// would edit.
    fn validate(file: ConfigFile) -> Result<Self, ConfigError> {
        let state_dir =
            state_directory(required(ConfigField::StateDir, file.state_dir)?.as_path())?;
        let listen = listen_address(&required(ConfigField::Listen, file.listen)?)?;
        let octoprint = octoprint(&file.octoprint)?;
        let safety = required(ConfigField::SafetyEnvelope, file.safety)?;
        check_envelope(&safety)?;
        let harness = harness_identity(file.supervisor.harness)?;
        let model = match file.supervisor.model {
            None => None,
            Some(name) => Some(
                ModelName::new(&named(ConfigField::Model, Some(name))?)
                    .map_err(|error| ConfigError::about(ConfigField::Model, error.to_string()))?,
            ),
        };
        let skill_path = skill(file.supervisor.skill_path.as_deref())?;
        let prompt_template_path = readable(
            ConfigField::PromptTemplatePath,
            file.supervisor.prompt_template_path.as_deref(),
        )?;
        let observation_interval = observation_interval(file.supervisor.observation_interval_s)?;
        let printer_poll = seconds_within(
            ConfigField::PrinterPollS,
            file.supervisor.printer_poll_s,
            PRINTER_POLL_RANGE_S,
        )?;
        let unattended_window = seconds_within(
            ConfigField::UnattendedWindowS,
            file.supervisor.unattended_window_s,
            UNATTENDED_WINDOW_RANGE_S,
        )?;
        let blank_frame_max_stddev = blank_frame_bound(file.vision.blank_frame_max_stddev)?;
        let obico_ml_api_url = file
            .obico
            .ml_api_url
            .as_deref()
            .map(|url| web_address(ConfigField::ObicoMlApiUrl, url))
            .transpose()?;
        let ingress_answer_bound = answer_bound(file.ingress.answer_bound_ms)?;
        let ingress_shared_secret = SharedSecret::new(&required(
            ConfigField::IngressSharedSecret,
            file.ingress.shared_secret,
        )?)?;
        let api_credential = file
            .api
            .credential
            .as_deref()
            .map(|value| {
                ApiCredential::new(value)
                    .map_err(|why| ConfigError::about(ConfigField::ApiCredential, why))
            })
            .transpose()?;
        let api_credential_verifier = file
            .api
            .credential_verifier
            .as_deref()
            .map(|value| {
                CredentialVerifier::parse(value.trim())
                    .map_err(|why| ConfigError::about(ConfigField::ApiCredentialVerifier, why))
            })
            .transpose()?;
        let camera_snapshot_url = file
            .camera
            .snapshot_url
            .map(|url| web_address(ConfigField::CameraSnapshotUrl, &url))
            .transpose()?;
        let obico_api = obico_api(file.obico)?;
        let spools = spool_policy(&file.spools)?;
        let recovery = recovery_facts(&file.recovery)?;
        let recovery_grants = recovery_grants(&file.policy.recovery)?;
        Ok(Self {
            state_dir,
            listen,
            octoprint,
            safety,
            harness,
            model,
            skill_path,
            prompt_template_path,
            observation_interval,
            printer_poll,
            unattended_window,
            blank_frame_max_stddev,
            obico_ml_api_url,
            ingress_answer_bound,
            ingress_shared_secret,
            api_credential,
            api_credential_verifier,
            camera_snapshot_url,
            obico_api,
            spools,
            recovery,
            recovery_grants,
        })
    }

    /// Where this server materializes the agent's prompt template and
    /// assessment schema. The skill is not among them: it is the installed one
    /// `skill_path` names.
    #[must_use]
    pub fn assets_dir(&self) -> PathBuf {
        self.state_dir.join(ASSETS_DIRECTORY)
    }
}

/// One address a field names, refused unless it is an `http` or `https` URL
/// naming a host, by the field's own key and without quoting the value.
fn web_address(field: ConfigField, url: &str) -> Result<WebAddress, ConfigError> {
    WebAddress::new(url).map_err(|why| ConfigError::about(field, why))
}

/// `Obico`'s API, when both its address and its token are written down.
///
/// One without the other is refused naming the one that is missing: an address
/// with no token is a request `Obico` refuses, and a token with no address is a
/// secret written down for nothing.
fn obico_api(section: ObicoSection) -> Result<Option<ObicoApi>, ConfigError> {
    match (section.url, section.access_token) {
        (None, None) => Ok(None),
        (Some(_), None) => Err(ConfigError::about(
            ConfigField::ObicoAccessToken,
            "it is required when obico.url is given",
        )),
        (None, Some(_)) => Err(ConfigError::about(
            ConfigField::ObicoUrl,
            "it is required when obico.access_token is given",
        )),
        (Some(url), Some(access_token)) => {
            let url = web_address(ConfigField::ObicoUrl, &url)?;
            let access_token = AccessToken::new(&access_token)
                .map_err(|why| ConfigError::about(ConfigField::ObicoAccessToken, why))?;
            Ok(Some(ObicoApi::new(url, access_token)))
        }
    }
}

/// The two values signing a harness in reads, and nothing else.
///
/// An operator signs the agent in before or after filling in the machine and
/// the failure detector, so `printobserver sign-in` reads the state directory
/// and the harness identity out of the server's own file and nothing beside
/// them: every other value the file carries, present or not and valid or not,
/// is one it never looks at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignInConfig {
    /// Where the state lives, and so where the harness keeps its sign-in.
    pub state_dir: PathBuf,
    /// The harness identity supervision turns run on.
    pub harness: HarnessIdentity,
}

impl SignInConfig {
    /// Read those two values out of one configuration file.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Unreadable`] when the file is not there,
    /// [`ConfigError::Unparsable`] when it is not a document carrying either
    /// value in the shape this program reads it in, and [`ConfigError::Field`]
    /// naming `state_dir` or `supervisor.harness` when that value cannot be
    /// accepted.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        let file: SignInFile =
            toml::from_str(&read(path)?).map_err(|error| ConfigError::Unparsable {
                path: path.to_path_buf(),
                detail: error.to_string(),
            })?;
        Ok(Self {
            state_dir: state_directory(required(ConfigField::StateDir, file.state_dir)?.as_path())?,
            harness: harness_identity(file.supervisor.harness)?,
        })
    }
}

/// The configuration file, read for the two values signing in takes.
///
/// Unlike [`ConfigFile`] it refuses no key it does not know, because every key
/// but these two is one signing in does not read.
#[derive(Deserialize)]
#[serde(crate = "printobserver_types::serde")]
struct SignInFile {
    /// Where the state lives.
    #[serde(default)]
    state_dir: Option<PathBuf>,
    /// The supervisor's section, for its harness alone.
    #[serde(default)]
    supervisor: SignInSupervisor,
}

/// The supervisor's section, read for the harness alone.
#[derive(Default, Deserialize)]
#[serde(crate = "printobserver_types::serde")]
struct SignInSupervisor {
    /// The harness identity turns run on.
    #[serde(default)]
    harness: Option<String>,
}

/// One configuration file's text.
fn read(path: &Path) -> Result<String, ConfigError> {
    std::fs::read_to_string(path).map_err(|error| ConfigError::Unreadable {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })
}

/// The harness identity, refused by its own key when it names nothing.
fn harness_identity(value: Option<String>) -> Result<HarnessIdentity, ConfigError> {
    HarnessIdentity::new(&named(ConfigField::Harness, value)?)
        .map_err(|error| ConfigError::about(ConfigField::Harness, error.to_string()))
}

/// One field the file has to carry, refused by its own key when it does not.
fn required<T>(field: ConfigField, value: Option<T>) -> Result<T, ConfigError> {
    value.ok_or_else(|| ConfigError::about(field, "the configuration file carries no value for it"))
}

/// One field that has to carry a name rather than nothing at all.
fn named(field: ConfigField, value: Option<String>) -> Result<String, ConfigError> {
    let value = required(field, value)?;
    if value.trim().is_empty() {
        return Err(ConfigError::about(
            field,
            "it is empty, and an empty value names nothing",
        ));
    }
    Ok(value.trim().to_owned())
}

/// The state directory, created where it is not there.
fn state_directory(named: &Path) -> Result<PathBuf, ConfigError> {
    if named.as_os_str().is_empty() {
        return Err(ConfigError::about(
            ConfigField::StateDir,
            "it names no directory, and this server's whole state lives in one",
        ));
    }
    std::fs::create_dir_all(named).map_err(|error| {
        ConfigError::about(
            ConfigField::StateDir,
            format!("{} cannot be created: {error}", named.display()),
        )
    })?;
    named.canonicalize().map(plainly_written).map_err(|error| {
        ConfigError::about(
            ConfigField::StateDir,
            format!("{} cannot be resolved: {error}", named.display()),
        )
    })
}

/// A resolved path, written the way its own platform's tools write it.
///
/// Resolving a path on Windows answers it in its verbatim form, `\\?\C:\…`:
/// one this program opens as readily as any other, and one no operator types and
/// some of the programs a supervision turn runs refuse — and every image path
/// this server answers is written under it. Where that form names an ordinary
/// drive path it is written as that path. Any other path, which on every other
/// platform is every path, is answered as it is.
#[must_use]
pub fn plainly_written(path: PathBuf) -> PathBuf {
    const VERBATIM: &str = r"\\?\";
    let drive = path
        .to_str()
        .and_then(|text| text.strip_prefix(VERBATIM))
        .filter(|rest| {
            let bytes = rest.as_bytes();
            bytes.len() >= 3
                && bytes[0].is_ascii_alphabetic()
                && bytes[1] == b':'
                && bytes[2] == b'\\'
        })
        .map(PathBuf::from);
    drive.unwrap_or(path)
}

/// The address to serve on, as an address rather than as text.
fn listen_address(named: &str) -> Result<SocketAddr, ConfigError> {
    named.trim().parse().map_err(|error| {
        ConfigError::about(
            ConfigField::Listen,
            format!("{named:?} is no address to listen on: {error}"),
        )
    })
}

/// Everything the printer adapter needs to reach the instance, refused by the
/// field whose value it could not take.
fn octoprint(section: &OctoprintSection) -> Result<OctoPrintConfig, ConfigError> {
    let fan = fan_support(&section.fan)?;
    let url = named(ConfigField::OctoprintUrl, section.url.clone())?;
    let key = named(ConfigField::OctoprintApiKey, section.api_key.clone())?;
    OctoPrintConfig::new(&url, key)
        .map(|configured| configured.with_fan(fan))
        .map_err(|error| {
            let field = match error {
                printobserver_octoprint::ConfigError::EmptyApiKey => ConfigField::OctoprintApiKey,
                _ => ConfigField::OctoprintUrl,
            };
            ConfigError::about(field, error.to_string())
        })
}

/// The fan setting one word of the vocabulary names.
fn fan_support(named: &str) -> Result<FanSupport, ConfigError> {
    FAN_VOCABULARY
        .iter()
        .find(|(spelling, _)| *spelling == named.trim())
        .map(|(_, support)| *support)
        .ok_or_else(|| {
            let vocabulary: Vec<&str> = FAN_VOCABULARY.iter().map(|(word, _)| *word).collect();
            ConfigError::about(
                ConfigField::OctoprintFan,
                format!("{named:?} is not one of {}", vocabulary.join(" or ")),
            )
        })
}

/// The envelope, refused when a range it declares admits nothing.
fn check_envelope(envelope: &SafetyEnvelope) -> Result<(), ConfigError> {
    if envelope.allowed.is_empty() {
        return Err(ConfigError::about(
            ConfigField::SafetyEnvelope,
            "it allows no adjustable at all, so no adjustment could ever be accepted",
        ));
    }
    for (adjustable, range) in &envelope.allowed {
        if !range.min.is_finite() || !range.max.is_finite() || range.min > range.max {
            return Err(ConfigError::about(
                ConfigField::SafetyEnvelope,
                format!(
                    "the range allowed for {adjustable} runs from {} to {}, which admits \
                     no value",
                    range.min, range.max
                ),
            ));
        }
    }
    // Closing a print record is the operator's judgement that the print is
    // over, so no configuration may hand it to the agent watching the print.
    if envelope
        .actions
        .get(&ActorClass::Agent)
        .is_some_and(|granted| granted.contains(&ActionKind::ClosePrint))
    {
        return Err(ConfigError::about(
            ConfigField::SafetyEnvelope,
            "it grants `close_print` to the agent, and closing a print record is the \
             operator's judgement that the print is over",
        ));
    }
    if envelope.agent_min_interval_s < 0 {
        return Err(ConfigError::about(
            ConfigField::SafetyEnvelope,
            format!(
                "the agent's minimum interval is {}s, and an interval is a count of seconds",
                envelope.agent_min_interval_s
            ),
        ));
    }
    Ok(())
}

/// One optional path, read where it was named so that a path naming nothing is
/// refused here rather than hours later.
fn readable(field: ConfigField, named: Option<&Path>) -> Result<Option<PathBuf>, ConfigError> {
    let Some(path) = named else {
        return Ok(None);
    };
    std::fs::read_to_string(path)
        .map(|_| Some(path.to_path_buf()))
        .map_err(|error| {
            ConfigError::about(field, format!("{} cannot be read: {error}", path.display()))
        })
}

/// The installed skill, refused naming how to install one when it is absent or
/// cannot be read.
fn skill(named: Option<&Path>) -> Result<PathBuf, ConfigError> {
    let Some(path) = named else {
        return Err(ConfigError::about(
            ConfigField::SkillPath,
            format!(
                "the configuration file carries no value for it, and this program carries \
                 no skill of its own: {SKILL_INSTALL}"
            ),
        ));
    };
    std::fs::read_to_string(path)
        .map(|_| path.to_path_buf())
        .map_err(|error| {
            ConfigError::about(
                ConfigField::SkillPath,
                format!(
                    "{} cannot be read: {error}. {SKILL_INSTALL}",
                    path.display()
                ),
            )
        })
}

/// How spools are checked, each key left out taking the core's own default.
fn spool_policy(section: &SpoolsSection) -> Result<SpoolPolicy, ConfigError> {
    let defaults = SpoolPolicy::default();
    let preflight = match section.preflight.as_deref() {
        None => defaults.preflight,
        Some("refuse") => PreflightMode::Refuse,
        Some("warn") => PreflightMode::Warn,
        Some("off") => PreflightMode::Off,
        Some(_) => {
            return Err(ConfigError::about(
                ConfigField::SpoolsPreflight,
                "the spool check is one of `refuse`, `warn` or `off`",
            ));
        }
    };
    let margin = |field: ConfigField, value: Option<f64>, default: f64| match value {
        None => Ok(default),
        Some(value) if value.is_finite() && value >= 0.0 => Ok(value),
        Some(_) => Err(ConfigError::about(
            field,
            "a margin is a figure at or above zero; a negative one asks a spool for less \
             than the print needs",
        )),
    };
    let buffer_percent = margin(
        ConfigField::SpoolsBufferPercent,
        section.buffer_percent,
        defaults.buffer_percent,
    )?;
    let buffer_min_g = margin(
        ConfigField::SpoolsBufferMinG,
        section.buffer_min_g,
        defaults.buffer_min_g,
    )?;
    let analysis_wait_s = match section.analysis_wait_seconds {
        None => defaults.analysis_wait_s,
        Some(seconds) => u64::try_from(seconds).map_err(|_| {
            ConfigError::about(
                ConfigField::SpoolsAnalysisWaitSeconds,
                "a wait is a number of seconds at or above zero",
            )
        })?,
    };
    Ok(SpoolPolicy {
        preflight,
        buffer_percent,
        buffer_min_g,
        analysis_wait_s,
    })
}

/// The printer's recovery facts, absent when the section states none.
fn recovery_facts(section: &RecoverySection) -> Result<Option<RecoveryConfig>, ConfigError> {
    if *section == RecoverySection::default() {
        return Ok(None);
    }
    let absent = |field: ConfigField| {
        ConfigError::about(
            field,
            "`[recovery]` states this printer's facts, and this one is one of them",
        )
    };
    let away_from_bed = match section.away_from_bed.as_deref() {
        None => return Err(absent(ConfigField::RecoveryAwayFromBed)),
        Some("z_plus") => AwayFromBed::ZPlus,
        Some("z_minus") => AwayFromBed::ZMinus,
        Some(other) => {
            return Err(ConfigError::about(
                ConfigField::RecoveryAwayFromBed,
                format!("{other:?} is neither `z_plus` nor `z_minus`"),
            ));
        }
    };
    let max_z_mm = section
        .max_z_mm
        .ok_or_else(|| absent(ConfigField::RecoveryMaxZMm))?;
    if !(max_z_mm.is_finite() && max_z_mm > 0.0) {
        return Err(ConfigError::about(
            ConfigField::RecoveryMaxZMm,
            format!("{max_z_mm} mm is no height a machine reaches; it is a figure above zero"),
        ));
    }
    let max_lift_mm = section
        .max_lift_mm
        .unwrap_or_else(|| DEFAULT_MAX_LIFT_MM.min(max_z_mm));
    if !(max_lift_mm.is_finite() && max_lift_mm > 0.0 && max_lift_mm <= max_z_mm) {
        return Err(ConfigError::about(
            ConfigField::RecoveryMaxLiftMm,
            format!(
                "{max_lift_mm} mm is outside the lifts a machine of {max_z_mm} mm makes: above \
                 zero, and at most `recovery.max_z_mm`"
            ),
        ));
    }
    let clearance_margin_mm = section
        .clearance_margin_mm
        .unwrap_or(DEFAULT_CLEARANCE_MARGIN_MM);
    if !(clearance_margin_mm.is_finite()
        && clearance_margin_mm >= 0.0
        && clearance_margin_mm < max_z_mm)
    {
        return Err(ConfigError::about(
            ConfigField::RecoveryClearanceMarginMm,
            format!(
                "{clearance_margin_mm} mm is outside the margins a machine of {max_z_mm} mm \
                 admits: at or above zero, and below `recovery.max_z_mm`"
            ),
        ));
    }
    let place = |field: ConfigField, written: Option<&Vec<f64>>| match written.map(Vec::as_slice) {
        None => Err(absent(field)),
        Some([x, y]) if x.is_finite() && y.is_finite() && *x >= 0.0 && *y >= 0.0 => Ok([*x, *y]),
        Some(_) => Err(ConfigError::about(
            field,
            "a place is `[x, y]`, two figures in millimetres at or above zero",
        )),
    };
    let park_xy = place(ConfigField::RecoveryParkXy, section.park_xy.as_ref())?;
    let z_home_xy = place(ConfigField::RecoveryZHomeXy, section.z_home_xy.as_ref())?;
    let z_home_sweeps_bed = section
        .z_home_sweeps_bed
        .ok_or_else(|| absent(ConfigField::RecoveryZHomeSweepsBed))?;
    Ok(Some(RecoveryConfig {
        away_from_bed,
        max_z_mm,
        max_lift_mm,
        clearance_margin_mm,
        park_xy,
        z_home_xy,
        z_home_sweeps_bed,
    }))
}

/// Who may ask for which recovery action, each list left out taking its
/// default, and the two grants no configuration may make refused.
fn recovery_grants(section: &RecoveryPolicySection) -> Result<RecoveryGrants, ConfigError> {
    let defaults = RecoveryGrants::default();
    let actions = |field: ConfigField, written: Option<&Vec<String>>, default: Vec<_>| {
        let Some(written) = written else {
            return Ok(default);
        };
        written
            .iter()
            .map(|name| {
                RecoveryActionKind::ALL
                    .iter()
                    .find(|kind| kind.as_str() == name.trim())
                    .copied()
                    .ok_or_else(|| {
                        ConfigError::about(
                            field,
                            format!("{name:?} is not a recovery action of this program"),
                        )
                    })
            })
            .collect::<Result<Vec<_>, _>>()
    };
    let operator = actions(
        ConfigField::PolicyRecoveryOperator,
        section.operator.as_ref(),
        defaults.operator,
    )?;
    let agent = actions(
        ConfigField::PolicyRecoveryAgent,
        section.agent.as_ref(),
        defaults.agent,
    )?;
    // Homing Z drives the nozzle or a gantry down onto whatever is on the bed,
    // and only a person standing at the machine can say nothing is.
    if agent.contains(&RecoveryActionKind::RecoverHomeZ) {
        return Err(ConfigError::about(
            ConfigField::PolicyRecoveryAgent,
            "it grants `recover_home_z` to the agent, and only a person at the machine can \
             say the bed is clear for Z to be homed",
        ));
    }
    let asserting = match section.asserting.as_ref() {
        None => defaults.asserting,
        Some(written) => written
            .iter()
            .map(|class| match class.trim() {
                "operator" => Ok(ActorClass::Operator),
                // An assertion is a person's word for what they can see, and
                // an agent's word for clearance is no measurement of it.
                "agent" => Err(ConfigError::about(
                    ConfigField::PolicyRecoveryAsserting,
                    "it lets the agent assert clearance, and an assertion is a person's word \
                     for what they can see at the machine",
                )),
                other => Err(ConfigError::about(
                    ConfigField::PolicyRecoveryAsserting,
                    format!("{other:?} is not an actor whose word can count; only `operator` is"),
                )),
            })
            .collect::<Result<Vec<_>, _>>()?,
    };
    Ok(RecoveryGrants {
        operator,
        agent,
        asserting,
    })
}

/// The observation interval, refused at zero.
fn observation_interval(seconds: u64) -> Result<core::time::Duration, ConfigError> {
    if seconds == 0 {
        return Err(ConfigError::about(
            ConfigField::ObservationIntervalS,
            "an interval of zero seconds observes a print again the moment its last turn \
             returns, which is a turn that never ends",
        ));
    }
    Ok(core::time::Duration::from_secs(seconds))
}

/// A number of seconds, refused outside the range its field admits.
fn seconds_within(
    field: ConfigField,
    seconds: u64,
    range: (u64, u64),
) -> Result<core::time::Duration, ConfigError> {
    let (least, most) = range;
    if !(least..=most).contains(&seconds) {
        return Err(ConfigError::about(
            field,
            format!("{seconds} seconds is outside the {least} to {most} seconds it admits"),
        ));
    }
    Ok(core::time::Duration::from_secs(seconds))
}

/// The blank-frame bound, refused outside the range it admits.
fn blank_frame_bound(written: Option<f64>) -> Result<BlankFrameBound, ConfigError> {
    written.map_or(Ok(BlankFrameBound::DEFAULT), |bound| {
        BlankFrameBound::new(bound).map_err(|refused| {
            ConfigError::about(
                ConfigField::VisionBlankFrameMaxStddev,
                format!(
                    "{refused} is outside the {MIN_BLANK_FRAME_MAX_STDDEV} to \
                     {MAX_BLANK_FRAME_MAX_STDDEV} grey levels it admits"
                ),
            )
        })
    })
}

/// The answer bound, refused when it is not one this program admits.
fn answer_bound(milliseconds: u64) -> Result<core::time::Duration, ConfigError> {
    if milliseconds == 0 {
        return Err(ConfigError::about(
            ConfigField::IngressAnswerBoundMs,
            "a bound of zero is an ingress that has run out of time before it starts",
        ));
    }
    if milliseconds >= OBICO_POSTING_TIMEOUT_MS {
        return Err(ConfigError::about(
            ConfigField::IngressAnswerBoundMs,
            format!(
                "{milliseconds}ms is not below the {OBICO_POSTING_TIMEOUT_MS}ms Obico posts \
                 under, so an alert answered at that bound is one Obico has already \
                 abandoned — and its posting does not retry"
            ),
        ));
    }
    Ok(core::time::Duration::from_millis(milliseconds))
}

#[cfg(test)]
mod tests {
    use super::CredentialVerifier;

    /// The SHA-256 of `abc`, as FIPS 180-2 gives it.
    const ABC: &str = "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    /// A verifier is the SHA-256 of the credential's UTF-8 bytes, written and
    /// read back in one spelling, and it admits that credential and no other.
    #[test]
    fn a_verifier_is_the_credentials_sha256_and_admits_only_it() {
        let verifier = CredentialVerifier::of("abc");

        assert_eq!(verifier.to_string(), ABC);
        assert_eq!(CredentialVerifier::parse(ABC), Ok(verifier.clone()));
        assert!(verifier.admits(b"abc"));
        assert!(!verifier.admits(b"abd"));
        assert!(!verifier.admits(b""));
    }

    /// Anything but `sha256:` and 64 lowercase hexadecimal digits is refused.
    #[test]
    fn a_verifier_in_any_other_spelling_is_refused() {
        for refused in [
            "",
            "sha256:",
            &ABC[..ABC.len() - 1],
            &format!("{ABC}0"),
            &ABC.to_uppercase(),
            &ABC.replace("sha256:", "sha512:"),
            &ABC.replace("sha256:", ""),
            &ABC.replace('a', "g"),
        ] {
            assert!(
                CredentialVerifier::parse(refused).is_err(),
                "{refused:?} was accepted"
            );
        }
    }
}
