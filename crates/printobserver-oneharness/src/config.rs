//! What the port is built with, and the seam a caller watches a turn through.
//!
//! Every field of a configuration that could be wrong carries a type that
//! cannot: a harness identity is non-empty because the only way to make one
//! rejects the empty string, a turn timeout is non-zero because it wraps a
//! non-zero integer, and an environment assignment has a `KEY=VALUE` shape
//! because it is a name and a value rather than a line somebody wrote. A
//! configuration this module hands back is therefore one no later check has to
//! re-examine — there is no representable value for it to find.

use core::fmt;
use core::num::NonZeroU64;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use oneharness_core::domain::report::RunReport;
use oneharness_core::io::run::RunRequest;
use oneharness_core::io::runner::ProcessSupervisor;

/// Why a value a configuration is built from is not one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// The harness identity was empty, or was nothing but whitespace.
    HarnessIdentityEmpty,
    /// The turn timeout was zero, which is a turn that has expired before it
    /// starts rather than a turn with no bound.
    TurnTimeoutZero,
    /// An environment assignment did not carry a `KEY=VALUE` shape.
    EnvAssignmentMalformed {
        /// What was offered as an assignment.
        text: String,
        /// Why it is not one.
        detail: &'static str,
    },
    /// The model was named as nothing, or as whitespace. A model nobody pinned
    /// is `None`; an empty name is a pin nothing can honour.
    ModelNameEmpty,
    /// The file named as the assessment schema is not a schema document.
    AssessmentSchemaInvalid {
        /// The path that was named.
        path: String,
        /// Why nothing there constrains an answer.
        detail: String,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HarnessIdentityEmpty => {
                write!(formatter, "the harness identity is empty")
            }
            Self::TurnTimeoutZero => {
                write!(
                    formatter,
                    "a turn timeout of zero seconds is a turn that has expired before it starts"
                )
            }
            Self::EnvAssignmentMalformed { text, detail } => {
                write!(
                    formatter,
                    "`{text}` is not a KEY=VALUE environment assignment: {detail}"
                )
            }
            Self::ModelNameEmpty => write!(formatter, "the model is named as nothing"),
            Self::AssessmentSchemaInvalid { path, detail } => {
                write!(formatter, "`{path}` does not constrain an answer: {detail}")
            }
        }
    }
}

impl core::error::Error for ConfigError {}

/// The harness identity turns run on, as `OneHarness` names it.
///
/// Non-empty by construction: `OneHarness` selects a harness by this name, and
/// an empty one selects nothing while reading as a configuration that was
/// merely left at its default.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HarnessIdentity(String);

impl HarnessIdentity {
    /// The identity named by this text.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::HarnessIdentityEmpty`] when the text is empty or
    /// is nothing but whitespace.
    pub fn new(name: &str) -> Result<Self, ConfigError> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(ConfigError::HarnessIdentityEmpty);
        }
        Ok(Self(trimmed.to_owned()))
    }

    /// The identity as `OneHarness` spells it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for HarnessIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// How long one supervision turn is given.
///
/// Non-zero by construction. A turn looks at one picture and writes one
/// paragraph, so the bound exists to stop a turn nothing is waiting for; a
/// bound of zero would stop every turn instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TurnTimeout(NonZeroU64);

impl TurnTimeout {
    /// The bound a configuration that names no other takes.
    pub const DEFAULT: Self = Self(NonZeroU64::new(300).expect("300 is not zero"));

    /// The bound of this many seconds.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::TurnTimeoutZero`] when the count is zero.
    pub fn new(seconds: u64) -> Result<Self, ConfigError> {
        NonZeroU64::new(seconds)
            .map(Self)
            .ok_or(ConfigError::TurnTimeoutZero)
    }

    /// The bound in seconds, as `OneHarness` takes it.
    #[must_use]
    pub fn seconds(self) -> u64 {
        self.0.get()
    }
}

impl fmt::Display for TurnTimeout {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}s", self.0)
    }
}

/// One `KEY=VALUE` assignment in a harness process's environment.
///
/// Held as the name and the value rather than as the line, so the line is
/// rendered rather than trusted: a value carrying its own `=` is ordinary and
/// stays part of the value, and there is no way to hold an assignment that has
/// no name at all.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EnvAssignment {
    /// The variable's name.
    name: String,
    /// What it is set to.
    value: String,
}

impl EnvAssignment {
    /// The assignment this text writes.
    ///
    /// The name is everything before the first `=` and the value is everything
    /// after it, so a value containing `=` needs no escaping.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::EnvAssignmentMalformed`] when the text carries no
    /// `=` at all, or when what precedes the first one is not a variable name.
    pub fn new(text: &str) -> Result<Self, ConfigError> {
        let malformed = |detail| ConfigError::EnvAssignmentMalformed {
            text: text.to_owned(),
            detail,
        };
        let (name, value) = text
            .split_once('=')
            .ok_or_else(|| malformed("it carries no `=`"))?;
        let mut characters = name.chars();
        let first = characters
            .next()
            .ok_or_else(|| malformed("it names no variable before the `=`"))?;
        if !(first.is_ascii_alphabetic() || first == '_')
            || !characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
        {
            return Err(malformed(
                "a variable name is a letter or an underscore followed by \
                 letters, digits and underscores",
            ));
        }
        Ok(Self {
            name: name.to_owned(),
            value: value.to_owned(),
        })
    }

    /// The variable's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// What the variable is set to.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }
}

impl fmt::Display for EnvAssignment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}={}", self.name, self.value)
    }
}

/// The model a turn is pinned to, when one is pinned.
///
/// A model nobody pinned is the absence of one of these rather than an empty
/// one: `OneHarness` reads an empty name as a model it cannot find, which
/// reaches a caller as a harness that will not run rather than as the
/// configuration mistake it is.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModelName(String);

impl ModelName {
    /// The model this text names.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::ModelNameEmpty`] when the text is empty or is
    /// nothing but whitespace.
    pub fn new(name: &str) -> Result<Self, ConfigError> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(ConfigError::ModelNameEmpty);
        }
        Ok(Self(trimmed.to_owned()))
    }

    /// The model as `OneHarness` spells it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ModelName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// The schema an answer is constrained by, named as a path `OneHarness` reads
/// per run.
///
/// Checked where it is named rather than where it is used: nothing about a
/// schema is read until a turn runs, so a path that is not there — or is there
/// and is not a document at all — would otherwise surface as every answer being
/// refused, hours after the configuration that caused it.
///
/// What this cannot establish is that the document is the *generated* artifact
/// rather than a schema somebody wrote: no property of a document says which
/// target produced it. That is a claim about the tree and is made there —
/// `just check-repo` refuses a tree in which this crate carries a schema of its
/// own, and one journey changes the generated artifact on disk and watches what
/// the port accepts move with it, which a document read here could not tell
/// apart from a copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssessmentSchema(PathBuf);

impl AssessmentSchema {
    /// The schema at this path, read to be sure something is there.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::AssessmentSchemaInvalid`] when the file cannot be
    /// read, is not JSON, or is not a JSON object — the three shapes that
    /// constrain nothing whatever `OneHarness` later makes of them.
    pub fn at(path: impl Into<PathBuf>) -> Result<Self, ConfigError> {
        let path = path.into();
        let invalid = |detail: String| ConfigError::AssessmentSchemaInvalid {
            path: path.display().to_string(),
            detail,
        };
        let text = std::fs::read_to_string(&path).map_err(|error| invalid(error.to_string()))?;
        let document: serde_json::Value =
            serde_json::from_str(&text).map_err(|error| invalid(error.to_string()))?;
        if !document.is_object() {
            return Err(invalid("it is JSON, but not a schema document".to_owned()));
        }
        Ok(Self(path))
    }

    /// The path handed to `OneHarness`, which reads it per run.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.0
    }
}

/// How the supervising agent is reached, and where its sessions live.
///
/// Every path is absolute in production. Nothing here is read until the port is
/// built, and the skill and the template are read exactly then — which is what
/// makes editing either of them a restart rather than a rebuild.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorConfig {
    /// The state directory this port keeps its sessions under. It holds both
    /// this port's own per-print ledger and `OneHarness`'s session store, which
    /// is what a restart continues a conversation from.
    pub state_dir: PathBuf,
    /// The `PrintObserver` skill, sent as every turn's system prompt.
    pub skill_path: PathBuf,
    /// The committed prompt template, whose three slots one turn fills.
    pub prompt_template_path: PathBuf,
    /// The generated assessment schema the agent's answer is constrained by.
    pub assessment_schema: AssessmentSchema,
    /// The harness identity turns run on.
    pub harness: HarnessIdentity,
    /// The model, when one is pinned rather than left to the harness.
    pub model: Option<ModelName>,
    /// The working directory each harness process runs in.
    pub working_dir: PathBuf,
    /// How long one turn is given.
    pub turn_timeout: TurnTimeout,
    /// The harness binary, when it is not the one `OneHarness` resolves by name.
    pub harness_bin: Option<PathBuf>,
    /// Extra environment for each harness process.
    pub harness_env: Vec<EnvAssignment>,
}

/// Receives every run request this port builds.
///
/// Observation only: a request is handed over after it is complete and before
/// `OneHarness` receives it, and an observer cannot change it.
pub trait RunRequestObserver: Send + Sync {
    /// One run request, exactly as it will be handed to `OneHarness`.
    fn built(&self, request: &RunRequest);
}

/// Receives the report of every run this port drives.
///
/// Observation only, and after the fact: a run's report carries far more than
/// the assessment a turn answers with — what the harness charged, how long it
/// took, what it said on the way — and a long-running host logs that rather
/// than losing it. It is also how a caller sees a run this port refused to
/// record.
pub trait RunReportObserver: Send + Sync {
    /// One finished run's report, exactly as `OneHarness` answered it.
    fn answered(&self, report: &RunReport);
}

/// Where a caller watches what a turn does: the requests this port builds, the
/// processes `OneHarness` creates under them, and the reports it answers.
///
/// Both halves are the supervision a long-running host wants anyway — the
/// harness tree is what a watchdog reaps — and both are what a test observes a
/// turn through, so no test needs a double of the port to see either.
#[derive(Clone, Default)]
pub struct TurnSeam {
    /// Handed every run request this port builds, before `OneHarness` receives it.
    pub requests: Option<Arc<dyn RunRequestObserver>>,
    /// Handed every harness process `OneHarness` creates under the run.
    pub processes: Option<Arc<dyn ProcessSupervisor + Send + Sync>>,
    /// Handed the report of every run, as `OneHarness` answered it.
    pub reports: Option<Arc<dyn RunReportObserver>>,
}

impl core::fmt::Debug for TurnSeam {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("TurnSeam")
            .field("requests", &self.requests.is_some())
            .field("processes", &self.processes.is_some())
            .field("reports", &self.reports.is_some())
            .finish()
    }
}
