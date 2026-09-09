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

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use printobserver_octoprint::FanSupport;
use printobserver_types::schemars::JsonSchema;
use printobserver_types::serde::{Deserialize, Serialize};
use printobserver_types::{ActionKind, ActorClass, Adjustable, Range, SafetyEnvelope};

/// The answer bound this repository ships, in milliseconds.
///
/// A fifth of [`OBICO_POSTING_TIMEOUT_MS`]. The margin is deliberate: the
/// supported host is a small ARM board beside the printer, and a bound set just
/// under the producer's timeout would be exceeded by the first slow moment on
/// it — after which `Obico` drops the alert, because its posting does not retry.
pub const DEFAULT_INGRESS_ANSWER_BOUND_MS: u64 = 1_000;

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
/// supervising agent's own committed assets into.
pub const ASSETS_DIRECTORY: &str = "assets";

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
    /// How long the ingress may take to answer, in milliseconds.
    IngressAnswerBoundMs,
    /// The shared secret the ingress requires of every post.
    IngressSharedSecret,
}

impl ConfigField {
    /// Every field this program takes, and there is no other.
    pub const ALL: [Self; 12] = [
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
        Self::IngressAnswerBoundMs,
        Self::IngressSharedSecret,
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
            Self::IngressAnswerBoundMs => "ingress.answer_bound_ms",
            Self::IngressSharedSecret => "ingress.shared_secret",
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
    pub url: String,
    /// The key it authenticates every request by.
    pub api_key: String,
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
    pub harness: String,
    /// The model turns are pinned to, when one is pinned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The skill to send as every turn's system prompt, when the operator
    /// supplies one instead of the committed skill this program carries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_path: Option<PathBuf>,
    /// The prompt template one turn fills, when the operator supplies one
    /// instead of the committed template this program carries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_template_path: Option<PathBuf>,
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
    pub shared_secret: String,
    /// How long the ingress may take to answer, in milliseconds.
    #[serde(default = "default_answer_bound_ms")]
    pub answer_bound_ms: u64,
}

/// The answer bound a configuration naming none takes.
const fn default_answer_bound_ms() -> u64 {
    DEFAULT_INGRESS_ANSWER_BOUND_MS
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
    pub state_dir: PathBuf,
    /// The address the API and the ingress are served on.
    pub listen: String,
    /// Where the `OctoPrint` instance is.
    pub octoprint: OctoprintSection,
    /// The operator's safety envelope.
    pub safety: SafetyEnvelope,
    /// How the supervising agent is reached.
    pub supervisor: SupervisorSection,
    /// What the `Obico` ingress requires.
    pub ingress: IngressSection,
}

/// One running server's validated configuration.
///
/// Every value here is one no later step has to re-examine: a path that could
/// not be made is not representable, an address that could not be parsed is
/// not representable, and a bound outside what this program admits is not
/// representable.
#[derive(Debug, Clone, PartialEq)]
pub struct ServerConfig {
    /// Where the store, the images and the sessions live.
    pub state_dir: PathBuf,
    /// The address the API and the ingress are served on.
    pub listen: SocketAddr,
    /// The base URL the `OctoPrint` instance answers on.
    pub octoprint_url: String,
    /// The key that instance authenticates every request by.
    pub octoprint_api_key: String,
    /// Whether the machine has a part-cooling fan this server may command.
    pub octoprint_fan: FanSupport,
    /// The operator's safety envelope.
    pub safety: SafetyEnvelope,
    /// The harness identity turns run on.
    pub harness: String,
    /// The model turns are pinned to, when one is pinned.
    pub model: Option<String>,
    /// The skill to send as every turn's system prompt, when the operator
    /// supplied one.
    pub skill_path: Option<PathBuf>,
    /// The prompt template one turn fills, when the operator supplied one.
    pub prompt_template_path: Option<PathBuf>,
    /// How long the ingress may take to answer.
    pub ingress_answer_bound: core::time::Duration,
    /// The shared secret every post to the ingress must carry.
    pub ingress_shared_secret: String,
}

/// A safety envelope wide enough to be worth writing down, for the
/// configuration this repository ships as an example.
#[must_use]
pub fn example_envelope() -> SafetyEnvelope {
    let mut allowed = BTreeMap::new();
    allowed.insert(Adjustable::Feedrate, Range::new(0.5, 1.5));
    allowed.insert(Adjustable::Flowrate, Range::new(0.9, 1.1));
    allowed.insert(Adjustable::Fan, Range::new(0.0, 100.0));
    allowed.insert(Adjustable::BedTarget, Range::new(0.0, 110.0));
    allowed.insert(Adjustable::ToolTarget { tool: 0 }, Range::new(0.0, 260.0));
    let every = vec![
        ActionKind::Pause,
        ActionKind::Resume,
        ActionKind::Cancel,
        ActionKind::StartPrint,
        ActionKind::SetFeedrateFactor,
        ActionKind::SetFlowrateFactor,
        ActionKind::SetToolTargetC,
        ActionKind::SetBedTargetC,
        ActionKind::SetFanPercent,
        ActionKind::AcknowledgeFailure,
    ];
    let mut actions = BTreeMap::new();
    actions.insert(ActorClass::Operator, every.clone());
    actions.insert(ActorClass::System, every.clone());
    actions.insert(ActorClass::Agent, every);
    SafetyEnvelope {
        allowed,
        actions,
        agent_min_interval_s: 0,
    }
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
        let text = std::fs::read_to_string(path).map_err(|error| ConfigError::Unreadable {
            path: path.to_path_buf(),
            detail: error.to_string(),
        })?;
        Self::parse(&text, path)
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
    fn validate(file: ConfigFile) -> Result<Self, ConfigError> {
        let state_dir = state_directory(&file.state_dir)?;
        let listen = listen_address(&file.listen)?;
        let octoprint_fan = fan_support(&file.octoprint.fan)?;
        if file.octoprint.url.trim().is_empty() {
            return Err(ConfigError::about(
                ConfigField::OctoprintUrl,
                "it names no OctoPrint instance",
            ));
        }
        if file.octoprint.api_key.trim().is_empty() {
            return Err(ConfigError::about(
                ConfigField::OctoprintApiKey,
                "it is empty, and an OctoPrint instance authenticates every request",
            ));
        }
        check_envelope(&file.safety)?;
        if file.supervisor.harness.trim().is_empty() {
            return Err(ConfigError::about(
                ConfigField::Harness,
                "it names no harness, and a harness identity selects the agent a turn runs on",
            ));
        }
        let model = match &file.supervisor.model {
            None => None,
            Some(name) if name.trim().is_empty() => {
                return Err(ConfigError::about(
                    ConfigField::Model,
                    "it is empty; a model nobody pinned is the field left out rather than \
                     a pin nothing can honour",
                ));
            }
            Some(name) => Some(name.trim().to_owned()),
        };
        let skill_path = readable(
            ConfigField::SkillPath,
            file.supervisor.skill_path.as_deref(),
        )?;
        let prompt_template_path = readable(
            ConfigField::PromptTemplatePath,
            file.supervisor.prompt_template_path.as_deref(),
        )?;
        let ingress_answer_bound = answer_bound(file.ingress.answer_bound_ms)?;
        if file.ingress.shared_secret.trim().is_empty() {
            return Err(ConfigError::about(
                ConfigField::IngressSharedSecret,
                "it is empty, and anything that can post to the ingress can pause a printer",
            ));
        }
        Ok(Self {
            state_dir,
            listen,
            octoprint_url: file.octoprint.url.trim().to_owned(),
            octoprint_api_key: file.octoprint.api_key,
            octoprint_fan,
            safety: file.safety,
            harness: file.supervisor.harness.trim().to_owned(),
            model,
            skill_path,
            prompt_template_path,
            ingress_answer_bound,
            ingress_shared_secret: file.ingress.shared_secret,
        })
    }

    /// Where this server materializes the agent's committed assets.
    #[must_use]
    pub fn assets_dir(&self) -> PathBuf {
        self.state_dir.join(ASSETS_DIRECTORY)
    }
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
    named.canonicalize().map_err(|error| {
        ConfigError::about(
            ConfigField::StateDir,
            format!("{} cannot be resolved: {error}", named.display()),
        )
    })
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
