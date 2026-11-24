//! The port itself: one supervision turn, and the closing of a session.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::JoinHandle;

use oneharness_core::domain::harness::HarnessIdentity as EngineIdentity;
use oneharness_core::domain::lean::ToolClass;
use oneharness_core::domain::mode::PermissionMode;
use oneharness_core::domain::report::{RunReport, RunResult, SessionReport, Status};
use oneharness_core::domain::session::SessionPhase as HarnessPhase;
use oneharness_core::errors::OneharnessError;
use oneharness_core::io::cancel::CancelToken;
use oneharness_core::io::compact::{CompactRequest, compact_session};
use oneharness_core::io::run::{RunControls, RunOutcome, RunRequest, run_supervised};
use oneharness_core::io::runner::ProcessSupervisor;
use printobserver_supervisor_api::AgentAssessment;
use printobserver_supervisor_api::{
    BoxFuture, CREDENTIAL_ENV, SERVER_ENV, SupervisorError, SupervisorPort, TurnAccess,
    TurnOutcome, TurnPass, TurnRequest, TurnSession,
};
use printobserver_supervisor_api::{SessionPhase, SupervisionSession};
use printobserver_types::serde_json::Value;
use printobserver_types::{PrintId, Reported, Timestamp};

use crate::config::{AgentCommand, HarnessIdentity, SupervisorConfig, TurnSeam};
use crate::ledger::{
    CallUsage, CompactionEnd, PrintLedger, RecordedCompaction, RecordedTurn, SessionName,
};
use crate::prompt::PromptTemplate;
use crate::skill::skill_prose;

/// The directory under the state directory `OneHarness` keeps its own session
/// store in. Naming it here is what makes a restart continue a conversation:
/// the store is beside this port's ledger rather than in a platform default
/// that a differently-configured process would not find.
pub const HARNESS_SESSIONS_DIRECTORY: &str = "harness-sessions";

/// The harness whose turns are granted a shell narrowed to the agent's
/// commands. Another harness keeps `OneHarness`'s read-only mode, which gives it
/// no shell at all.
const CLAUDE_CODE: &str = "claude-code";

/// The built-in tool classes every turn loads, and there is no other: the shell
/// the agent's commands run in and file reading. `OneHarness` translates them
/// for whichever harness runs the turn.
pub const TURN_TOOL_SET: [ToolClass; 2] = [ToolClass::Shell, ToolClass::Read];

/// The Claude Code tool of [`TURN_TOOL_SET`] that is allowed outright. The
/// shell is allowed only for the configured commands.
const CLAUDE_READ_TOOL: &str = "Read";

/// What follows the skill's prose in every run's system text: how the agent
/// uses its tools, which is all a replaced harness prompt told it that a turn
/// still needs. It is this crate's own committed asset, so the system text is
/// two files a person reads.
pub const TOOL_FRAMING: &str = include_str!("../assets/tool-framing.md");

/// What an answer of `stop` that stopped nothing is refused with: the answer
/// acts on nothing, and the command that stops a print is named.
pub const UNACTED_STOP: &str = "Refused: your answer's disposition is stop, but an answer stops \
nothing, and this print is still printing: this turn has asked for no pause. Run `printobserver \
pause` for it now, as its usage above shows, and answer again; or answer with the disposition \
you meant.";

/// How many characters of an answer that failed validation are kept with the
/// failure: enough to read what the agent said, bounded so a runaway answer
/// cannot swell the record it lands in.
pub const ANSWER_EXCERPT_CHARS: usize = 2000;

/// What follows an excerpt that was cut short.
pub const ANSWER_CUT: &str = " …";

/// What joins the reason an answer was refused to what the agent said.
pub const ANSWER_SAID: &str = "; the agent answered: ";

/// The supervising agent, reached through `OneHarness` in this process.
#[derive(Debug)]
pub struct OneharnessSupervisor {
    /// How the agent is reached and where its sessions live.
    config: SupervisorConfig,
    /// Where a caller watches what a turn does.
    seam: TurnSeam,
    /// The `PrintObserver` skill's prose, read from the configured path when
    /// this port was built and with its frontmatter split off.
    skill: String,
    /// The committed prompt template, read from the tree when this port was
    /// built.
    template: PromptTemplate,
    /// The idle compaction running for each print, while one runs. Held only
    /// to cancel it: what a compaction did is written to the print's ledger.
    compacting: Mutex<HashMap<PrintId, Compacting>>,
}

/// One idle compaction under way: what cancels it, what says it has ended,
/// and the thread that writes its end down once its harness process has
/// exited.
#[derive(Debug)]
struct Compacting {
    /// Cancels the compaction's harness process.
    cancel: CancelToken,
    /// Where a scheduled check waits for its end.
    ending: Arc<Ending>,
    /// The thread running it, which returns once its end is written down.
    thread: JoinHandle<()>,
}

/// Whether an idle compaction has ended, and since when a scheduled check has
/// waited for it.
#[derive(Debug, Default)]
struct Ending {
    /// What has happened so far.
    state: Mutex<EndingState>,
    /// Signalled once the compaction's end is written down.
    ended: Condvar,
}

/// What has happened to an idle compaction so far.
#[derive(Debug, Default)]
struct EndingState {
    /// When a scheduled check began to wait for it, if one has.
    waited_since: Option<std::time::Instant>,
    /// Whether its end is written down.
    ended: bool,
}

/// How much longer than a compaction's own timeout a scheduled check waits for
/// it: the margin for writing its end down once its harness process is gone.
const ENDING_MARGIN: std::time::Duration = std::time::Duration::from_secs(30);

/// `value` as a turn's prompt shows it: compact JSON with what the agent never
/// reads taken out.
///
/// Every call of the session re-reads every check's prompt, so a field no
/// judgement or command uses is paid for again on each of them. Out are an
/// image's content digest, which identifies stored bytes the agent opens by
/// path, and an event's `raw` body, the producer's own undecoded copy of the
/// payload beside it; a reading inside its range is shown as its value alone,
/// and one outside it keeps its flag.
fn shown<T: serde::Serialize>(value: &T, what: &str) -> Result<String, SupervisorError> {
    let unavailable = |error: serde_json::Error| SupervisorError::Unavailable {
        detail: format!("{what} cannot be written down: {error}"),
    };
    let mut value = serde_json::to_value(value).map_err(unavailable)?;
    if let Value::Object(fields) = &mut value {
        fields.remove("raw");
    }
    trimmed(&mut value);
    serde_json::to_string(&value).map_err(unavailable)
}

/// Take every content digest out of `value`, at every depth, and show each
/// in-range reading as its value.
///
/// A reading is whatever [`Reported`]'s own deserializer reads as one, which
/// refuses an object carrying any other field, so the shape is that type's
/// rather than restated here.
fn trimmed(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            fields.remove("sha256");
            for field in fields.values_mut() {
                trimmed(field);
                let in_range = field
                    .is_object()
                    .then(|| serde_json::from_value::<Reported<f64>>(field.clone()).ok())
                    .flatten()
                    .filter(|reading| !reading.out_of_range());
                if let Some(reading) = in_range {
                    *field = Value::from(reading.value());
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(trimmed),
        _ => {}
    }
}

impl OneharnessSupervisor {
    /// Build the port, reading the skill and the prompt template from the tree.
    ///
    /// # Errors
    ///
    /// Returns [`SupervisorError::Unavailable`] when either file cannot be read,
    /// when the skill opens a frontmatter block it never closes, or when the
    /// template does not declare the six slots a turn fills.
    pub fn open(config: SupervisorConfig) -> Result<Self, SupervisorError> {
        Self::observed(config, TurnSeam::default())
    }

    /// Build the port with a caller watching its requests and its processes.
    ///
    /// # Errors
    ///
    /// The same as [`OneharnessSupervisor::open`].
    pub fn observed(config: SupervisorConfig, seam: TurnSeam) -> Result<Self, SupervisorError> {
        let skill = skill_prose(&read(&config.skill_path)?)
            .map_err(|error| SupervisorError::Unavailable {
                detail: format!("{}: {error}", config.skill_path.display()),
            })?
            .to_owned();
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
            compacting: Mutex::new(HashMap::new()),
        })
    }

    /// Every idle compaction recorded for one print, in order, read from the
    /// state directory.
    ///
    /// # Errors
    ///
    /// Returns [`SupervisorError::Unavailable`] when the ledger cannot be read.
    pub fn recorded_compactions(
        &self,
        print_id: &PrintId,
    ) -> Result<Vec<RecordedCompaction>, SupervisorError> {
        Ok(self.ledger(print_id)?.compactions().to_vec())
    }

    /// Compact a print's conversation once, now, as its idle compaction does
    /// once a turn leaves it at the threshold, whatever its size: the session
    /// its last turn ran in, measured as that turn left it. Answers the
    /// compaction as it is written down, however it ended.
    ///
    /// A measurement, for a host whose conversations end below the threshold:
    /// what one compaction reads and leaves is otherwise never observed there.
    /// It runs the idle compaction's own code, so what it measures is that.
    ///
    /// # Errors
    ///
    /// Returns [`SupervisorError::Unavailable`] when the ledger cannot be read
    /// or records no turn with calls to compact after.
    pub fn compact_once(&self, print_id: &PrintId) -> Result<RecordedCompaction, SupervisorError> {
        let ledger = self.ledger(print_id)?;
        let (session, size) = ledger
            .turns()
            .iter()
            .rev()
            .find_map(|turn| {
                let size = turn.calls.last().and_then(CallUsage::conversation_tokens)?;
                Some((turn.session_name.clone(), size))
            })
            .ok_or_else(|| SupervisorError::Unavailable {
                detail: format!("print {print_id} records no turn with calls to compact after"),
            })?;
        Ok(IdleCompaction {
            config: self.config.clone(),
            seam: self.seam.clone(),
            print_id: *print_id,
            session,
            size,
            cancel: CancelToken::new(),
            ending: Arc::new(Ending::default()),
        }
        .run())
    }

    /// Wait, while an idle compaction of one print's session runs, until it
    /// ends, and at most its own timeout and [`ENDING_MARGIN`]; answer at once
    /// when none runs. The wait is written down with the compaction's end.
    fn wait_for_compaction(&self, print_id: &PrintId) {
        let ending = self
            .compacting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(print_id)
            .map(|running| Arc::clone(&running.ending));
        let Some(ending) = ending else {
            return;
        };
        let bound =
            std::time::Duration::from_secs(self.config.turn_timeout.seconds()) + ENDING_MARGIN;
        let mut state = ending.state.lock().unwrap_or_else(PoisonError::into_inner);
        state
            .waited_since
            .get_or_insert_with(std::time::Instant::now);
        // However it ends — completed, cancelled, failed or timed out — the
        // check runs then, and past the bound it runs regardless.
        let _ = ending
            .ended
            .wait_timeout_while(state, bound, |state| !state.ended)
            .unwrap_or_else(PoisonError::into_inner);
    }

    /// End the idle compaction of one print's session, if one is running: it
    /// is cancelled, and this returns once its harness process has exited and
    /// its end is written down. A session is never compacted while a turn of
    /// it runs, and a turn never waits for a compaction to finish.
    fn settle(&self, print_id: &PrintId) {
        let running = self
            .compacting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(print_id);
        if let Some(running) = running {
            running.cancel.cancel();
            // A compaction thread that panicked wrote nothing down; the turn
            // goes on in the session as it was.
            let _ = running.thread.join();
        }
    }

    /// Compact a print's session in the background when the turn that just
    /// ended left its conversation at or above the idle threshold.
    ///
    /// A conversation's size is its last model call's whole input and output,
    /// which is what the next call re-reads. A cancelled compaction leaves the
    /// conversation as large as it was, so the next turn's end starts it again.
    fn compact_when_due(&self, print_id: &PrintId, session: &SessionName, calls: &[CallUsage]) {
        let Some(threshold) = self.config.idle_compact_tokens else {
            return;
        };
        let Some(size) = calls.last().and_then(CallUsage::conversation_tokens) else {
            return;
        };
        if size < threshold.get() {
            return;
        }
        let cancel = CancelToken::new();
        let ending = Arc::new(Ending::default());
        let compaction = IdleCompaction {
            config: self.config.clone(),
            seam: self.seam.clone(),
            print_id: *print_id,
            session: session.clone(),
            size,
            cancel: cancel.clone(),
            ending: Arc::clone(&ending),
        };
        // A thread of this process, not a child process: the compaction's own
        // harness is started by `OneHarness`, as a turn's is.
        let thread = std::thread::spawn(move || {
            compaction.run();
        });
        self.compacting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                *print_id,
                Compacting {
                    cancel,
                    ending,
                    thread,
                },
            );
    }

    /// Run one prompt in a fresh session through exactly the run request a turn
    /// is built as — the skill, the lean settings, the permissions, the schema
    /// and the configured harness — with no image, no pass and no print: what a
    /// call of this configuration costs before any check adds to it.
    ///
    /// The session is named `session` and kept beside the turns' sessions, so a
    /// probe never continues a print's conversation.
    ///
    /// # Errors
    ///
    /// Returns [`SupervisorError::Unavailable`] when `OneHarness` refuses the run,
    /// answers anything other than one result, or answers one the harness did
    /// not complete.
    pub fn probe(&self, session: &str, prompt: &str) -> Result<RunResult, SupervisorError> {
        self.probe_with_image(session, prompt, None)
    }

    /// [`probe`](Self::probe) with empty system text, which still replaces the
    /// harness's own prompt: what the harness itself sends beside a turn's
    /// prompt, its tool definitions and whatever it frames them with, apart
    /// from the skill this port sends. A text
    /// probe's first call less this one's is what the skill and the tool
    /// framing add to every call.
    ///
    /// # Errors
    ///
    /// As [`probe`](Self::probe)'s.
    pub fn probe_without_system(
        &self,
        session: &str,
        prompt: &str,
    ) -> Result<RunResult, SupervisorError> {
        self.probe_run(session, prompt, None, String::new())
    }

    /// [`probe`](Self::probe), with `image` attached to the prompt as a turn's
    /// check image is: what one check image adds to a call is this run's first
    /// call less a text-only probe's.
    ///
    /// # Errors
    ///
    /// As [`probe`](Self::probe)'s.
    pub fn probe_with_image(
        &self,
        session: &str,
        prompt: &str,
        image: Option<PathBuf>,
    ) -> Result<RunResult, SupervisorError> {
        self.probe_run(session, prompt, image, self.system_text())
    }

    /// One probe run, built as a turn's request is and with the turn's own
    /// prompt replaced: only `system` differs between the probes, so the
    /// difference between two of their first calls is that text and nothing
    /// the request carries besides.
    fn probe_run(
        &self,
        session: &str,
        prompt: &str,
        image: Option<PathBuf>,
        system: String,
    ) -> Result<RunResult, SupervisorError> {
        let name = SessionName::probe(session)?;
        let handed_image = image.is_some();
        let request = RunRequest {
            system: Some(system),
            ..self.build_request(
                &name,
                prompt,
                image.into_iter().collect(),
                handed_image,
                None,
            )
        };
        // A probe is no supervision turn, so it carries no compaction setting.
        let request = RunRequest {
            auto_compact_tokens: None,
            ..request
        };
        let outcome = self.run(&request).map_err(|error| unavailable(&error))?;
        let [result] = <[RunResult; 1]>::try_from(outcome.report.results).map_err(|results| {
            SupervisorError::Unavailable {
                detail: format!("the probe answered {} results, not one", results.len()),
            }
        })?;
        // A run the harness did not complete measured nothing worth reporting.
        if result.status != Status::Ok || result.failure_kind.is_some() {
            return Err(SupervisorError::Unavailable {
                detail: format!(
                    "the probe's run did not complete ({:?}): {}",
                    result.status,
                    result.error.as_deref().unwrap_or("no detail")
                ),
            });
        }
        Ok(result)
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

    /// The prompt for one turn in one session: the committed template with its
    /// six slots filled, and nothing else.
    ///
    /// Built for the session the turn runs in, because the actor the agent
    /// names itself by is that session's: an agent left to guess the shape of
    /// its own actor is refused by every request it guesses wrong.
    fn prompt_for(
        &self,
        request: &TurnRequest,
        session: &SessionName,
    ) -> Result<String, SupervisorError> {
        let event = shown(&request.event, "the triggering event")?;
        let situation = shown(&request.situation, "the turn's situation")?;
        let facts = shown(&request.facts, "the print's facts")?;
        let actor = serde_json::json!({ "agent": { "session_name": session.as_str() } });
        Ok(self.template.fill(
            &event,
            &situation,
            &facts,
            &request.context_command,
            &actor.to_string(),
            &self.commands_a_turn_runs(request.image_path.is_some()),
        ))
    }

    /// The agent commands a turn may run: every configured one, less those
    /// withheld beside an image when the turn is handed one.
    ///
    /// The one place both the turn's permissions and the commands its prompt
    /// lists are chosen from, so a command one of them withholds the other
    /// cannot name.
    fn agent_commands(&self, handed_image: bool) -> impl Iterator<Item = &AgentCommand> {
        self.config.agent_commands.iter().filter(move |command| {
            !(handed_image && self.config.withheld_beside_an_image.contains(command))
        })
    }

    /// The usage line of each command a turn may run, one per line, in the
    /// order the program prints them.
    fn commands_a_turn_runs(&self, handed_image: bool) -> String {
        let commands: Vec<&AgentCommand> = self.agent_commands(handed_image).collect();
        self.config
            .command_usage
            .iter()
            .filter(|line| {
                commands.iter().any(|command| {
                    let prefix = command.as_str();
                    line.as_str() == prefix || line.starts_with(&format!("{prefix} "))
                })
            })
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Whether the configured harness is Claude Code.
    fn on_claude_code(&self) -> bool {
        self.config
            .harness
            .as_str()
            .parse::<EngineIdentity>()
            .is_ok_and(|identity| identity.base() == CLAUDE_CODE)
    }

    /// The system text every run is sent, replacing the harness's own system
    /// prompt: the skill's prose, then [`TOOL_FRAMING`].
    fn system_text(&self) -> String {
        format!("{}\n\n{TOOL_FRAMING}", self.skill)
    }

    /// How the harness is permitted to act during a turn, and the arguments that
    /// narrow it.
    ///
    /// With no agent commands configured, or on a harness other than Claude
    /// Code, a turn is `OneHarness`'s read-only mode: the agent reads files and
    /// has no shell. A turn that has agent commands runs Claude Code in
    /// `dontAsk` mode instead — what [`PermissionMode::Default`] maps to — which
    /// refuses any tool call no rule allows and carries on. Its rules allow file
    /// reading outright and the shell only for a command beginning with one of
    /// the configured prefixes. Which tools load at all is the run request's
    /// [`TURN_TOOL_SET`], not an argument here.
    ///
    /// A turn handed an image is allowed no command withheld beside one, on
    /// every run it makes: the answer asked for again is the same turn's.
    fn permissions(&self, handed_image: bool) -> (PermissionMode, Vec<String>) {
        if !self.on_claude_code() || self.config.agent_commands.is_empty() {
            return (PermissionMode::ReadOnly, Vec::new());
        }
        // llmlint: ignore[least_privilege_grants] suppressions.toml has the reason.
        let mut arguments = vec!["--allowedTools".to_owned(), CLAUDE_READ_TOOL.to_owned()];
        arguments.extend(
            self.agent_commands(handed_image)
                // llmlint: ignore[least_privilege_grants] suppressions.toml has the reason.
                .map(|command| format!("Bash({command}:*)")),
        );
        (PermissionMode::Default, arguments)
    }

    /// The run request for one turn in one session, attaching `images` to its
    /// prompt.
    ///
    /// Every turn is lean: it loads [`TURN_TOOL_SET`] alone, no skills list and
    /// no MCP server, because each call re-sends whatever a turn loads. Its
    /// system text replaces the harness's own system prompt rather than being
    /// appended to it, for the same reason.
    ///
    /// The answer is held to the assessment schema by `OneHarness` itself, on
    /// every harness: it hands the harness the schema natively, beside the
    /// image, and asks again in the same session for an answer that does not
    /// conform. This port asks for no schema in its prompt and re-asks for
    /// none.
    ///
    /// The run's environment carries the configured assignments and then the
    /// pass this run was issued: the server's address and the credential minted
    /// for this run, which is the only way a turn is handed either. A
    /// configured assignment of either variable is left out, so the pass is the
    /// one value of each the run sees. No file holding the credential is
    /// written, and the context command names none.
    fn build_request(
        &self,
        session: &SessionName,
        prompt: &str,
        images: Vec<PathBuf>,
        handed_image: bool,
        pass: Option<&TurnPass>,
    ) -> RunRequest {
        let (mode, passthrough) = self.permissions(handed_image);
        let mut env: Vec<String> = self
            .config
            .harness_env
            .iter()
            .map(ToString::to_string)
            .filter(|assigned| {
                ![SERVER_ENV, CREDENTIAL_ENV]
                    .iter()
                    .any(|name| assigned.starts_with(&format!("{name}=")))
            })
            .collect();
        env.extend(
            pass.into_iter()
                .flat_map(TurnPass::environment)
                .map(|(name, value)| format!("{name}={value}")),
        );
        RunRequest {
            harness: vec![self.config.harness.to_string()],
            prompt: vec![prompt.to_owned()],
            model: self.config.model.iter().map(ToString::to_string).collect(),
            system: Some(self.system_text()),
            replace_system: Some(true),
            session: Some(session.to_string()),
            session_dir: Some(self.config.state_dir.join(HARNESS_SESSIONS_DIRECTORY)),
            schema: Some(self.config.assessment_schema.path().to_path_buf()),
            timeout: Some(self.config.turn_timeout.seconds()),
            auto_compact_tokens: self.config.auto_compact_tokens,
            cwd: Some(self.config.working_dir.clone()),
            env,
            mode: Some(mode),
            passthrough,
            tool_set: Some(TURN_TOOL_SET.to_vec()),
            load_skills: Some(false),
            load_mcp: Some(false),
            images,
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

    /// Hand one run request to `OneHarness`, in this process, carrying the
    /// pass `access` issues for the session it runs in.
    fn drive(
        &self,
        session: &SessionName,
        prompt: &str,
        images: &[PathBuf],
        handed_image: bool,
        access: &dyn TurnAccess,
    ) -> Result<Result<RunOutcome, OneharnessError>, SupervisorError> {
        let pass = access.issue(&TurnSession::new(session.as_str())?)?;
        let request =
            self.build_request(session, prompt, images.to_vec(), handed_image, Some(&pass));
        Ok(self.run(&request))
    }

    /// Run one request, in this process, through the caller's seam.
    fn run(&self, request: &RunRequest) -> Result<RunOutcome, OneharnessError> {
        if let Some(observer) = &self.seam.requests {
            observer.built(request);
        }
        let supervisor: Option<&dyn ProcessSupervisor> = self
            .seam
            .processes
            .as_ref()
            .map(|processes| Arc::as_ref(processes) as &dyn ProcessSupervisor);
        let outcome = run_supervised(request, RunControls::default(), supervisor)?;
        if let Some(observer) = &self.seam.reports {
            observer.answered(&outcome.report);
        }
        Ok(outcome)
    }

    /// One supervision turn, from the ledger through the run and back.
    fn take_turn(
        &self,
        request: &TurnRequest,
        access: &dyn TurnAccess,
    ) -> Result<TurnOutcome, SupervisorError> {
        let print_id = &request.print_id;
        self.settle(print_id);
        let started_at = Timestamp::now();
        let mut ledger = self.ledger(print_id)?;
        let mut session = ledger.session_for_next_turn();
        let images: Vec<PathBuf> = request.image_path.iter().cloned().collect();
        let handed_image = !images.is_empty();

        let outcome = match self.drive(
            &session,
            &self.prompt_for(request, &session)?,
            &images,
            handed_image,
            access,
        )? {
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
                // The prompt is built again, because the actor it hands the
                // agent names the session the turn now runs in — and so is the
                // credential, which is bound to that session.
                self.drive(
                    &session,
                    &self.prompt_for(request, &session)?,
                    &images,
                    handed_image,
                    access,
                )?
                .map_err(|error| unavailable(&error))?
            }
            Err(error) => return Err(unavailable(&error)),
        };

        let mut reported = TurnReport::of(outcome.report, &session, &self.config.harness)?;
        if self.on_claude_code() {
            self.held_to_its_stop(request, &mut reported, access);
        }
        self.record(&mut ledger, print_id, &reported, started_at)
    }

    /// Hold an answer of `stop` to a stop asked for: an answer records a
    /// judgement and acts on nothing, so `stop` on a print that was printing,
    /// from a turn that asked for no pause or cancel, is refused once in the
    /// same session — naming the command that stops a print — and the answer
    /// to that refusal, which `OneHarness` holds to the schema as it held the
    /// first, is the turn's.
    /// Whatever it then answers stands; the supervisor writes down a stop that
    /// still stopped nothing.
    fn held_to_its_stop(
        &self,
        request: &TurnRequest,
        reported: &mut TurnReport,
        access: &dyn TurnAccess,
    ) {
        let says_stop = reported
            .result
            .structured
            .as_ref()
            .and_then(|answer| answer.get("disposition"))
            .and_then(Value::as_str)
            == Some("stop");
        let printing =
            request.situation.printer_state == Some(printobserver_types::PrinterState::Printing);
        if !says_stop || !printing || access.asked_to_stop() || reported.result.status != Status::Ok
        {
            return;
        }
        let earlier = reported.result.calls.take().unwrap_or_default();
        let handed_image = request.image_path.is_some();
        match self.asked_again(&reported.name, UNACTED_STOP, handed_image, access) {
            Ok(answered) => {
                reported.result = answered;
                let mut calls = earlier;
                calls.extend(reported.result.calls.take().into_iter().flatten());
                reported.result.calls = Some(calls);
            }
            // A run that could not ask again leaves the answer it had, which
            // the supervisor records as a stop that stopped nothing.
            Err(_) => reported.result.calls = Some(earlier),
        }
    }

    /// One run asking again in the turn's session, and the result it answered.
    fn asked_again(
        &self,
        session: &SessionName,
        again: &str,
        handed_image: bool,
        access: &dyn TurnAccess,
    ) -> Result<RunResult, SupervisorError> {
        let outcome = self
            .drive(session, again, &[], handed_image, access)?
            .map_err(|error| unavailable(&error))?;
        Ok(TurnReport::of(outcome.report, session, &self.config.harness)?.result)
    }

    /// Write down what a finished run did, and answer the caller.
    fn record(
        &self,
        ledger: &mut PrintLedger,
        print_id: &PrintId,
        reported: &TurnReport,
        started_at: Timestamp,
    ) -> Result<TurnOutcome, SupervisorError> {
        let at = Timestamp::now();
        let session = ledger.record_session(
            &reported.name,
            &reported.result.harness_id,
            reported.phase(),
            at,
        );
        let answer = assessment(&reported.result);
        let failure = answer.as_ref().err().map(ToString::to_string);
        let calls: Vec<CallUsage> = reported
            .result
            .calls
            .iter()
            .flatten()
            .map(CallUsage::of)
            .collect();
        ledger.record_turn(
            &reported.name,
            (started_at, at),
            failure.as_deref(),
            calls.clone(),
        );
        self.save(ledger, print_id)?;
        self.compact_when_due(print_id, &reported.name, &calls);

        Ok(TurnOutcome {
            session,
            phase: reported.phase(),
            assessment: answer?,
        })
    }
}

impl Drop for OneharnessSupervisor {
    /// A port going away leaves no compaction running behind it.
    fn drop(&mut self) {
        let running: Vec<PrintId> = self
            .compacting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .keys()
            .copied()
            .collect();
        for print_id in running {
            self.settle(&print_id);
        }
    }
}

/// One idle compaction of a print's session, owning everything it needs so it
/// runs on a thread of its own while the port takes other prints' turns.
struct IdleCompaction {
    /// How the agent is reached and where its sessions live.
    config: SupervisorConfig,
    /// Where a caller watches the processes it creates.
    seam: TurnSeam,
    /// The print whose session it compacts.
    print_id: PrintId,
    /// The session it compacts.
    session: SessionName,
    /// The conversation's size when it was started.
    size: u64,
    /// What an event for the print cancels it through.
    cancel: CancelToken,
    /// Where a scheduled check waits for its end.
    ending: Arc<Ending>,
}

impl IdleCompaction {
    /// Compact the session through `OneHarness`, write down how it ended, and
    /// answer that.
    fn run(self) -> RecordedCompaction {
        let started_at = Timestamp::now();
        let (ended, reported_before, reported_after, usage, error) = match self.compact() {
            Ok(outcome) => (
                CompactionEnd::of(outcome.status),
                outcome.context_tokens_before,
                outcome.context_tokens_after,
                CallUsage::of(&outcome.usage),
                outcome.error,
            ),
            Err(error) => (
                CompactionEnd::Failed,
                None,
                None,
                CallUsage::default(),
                Some(error),
            ),
        };
        // Held while the end is written down, so a scheduled check that began
        // to wait is either counted here or finds it ended.
        let mut state = self
            .ending
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let compaction = RecordedCompaction {
            session_name: self.session.clone(),
            started_at,
            ended_at: Timestamp::now(),
            ended,
            size_before: self.size,
            reported_before,
            reported_after,
            usage,
            error,
            scheduled_check_waited_ms: state
                .waited_since
                .map(|since| u64::try_from(since.elapsed().as_millis()).unwrap_or(u64::MAX)),
        };
        // A ledger that cannot be read or written loses this record alone; the
        // next turn reads the same ledger and is refused naming why.
        if let Ok(mut ledger) = PrintLedger::read(&self.config.state_dir, &self.print_id) {
            ledger.record_compaction(compaction.clone());
            let _ = ledger.write(&self.config.state_dir);
        }
        state.ended = true;
        drop(state);
        self.ending.ended.notify_all();
        compaction
    }

    /// The compaction itself, in the session store and the working directory
    /// a turn of the session runs with, under the harness's own environment.
    fn compact(&self) -> Result<oneharness_core::domain::compact::CompactOutcome, String> {
        let harness = self
            .config
            .harness
            .as_str()
            .parse::<EngineIdentity>()
            .map_err(|error| error.to_string())?;
        let session = oneharness_core::domain::sdk::NonEmptyString::try_from(self.session.as_str())
            .map_err(|error| error.to_string())?;
        let request = CompactRequest {
            harness: Some(harness),
            session,
            session_dir: Some(self.config.state_dir.join(HARNESS_SESSIONS_DIRECTORY)),
            cwd: Some(self.config.working_dir.clone()),
            // No pass: a compaction runs no command of this program.
            env: self
                .config
                .harness_env
                .iter()
                .map(ToString::to_string)
                .filter(|assigned| {
                    ![SERVER_ENV, CREDENTIAL_ENV]
                        .iter()
                        .any(|name| assigned.starts_with(&format!("{name}=")))
                })
                .collect(),
            model: self.config.model.as_ref().map(ToString::to_string),
            bin: self
                .config
                .harness_bin
                .iter()
                .map(|path| format!("{}={}", self.config.harness, path.display()))
                .collect(),
            config: Vec::new(),
            no_config: true,
            timeout: Some(self.config.turn_timeout.seconds()),
        };
        let supervisor: Option<&dyn ProcessSupervisor> = self
            .seam
            .processes
            .as_ref()
            .map(|processes| Arc::as_ref(processes) as &dyn ProcessSupervisor);
        compact_session(
            &request,
            RunControls {
                cancel: self.cancel.clone(),
                ..RunControls::default()
            },
            supervisor,
        )
        .map_err(|error| error.to_string())
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
    /// The session the turn asked about, which the block above agrees with.
    name: SessionName,
}

impl TurnReport {
    /// Narrow one finished run's report to the turn it reports, given the
    /// session and the harness identity that turn asked for.
    ///
    /// Both are checked rather than read: what goes into the ledger is what a
    /// restart continues this print's conversation from, so a report about
    /// another session, or from another identity, is one this print's history
    /// must not absorb — however it came to be answered.
    ///
    /// # Errors
    ///
    /// Returns [`SupervisorError::Unavailable`] when the run answered no
    /// session block, no result at all, more than one result, or a session or
    /// identity other than the one asked for.
    pub fn of(
        report: RunReport,
        asked_session: &SessionName,
        asked_harness: &HarnessIdentity,
    ) -> Result<Self, SupervisorError> {
        let Some(session) = report.session else {
            return Err(SupervisorError::Unavailable {
                detail: NO_SESSION.to_owned(),
            });
        };
        if session.name != asked_session.as_str() {
            return Err(SupervisorError::Unavailable {
                detail: format!(
                    "the harness answered about session `{}`, and this turn asked about `{asked_session}`",
                    session.name
                ),
            });
        }
        let [result] = <[RunResult; 1]>::try_from(report.results).map_err(|results| {
            SupervisorError::Unavailable {
                detail: if results.is_empty() {
                    NO_RESULT.to_owned()
                } else {
                    format!("{}, and this turn asked one to answer once", results.len())
                },
            }
        })?;
        if result.harness_id != asked_harness.as_str() {
            return Err(SupervisorError::Unavailable {
                detail: format!(
                    "`{}` answered this turn, and it was asked of `{asked_harness}`",
                    result.harness_id
                ),
            });
        }
        Ok(Self {
            result,
            session,
            name: asked_session.clone(),
        })
    }

    /// The session block the run answered with.
    #[must_use]
    pub fn session(&self) -> &SessionReport {
        &self.session
    }

    /// The session the turn ran in.
    #[must_use]
    pub fn name(&self) -> &SessionName {
        &self.name
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

/// Why an answer was refused, followed by what the agent actually said.
///
/// The reason alone ("no JSON value could be extracted") names the symptom
/// rather than the cause. The agent's own words usually name the cause — a
/// command it was refused, a sign-in the harness is missing — and nothing else
/// keeps them.
fn with_answer(why: String, text: Option<&str>) -> String {
    let Some(said) = text.map(str::trim).filter(|said| !said.is_empty()) else {
        return why;
    };
    let mut kept = said.char_indices().map(|(at, _)| at);
    match kept.nth(ANSWER_EXCERPT_CHARS) {
        Some(cut) => format!("{why}{ANSWER_SAID}{}{ANSWER_CUT}", &said[..cut]),
        None => format!("{why}{ANSWER_SAID}{said}"),
    }
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
        let why = result
            .schema_error
            .clone()
            .unwrap_or_else(|| NO_ANSWER.to_owned());
        return Err(SupervisorError::InvalidAnswer {
            detail: with_answer(why, result.text.as_deref()),
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
    fn idle_work_ended(&self, print_id: PrintId) -> BoxFuture<'_, Result<(), SupervisorError>> {
        Box::pin(async move {
            self.wait_for_compaction(&print_id);
            Ok(())
        })
    }

    fn run_turn(
        &self,
        request: TurnRequest,
        access: Arc<dyn TurnAccess>,
    ) -> BoxFuture<'_, Result<TurnOutcome, SupervisorError>> {
        Box::pin(async move { self.take_turn(&request, access.as_ref()) })
    }

    fn close_session(
        &self,
        print_id: PrintId,
        close_reason: String,
    ) -> BoxFuture<'_, Result<(), SupervisorError>> {
        Box::pin(async move {
            self.settle(&print_id);
            let mut ledger = self.ledger(&print_id)?;
            ledger.close_current(&close_reason, Timestamp::now());
            self.save(&ledger, &print_id)
        })
    }
}
