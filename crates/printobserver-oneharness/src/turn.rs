//! The port itself: one supervision turn, and the closing of a session.

use std::path::Path;
use std::sync::Arc;

use oneharness_core::domain::mode::PermissionMode;
use oneharness_core::domain::report::{RunResult, SessionReport, Status};
use oneharness_core::domain::session::SessionPhase as HarnessPhase;
use oneharness_core::errors::OneharnessError;
use oneharness_core::io::run::{RunControls, RunOutcome, RunRequest, run_supervised};
use oneharness_core::io::runner::ProcessSupervisor;
use printobserver_supervisor_api::{
    BoxFuture, SupervisorError, SupervisorPort, TurnOutcome, TurnRequest,
};
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
            harness: vec![self.config.harness.clone()],
            prompt: vec![prompt.to_owned()],
            model: self.config.model.clone().into_iter().collect(),
            system: Some(self.skill.clone()),
            session: Some(session.to_owned()),
            session_dir: Some(self.config.state_dir.join(HARNESS_SESSIONS_DIRECTORY)),
            schema: Some(self.config.assessment_schema_path.clone()),
            timeout: Some(self.config.turn_timeout_s),
            cwd: Some(self.config.working_dir.clone()),
            env: self.config.harness_env.clone(),
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
        run_supervised(&request, RunControls::default(), supervisor)
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

        self.record(&mut ledger, print_id, &session, outcome)
    }

    /// Write down what a finished run did, and answer the caller.
    fn record(
        &self,
        ledger: &mut PrintLedger,
        print_id: &PrintId,
        planned: &str,
        outcome: RunOutcome,
    ) -> Result<TurnOutcome, SupervisorError> {
        let at = Timestamp::now();
        let result = outcome.report.results.into_iter().next();
        let (Some(result), Some(reported)) = (result, outcome.report.session) else {
            ledger.record_turn(planned, at, Some(NO_SESSION));
            self.save(ledger, print_id)?;
            return Err(SupervisorError::Unavailable {
                detail: NO_SESSION.to_owned(),
            });
        };

        let session = ledger.record_session(
            &reported.name,
            &result.harness_id,
            reported.phase == HarnessPhase::Create,
            at,
        );
        let answer = assessment(&result);
        let failure = answer.as_ref().err().map(ToString::to_string);
        ledger.record_turn(&session.session_name, at, failure.as_deref());
        self.save(ledger, print_id)?;

        Ok(TurnOutcome {
            session,
            phase: phase_of(&reported),
            assessment: answer?,
        })
    }
}

/// The detail a run that reported no session handle is recorded under.
const NO_SESSION: &str = "the harness exposed no session, so the conversation cannot be continued";

/// Read a file the port is built from.
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

/// Whether the run opened the session or continued it, in this system's words.
fn phase_of(reported: &SessionReport) -> SessionPhase {
    match reported.phase {
        HarnessPhase::Create => SessionPhase::Created,
        HarnessPhase::Continue => SessionPhase::Continued,
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
            detail: result.schema_error.clone().unwrap_or_else(|| {
                "the answer carried no value the assessment schema could be applied to".to_owned()
            }),
        });
    }
    let Some(value) = result.structured.clone() else {
        return Err(SupervisorError::InvalidAnswer {
            detail: "the answer carried no JSON value".to_owned(),
        });
    };
    serde_json::from_value(value).map_err(|error| SupervisorError::InvalidAnswer {
        detail: error.to_string(),
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
