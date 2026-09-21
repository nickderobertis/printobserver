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
//! # The API credential
//!
//! Every request beneath the versioned prefix carries one credential, and
//! [`ApiCredential`] is it. `api.credential` names it outright; left out, the
//! composition root generates one into the state directory the first time it
//! starts and reuses it after. Either way it is held in a type neither
//! rendering of which shows it, and the one comparison a presented credential
//! is admitted by is [`ApiCredential::admits`].

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use printobserver_core::SafetyEnvelope;
use printobserver_octoprint::{FanSupport, OctoPrintConfig};
use printobserver_oneharness::{HarnessIdentity, ModelName};
use printobserver_types::schemars::JsonSchema;
use printobserver_types::serde::{Deserialize, Serialize};

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
    /// How long the ingress may take to answer, in milliseconds.
    IngressAnswerBoundMs,
    /// The shared secret the ingress requires of every post.
    IngressSharedSecret,
    /// The credential every request to a versioned operation must carry, when
    /// the operator chose one rather than letting the server generate it.
    ApiCredential,
}

impl ConfigField {
    /// Every field this program takes, and there is no other.
    pub const ALL: [Self; 13] = [
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
        Self::ApiCredential,
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
            Self::ApiCredential => "api.credential",
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
#[derive(Default)]
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
    /// The credential every request must carry. Left out, the server generates
    /// one into its state directory and reuses it on every later start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential: Option<String>,
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

/// The credential every request to a versioned operation must carry.
///
/// Neither rendering of this type shows the value. Its text is read in exactly
/// one place outside this file — the client configuration the composition root
/// writes, which exists to carry it — and the one comparison a presented
/// credential is admitted by is [`Self::admits`], so there is no second
/// comparison anywhere that could return early.
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

    /// The text, for the one file whose purpose is to carry it.
    pub(crate) fn written(&self) -> &str {
        &self.0
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
    /// How long the ingress may take to answer.
    pub ingress_answer_bound: core::time::Duration,
    /// The shared secret every post to the ingress must carry.
    pub ingress_shared_secret: SharedSecret,
    /// The API credential the operator configured, when they configured one.
    /// Absent, the composition root takes the one in the state directory.
    pub api_credential: Option<ApiCredential>,
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
        Ok(Self {
            state_dir,
            listen,
            octoprint,
            safety,
            harness,
            model,
            skill_path,
            prompt_template_path,
            ingress_answer_bound,
            ingress_shared_secret,
            api_credential,
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
