//! The port itself: one supervision turn, and the closing of a session.

use std::path::Path;
use std::sync::Arc;

use oneharness_core::domain::mode::PermissionMode;
use oneharness_core::domain::report::{RunReport, RunResult, SessionReport, Status};
use oneharness_core::domain::session::SessionPhase as HarnessPhase;
use oneharness_core::errors::OneharnessError;
use oneharness_core::io::run::{RunControls, RunOutcome, RunRequest, run_supervised};
use oneharness_core::io::runner::ProcessSupervisor;
use printobserver_supervisor_api::{
    BoxFuture, SupervisorError, SupervisorPort, TurnOutcome, TurnRequest,
};
use printobserver_types::serde_json::Value;
use printobserver_types::{AgentAssessment, PrintId, SessionPhase, SupervisionSession, Timestamp};

use crate::config::{SupervisorConfig, TurnSeam};
use crate::ledger::{PrintLedger, RecordedTurn};
use crate::prompt::{NO_IMAGE, PromptTemplate};

/// The directory under the state directory `OneHarness` keeps its own session
/// store in. Naming it here is what makes a restart continue a conversation:
/// the store is beside this port's ledger rather than in a platform default
/// that a differently-configured process would not find.
pub const HARNESS_SESSIONS_DIRECTORY: &str = "harness-sessions";

/// The supervising agent, reached through `OneHarness` in this process.
#[derive(Debug)]
pub struct OneharnessSupervisor {
    /// How the agent is reached and where its sessions live.
    config: SupervisorConfig,
    /// Where a caller watches what a turn does.
    seam: TurnSeam,
    /// The `PrintObserver` skill, read from the tree when this port was built.
    skill: String,
    /// The committed prompt template, read from the tree when this port was
    /// built.
    template: PromptTemplate,
}

impl OneharnessSupervisor {
    /// Build the port, reading the skill and the prompt template from the tree.
    ///
    /// # Errors
    ///
    /// Returns [`SupervisorError::Unavailable`] when either file cannot be read
    /// or the template does not declare the three slots a turn fills.
    pub fn open(config: SupervisorConfig) -> Result<Self, SupervisorError> {
        Self::observed(config, TurnSeam::default())
    }

    /// Build the port with a caller watching its requests and its processes.
    ///
    /// # Errors
    ///
    /// The same as [`OneharnessSupervisor::open`].
    pub fn observed(config: SupervisorConfig, seam: TurnSeam) -> Result<Self, SupervisorError> {
        let skill = read(&config.skill_path)?;
        let template =
            PromptTemplate::parse(&read(&config.prompt_template_path)?).map_err(|error| {
                SupervisorError::Unavailable {
                    detail: format!("{}: {error}", config.prompt_template_path.display()),
                }
            })?;
        Ok(Self {
            config,
            seam,
            skill,
            template,
        })
    }

    /// Every session opened for one print, in order, read from the state
    /// directory.
    ///
    /// # Errors
    ///
    /// Returns [`SupervisorError::Unavailable`] when the ledger cannot be read.
    pub fn recorded_sessions(
        &self,
        print_id: &PrintId,
    ) -> Result<Vec<SupervisionSession>, SupervisorError> {
        Ok(self.ledger(print_id)?.sessions().to_vec())
    }

    /// Every turn recorded for one print, in order, read from the state
    /// directory.
    ///
    /// # Errors
    ///
    /// Returns [`SupervisorError::Unavailable`] when the ledger cannot be read.
    pub fn recorded_turns(&self, print_id: &PrintId) -> Result<Vec<RecordedTurn>, SupervisorError> {
        Ok(self.ledger(print_id)?.turns().to_vec())
    }

    /// One print's ledger.
    fn ledger(&self, print_id: &PrintId) -> Result<PrintLedger, SupervisorError> {
        PrintLedger::read(&self.config.state_dir, print_id).map_err(|error| {
            SupervisorError::Unavailable {
                detail: format!(
                    "the supervision ledger of print {print_id} is unreadable: {error}"
                ),
            }
        })
    }

    /// Write one print's ledger back.
    fn save(&self, ledger: &PrintLedger, print_id: &PrintId) -> Result<(), SupervisorError> {
        ledger
            .write(&self.config.state_dir)
            .map_err(|error| SupervisorError::Unavailable {
                detail: format!(
                    "the supervision ledger of print {print_id} could not be written: {error}"
                ),
            })
    }

    /// The prompt for one turn: the committed template with its three slots
    /// filled, and nothing else.
    fn prompt_for(&self, request: &TurnRequest) -> Result<String, SupervisorError> {
        let event = serde_json::to_string_pretty(&request.event).map_err(|error| {
            SupervisorError::Unavailable {
                detail: format!("the triggering event cannot be written down: {error}"),
            }
        })?;
        let image = request
            .image_path
            .as_ref()
            .map_or_else(|| NO_IMAGE.to_owned(), |path| path.display().to_string());
        Ok(self.template.fill(&event, &image, &request.context_command))
    }

    /// The run request for one turn in one session.
    fn build_request(&self, session: &str, prompt: &str) -> RunRequest {
        RunRequest {
            harness: vec![self.config.harness.to_string()],
            prompt: vec![prompt.to_owned()],
            model: self.config.model.iter().map(ToString::to_string).collect(),
            system: Some(self.skill.clone()),
            session: Some(session.to_owned()),
            session_dir: Some(self.config.state_dir.join(HARNESS_SESSIONS_DIRECTORY)),
            schema: Some(self.config.assessment_schema.path().to_path_buf()),
            timeout: Some(self.config.turn_timeout.seconds()),
            cwd: Some(self.config.working_dir.clone()),
            env: self
                .config
                .harness_env
                .iter()
                .map(ToString::to_string)
                .collect(),
            mode: Some(PermissionMode::ReadOnly),
            // The supervisor's turns are decided here, not by whatever
            // configuration happens to be on the host it runs on.
            no_config: true,
            bin: self
                .config
                .harness_bin
                .iter()
                .map(|path| format!("{}={}", self.config.harness, path.display()))
                .collect(),
            ..RunRequest::default()
        }
    }

    /// Hand one run request to `OneHarness`, in this process.
    fn drive(&self, session: &str, prompt: &str) -> Result<RunOutcome, OneharnessError> {
        let request = self.build_request(session, prompt);
        if let Some(observer) = &self.seam.requests {
            observer.built(&request);
        }
        let supervisor: Option<&dyn ProcessSupervisor> = self
            .seam
            .processes
            .as_ref()
            .map(|processes| Arc::as_ref(processes) as &dyn ProcessSupervisor);
        let outcome = run_supervised(&request, RunControls::default(), supervisor)?;
        if let Some(observer) = &self.seam.reports {
            observer.answered(&outcome.report);
        }
        Ok(outcome)
    }

    /// One supervision turn, from the ledger through the run and back.
    fn take_turn(&self, request: &TurnRequest) -> Result<TurnOutcome, SupervisorError> {
        let print_id = &request.print_id;
        let prompt = self.prompt_for(request)?;
        let mut ledger = self.ledger(print_id)?;
        let mut session = ledger.session_for_next_turn();

        let outcome = match self.drive(&session, &prompt) {
            Ok(outcome) => outcome,
            // The harness binds a session to the identity that created it and
            // refuses to continue it on another. That is a session that has
            // ended, not a turn the caller has to handle: it is closed carrying
            // the harness's own reason, and the turn runs in a new one.
            Err(OneharnessError::SessionHarnessConflict { was, now, .. }) => {
                let reason = format!(
                    "the harness refused to continue this session on `{now}`: \
                     it was created on `{was}`"
                );
                ledger.close_current(&reason, Timestamp::now());
                session = ledger.name_after_current();
                self.save(&ledger, print_id)?;
                self.drive(&session, &prompt)
                    .map_err(|error| unavailable(&error))?
            }
            Err(error) => return Err(unavailable(&error)),
        };

        self.record(&mut ledger, print_id, &TurnReport::of(outcome.report)?)
    }

    /// Write down what a finished run did, and answer the caller.
    fn record(
        &self,
        ledger: &mut PrintLedger,
        print_id: &PrintId,
        reported: &TurnReport,
    ) -> Result<TurnOutcome, SupervisorError> {
        let at = Timestamp::now();
        let session = ledger.record_session(
            &reported.session.name,
            &reported.result.harness_id,
            reported.session.phase == HarnessPhase::Create,
            at,
        );
        let answer = assessment(&reported.result);
        let failure = answer.as_ref().err().map(ToString::to_string);
        ledger.record_turn(&session.session_name, at, failure.as_deref());
        self.save(ledger, print_id)?;

        Ok(TurnOutcome {
            session,
            phase: reported.phase(),
            assessment: answer?,
        })
    }
}

/// What a finished run answered, narrowed to what a turn is written down from:
/// the one result the run produced, and the session block naming the
/// conversation it ran in.
///
/// `OneHarness`'s report is the answer to every shape of run it serves — a run
/// under no session handle, whose session block is then absent; a fan-out over
/// several harnesses or models, whose results are then several. A turn is one
/// harness answering one prompt under one session, so the narrowing happens
/// once, here, and every step after it holds exactly one of each rather than a
/// shape nobody downstream can act on.
///
/// A report of any other shape is refused rather than written down. It answers
/// a run this port did not ask for, so nothing in it can be attributed to this
/// print's conversation: the session such a turn would be recorded under is one
/// this port planned and the harness never confirmed, and a ledger carrying
/// turns of a session that was never opened is a history that reads as though
/// the agent had been consulted.
#[derive(Debug, Clone)]
pub struct TurnReport {
    /// The result the run produced.
    result: RunResult,
    /// The session block the run answered with.
    session: SessionReport,
}

impl TurnReport {
    /// Narrow one finished run's report to the turn it reports.
    ///
    /// # Errors
    ///
    /// Returns [`SupervisorError::Unavailable`] when the run answered no
    /// session block, or no result at all.
    pub fn of(report: RunReport) -> Result<Self, SupervisorError> {
        let Some(session) = report.session else {
            return Err(SupervisorError::Unavailable {
                detail: NO_SESSION.to_owned(),
            });
        };
        let [result] = <[RunResult; 1]>::try_from(report.results).map_err(|results| {
            SupervisorError::Unavailable {
                detail: if results.is_empty() {
                    NO_RESULT.to_owned()
                } else {
                    format!("{}, and this turn asked one to answer once", results.len())
                },
            }
        })?;
        Ok(Self { result, session })
    }

    /// The session block the run answered with.
    #[must_use]
    pub fn session(&self) -> &SessionReport {
        &self.session
    }

    /// Whether the run opened the session or continued it, in this system's
    /// words.
    #[must_use]
    pub fn phase(&self) -> SessionPhase {
        match self.session.phase {
            HarnessPhase::Create => SessionPhase::Created,
            HarnessPhase::Continue => SessionPhase::Continued,
        }
    }
}

/// The detail an answer no schema verdict was reached about is refused with.
const NO_ANSWER: &str = "the answer carried no value the assessment schema could be applied to";

/// The detail a run that answered no session block is refused with.
const NO_SESSION: &str = "the harness exposed no session, so the conversation cannot be continued";

/// The detail a run that answered no result at all is refused with. A run that
/// answered more than one says how many instead: it is a different shape of run
/// rather than one that did not happen.
const NO_RESULT: &str = "the harness answered no result, so no turn was taken";

fn read(path: &Path) -> Result<String, SupervisorError> {
    std::fs::read_to_string(path).map_err(|error| SupervisorError::Unavailable {
        detail: format!("{} is unreadable: {error}", path.display()),
    })
}

/// Whatever `OneHarness` refused a run for, as this port's own error.
fn unavailable(error: &OneharnessError) -> SupervisorError {
    SupervisorError::Unavailable {
        detail: error.to_string(),
    }
}

/// The assessment one result carries, or why it carries none.
///
/// The answer has already been through `OneHarness`'s own validate-and-re-prompt
/// loop by the time it arrives here, so a result that still does not conform is
/// one no further attempt would fix.
fn assessment(result: &RunResult) -> Result<AgentAssessment, SupervisorError> {
    if result.status == Status::Timeout {
        return Err(SupervisorError::TimedOut);
    }
    if let Some(kind) = &result.failure_kind {
        return Err(SupervisorError::Unavailable {
            detail: format!(
                "the harness failed ({kind:?}): {}",
                result.error.as_deref().unwrap_or("no detail")
            ),
        });
    }
    if result.status != Status::Ok && result.status != Status::Nonzero {
        return Err(SupervisorError::Unavailable {
            detail: format!(
                "the harness did not run the turn ({:?}): {}",
                result.status,
                result.error.as_deref().unwrap_or("no detail")
            ),
        });
    }
    if result.schema_valid != Some(true) {
        return Err(SupervisorError::InvalidAnswer {
            detail: result.schema_error.clone().unwrap_or(NO_ANSWER.to_owned()),
        });
    }
    // The value is read back as the assessment itself rather than trusted for
    // having passed the schema: the two agree only while the schema is the one
    // generated from the type, and a schema saying otherwise is a bad answer
    // rather than a value this port hands on.
    serde_json::from_value(result.structured.clone().unwrap_or(Value::Null)).map_err(|error| {
        SupervisorError::InvalidAnswer {
            detail: error.to_string(),
        }
    })
}

impl SupervisorPort for OneharnessSupervisor {
    fn run_turn(
        &self,
        request: TurnRequest,
    ) -> BoxFuture<'_, Result<TurnOutcome, SupervisorError>> {
        Box::pin(async move { self.take_turn(&request) })
    }

    fn close_session(
        &self,
        print_id: PrintId,
        close_reason: String,
    ) -> BoxFuture<'_, Result<(), SupervisorError>> {
        Box::pin(async move {
            let mut ledger = self.ledger(&print_id)?;
            ledger.close_current(&close_reason, Timestamp::now());
            self.save(&ledger, &print_id)
        })
    }
}
