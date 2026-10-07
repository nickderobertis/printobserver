//! The composition root: the one place an implementation crate is named.
//!
//! Everything below this file is written against the three ports and the
//! supervision domain's store traits. This is where one implementation is
//! chosen for each — `OctoPrint` behind the printer port, `Obico` behind the
//! vision port, `OneHarness` behind the supervisor port, and `SQLite` behind
//! the store traits — and `just check-repo` refuses any other crate that names
//! one. The store is one implementation held once per aggregate: [`Stores::of`]
//! coerces the one `SQLite` store to each of the five trait objects, and every
//! consumer below is handed the handle for the aggregate it names and no
//! other.
//!
//! [`Server::start`] is that root. [`Server::start_with`] takes ports a caller
//! composed, which is how the tiers that stand in for a machine drive the real
//! surface without one; the two share every line after the ports exist, so what
//! a tier drives is the server this file starts.

use std::ffi::OsStr;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use axum::routing::post;
use printobserver_core::store::Stores;
use printobserver_core::{Clock as _, CoreConfig, Supervisor, SystemClock};
use printobserver_obico::{ObicoVision, ObicoVisionConfig};
use printobserver_octoprint::OctoPrintPrinter;
use printobserver_oneharness::{
    AgentCommand, AssessmentSchema, EnvAssignment, HarnessSignIn, OneharnessSupervisor,
    SupervisorConfig,
};
use printobserver_printer_api::{PrinterError, PrinterPort};
use printobserver_store_sqlite::SqliteStore;
use printobserver_supervisor_api::SupervisorPort;
use tokio::net::TcpListener;
use tokio::sync::{oneshot, watch};
use tokio::task::JoinHandle;

use crate::api::{ApiState, router};
use crate::config::{ApiCredential, ConfigError, ConfigField, CredentialVerifier, ServerConfig};
use crate::ingress::{IngressState, receive};
use crate::operations::{INGRESS_PATH, OPERATIONS, command_for};
use crate::reconcile::{ReconcileStores, Reconciliation, overdue, reconcile};
use crate::turns::TurnCredentials;

/// The program a supervision turn runs to read its print's context.
pub const CONTEXT_PROGRAM: &str = "printobserver";

/// The file, under the state directory, a server before this one wrote the
/// address it took and the credential in force into. A start deletes it: a
/// supervision turn runs as the service's user and could read the operator's
/// credential out of it.
pub const CLIENT_CONFIG_FILE: &str = "client.toml";

/// The file, under the state directory, a server before this one generated its
/// API credential into, as plaintext. A start converts it into
/// [`API_CREDENTIAL_VERIFIER_FILE`] and deletes it, so the credential it held
/// keeps authenticating while nothing the service's user can read holds it.
pub const API_CREDENTIAL_FILE: &str = "api-credential";

/// The file, under the state directory, holding the verifier of the operator's
/// credential — read when `api.credential_verifier` is not configured.
pub const API_CREDENTIAL_VERIFIER_FILE: &str = "api-credential.verifier";

/// The command that gives an operator a credential, as every refusal and
/// warning about one names it.
pub const CREDENTIAL_ISSUE: &str = "printobserver credential issue";

/// The command that prints the verifier of a credential an operator already
/// holds.
pub const CREDENTIAL_VERIFIER: &str = "printobserver credential verifier";

/// The mode the verifier file is created with.
///
/// Unix alone has one. A file Windows creates carries the access its directory
/// grants rather than a mode, so there the state directory is what keeps it to
/// the service's own account.
#[cfg(unix)]
const PRIVATE: u32 = 0o600;

/// The command a supervision turn runs to read its print's context.
///
/// This program's own context read, against the print the turn is about, and
/// nothing else: where the server is and the credential the turn answers to
/// reach the turn through its environment alone — the address the server
/// bound and a credential minted for that turn — so the command names no
/// configuration file and the program reads none.
/// `printobserver-oneharness` substitutes the print for the placeholder.
#[must_use]
pub fn context_command() -> String {
    format!("{CONTEXT_PROGRAM} context --print-id {{print_id}}")
}

/// Who an operator's credential is checked against, as one start settled it.
#[derive(Debug, Clone)]
pub enum OperatorCredential {
    /// Its verifier, from `api.credential_verifier` or the state directory.
    Verifier(CredentialVerifier),
    /// A plaintext `api.credential` the configuration still carries, admitted
    /// until it is replaced.
    Plaintext(ApiCredential),
    /// Nothing at all: a fresh install whose operator has not run
    /// `printobserver credential issue` yet. No operator request is admitted.
    Unconfigured,
}

impl OperatorCredential {
    /// Whether a presented credential is the operator's.
    #[must_use]
    pub fn admits(&self, presented: &[u8]) -> bool {
        match self {
            Self::Verifier(verifier) => verifier.admits(presented),
            Self::Plaintext(credential) => credential.admits(presented),
            Self::Unconfigured => false,
        }
    }
}

/// The warning every start under a plaintext `api.credential` gives, which
/// names the key, its replacement and the command that computes one, and never
/// what the key holds.
#[must_use]
pub fn plaintext_credential_warning() -> String {
    format!(
        "the server configuration carries `api.credential`, an operator credential in \
         plaintext. A supervision turn runs as this service's user and can read that file, \
         so it can read that credential until it is replaced. Replace it with \
         `api.credential_verifier`: pipe the credential into `{CREDENTIAL_VERIFIER}` to print \
         that line, put it in the configuration, delete `api.credential`, and restart the \
         service"
    )
}

/// Write one file this start creates, exclusively and private to the service.
fn write_private(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, PRIVATE);
    let mut file = options.open(path)?;
    let written = file.write_all(contents).and_then(|()| file.sync_all());
    if written.is_err() {
        // The file is this start's own and holds nothing usable, so it is
        // taken back rather than left for the next start to refuse.
        let _ = std::fs::remove_file(path);
    }
    written
}

/// Remove one file a server before this one left, when it is there.
fn remove_if_there(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// Take the plaintext a server before this one left in the state directory out
/// of it.
///
/// `client.toml` is deleted. `api-credential` is converted into the verifier
/// file — written first, exclusively, so a start that stops between the two
/// leaves the credential still convertible — and then deleted; one left beside
/// a verifier file that is already there is deleted outright, because that
/// verifier is what is in force.
///
/// # Errors
///
/// Returns [`StartError::Credential`] naming the file, and never what it holds,
/// when the legacy credential cannot be read, holds nothing a header could
/// present, or cannot be converted or removed; and [`StartError::State`] when
/// `client.toml` cannot be removed.
fn take_plaintext_out(state_dir: &Path) -> Result<(), StartError> {
    let client_config = state_dir.join(CLIENT_CONFIG_FILE);
    remove_if_there(&client_config).map_err(|error| StartError::State {
        detail: format!(
            "{} could not be removed, and it may carry the operator's credential where a \
             supervision turn can read it: {error}",
            client_config.display()
        ),
    })?;
    let legacy = state_dir.join(API_CREDENTIAL_FILE);
    let refusing = |detail: String| StartError::Credential {
        path: legacy.clone(),
        detail,
    };
    let held = match std::fs::read(&legacy) {
        Ok(held) => held,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(refusing(format!("it cannot be read: {error}"))),
    };
    let verifier_file = state_dir.join(API_CREDENTIAL_VERIFIER_FILE);
    if !verifier_file.exists() {
        let text = core::str::from_utf8(&held).ok().map(without_terminator);
        let credential = text
            .ok_or("it is not text")
            .and_then(ApiCredential::new)
            .map_err(|why| {
                refusing(format!(
                    "{why}. This server converts the credential an earlier version generated \
                     there into its verifier: correct the file, or remove it and run \
                     `{CREDENTIAL_ISSUE} --replace`"
                ))
            })?;
        write_private(
            &verifier_file,
            format!("{}\n", credential.verifier()).as_bytes(),
        )
        .map_err(|error| {
            refusing(format!(
                "its verifier could not be written to {}: {error}",
                verifier_file.display()
            ))
        })?;
    }
    std::fs::remove_file(&legacy)
        .map_err(|error| refusing(format!("it could not be removed: {error}")))
}

/// The operator credential this start serves under, and the warnings it owes.
///
/// The legacy plaintext is taken out of the state directory first. Then
/// `api.credential_verifier` when it is configured, a plaintext
/// `api.credential` when that is all the configuration carries, the verifier
/// file in the state directory, and otherwise nothing — a fresh install, which
/// supervises and admits no operator until one is issued. A plaintext
/// `api.credential` warns whether or not a verifier wins over it.
///
/// # Errors
///
/// Returns [`StartError::Credential`] naming the file, and never what it
/// holds, when the legacy plaintext cannot be taken out or the verifier file
/// cannot be read as one.
pub fn operator_credential(
    config: &ServerConfig,
) -> Result<(OperatorCredential, Vec<String>), StartError> {
    take_plaintext_out(&config.state_dir)?;
    let mut warnings = Vec::new();
    if config.api_credential.is_some() {
        warnings.push(plaintext_credential_warning());
    }
    if let Some(verifier) = &config.api_credential_verifier {
        return Ok((OperatorCredential::Verifier(verifier.clone()), warnings));
    }
    if let Some(plaintext) = &config.api_credential {
        return Ok((OperatorCredential::Plaintext(plaintext.clone()), warnings));
    }
    let path = config.state_dir.join(API_CREDENTIAL_VERIFIER_FILE);
    let held = match std::fs::read_to_string(&path) {
        Ok(held) => held,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((OperatorCredential::Unconfigured, warnings));
        }
        Err(error) => {
            return Err(StartError::Credential {
                path,
                detail: format!("it cannot be read: {error}"),
            });
        }
    };
    let verifier =
        CredentialVerifier::parse(held.trim()).map_err(|why| StartError::Credential {
            path: path.clone(),
            detail: format!(
                "{why}. Correct the file, or remove it and set `api.credential_verifier`"
            ),
        })?;
    Ok((OperatorCredential::Verifier(verifier), warnings))
}

/// A credential file's text with the one line terminator a person's editor or
/// shell ends it with set aside.
///
/// Exactly one, `\n` or `\r\n`: that terminator is not part of the credential,
/// and anything else in the text — a second one included — is left for the
/// character rule to refuse.
fn without_terminator(text: &str) -> &str {
    text.strip_suffix("\r\n")
        .or_else(|| text.strip_suffix('\n'))
        .unwrap_or(text)
}

/// The file name the agent's prompt template is materialized under.
pub const PROMPT_FILE: &str = "turn-prompt.md";

/// The file name the assessment schema is materialized under.
pub const SCHEMA_FILE: &str = "assessment-schema.json";

/// The prompt template this program carries and materializes.
///
/// The adapter's own committed asset rather than a copy of it, so what an
/// installed program fills a turn with and what `just check-repo` reads are one
/// file.
pub const TURN_PROMPT: &str = printobserver_oneharness::DEFAULT_TURN_PROMPT;

/// The four implementations one running server was composed from.
///
/// A tier that stands in for a machine hands these in; [`Server::start`] builds
/// them from the configuration.
pub struct Ports {
    /// The printer.
    pub printer: Arc<dyn PrinterPort>,
    /// Durable state, one handle per aggregate onto one store.
    pub stores: Stores,
    /// External observations, and the snapshot fetch.
    pub vision: Arc<ObicoVision>,
    /// The supervising agent's harness.
    pub agent: Arc<dyn SupervisorPort>,
}

impl core::fmt::Debug for Ports {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.debug_struct("Ports").finish_non_exhaustive()
    }
}

/// Why this server did not start.
#[derive(Debug)]
pub enum StartError {
    /// The configuration will not do, naming the field that will not.
    Configuration(ConfigError),
    /// The state directory could not be prepared.
    State {
        /// What went wrong.
        detail: String,
    },
    /// A credential file in the state directory cannot be used.
    Credential {
        /// The file.
        path: PathBuf,
        /// Why it cannot be used, in words that never quote what it holds.
        detail: String,
    },
    /// The address could not be listened on.
    Listen {
        /// The address that was asked for.
        address: SocketAddr,
        /// What went wrong.
        detail: String,
    },
    /// The store could not be opened.
    Store {
        /// What the store said.
        detail: String,
    },
    /// The supervising agent's harness could not be reached at all.
    Supervisor {
        /// What the harness said.
        detail: String,
    },
    /// Reconciling what the store held failed.
    Reconciliation {
        /// What went wrong.
        detail: String,
    },
}

impl StartError {
    /// The configuration field this refusal is about, when it is about one.
    #[must_use]
    pub const fn field(&self) -> Option<ConfigField> {
        match self {
            Self::Configuration(error) => error.field(),
            _ => None,
        }
    }
}

impl core::fmt::Display for StartError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Configuration(error) => write!(formatter, "{error}"),
            Self::State { detail } => {
                write!(
                    formatter,
                    "the state directory could not be prepared: {detail}"
                )
            }
            Self::Credential { path, detail } => write!(
                formatter,
                "the credential file {} cannot be used: {detail}",
                path.display()
            ),
            Self::Listen { address, detail } => {
                write!(formatter, "{address} could not be listened on: {detail}")
            }
            Self::Store { detail } => write!(formatter, "the store could not be opened: {detail}"),
            Self::Supervisor { detail } => write!(
                formatter,
                "the supervising agent's harness could not be reached: {detail}"
            ),
            Self::Reconciliation { detail } => write!(
                formatter,
                "what the store held could not be reconciled: {detail}"
            ),
        }
    }
}

impl core::error::Error for StartError {}

impl From<ConfigError> for StartError {
    fn from(error: ConfigError) -> Self {
        Self::Configuration(error)
    }
}

/// The composition root.
#[derive(Debug)]
pub struct Server;

impl Server {
    /// Read one configuration file, compose the four implementations, and serve.
    ///
    /// # Errors
    ///
    /// Returns [`StartError::Configuration`] naming the field that will not do
    /// — including the two only the machine can rule on, an `OctoPrint` address
    /// nothing answers at and a key that instance refuses — and the other
    /// variants for a host that will not let this server come up.
    pub async fn start(config_path: impl AsRef<Path>) -> Result<Running, StartError> {
        let config = ServerConfig::load(config_path)?;
        let stores = Stores::of(Arc::new(SqliteStore::open(&config.state_dir).map_err(
            |error| StartError::Store {
                detail: error.to_string(),
            },
        )?));
        let printer = Arc::new(OctoPrintPrinter::new(config.octoprint.clone()));
        probe(printer.as_ref()).await?;
        let vision = Arc::new(
            ObicoVision::new(ObicoVisionConfig::default())
                .map_err(|error| StartError::State {
                    detail: error.to_string(),
                })?
                .with_api(config.obico_api.clone()),
        );
        let agent = Arc::new(agent_for(&config)?);
        Self::start_with(
            config,
            Ports {
                printer,
                stores,
                vision,
                agent,
            },
        )
        .await
    }

    /// Serve over ports a caller composed.
    ///
    /// # Errors
    ///
    /// The same as [`Server::start`], less the refusals that belong to building
    /// the implementations.
    pub async fn start_with(config: ServerConfig, ports: Ports) -> Result<Running, StartError> {
        // The operator's credential is settled before anything listens, so
        // there is no moment at which this server answers a request it has
        // nothing to check against — and the plaintext an earlier version left
        // is out of the state directory before the first turn could read it.
        let (operator, warnings) = operator_credential(&config)?;
        // The listener is taken next, because the command a supervision turn
        // runs to read its context names the address this server is answering
        // on — and a configuration may ask for any free port.
        let listener =
            TcpListener::bind(config.listen)
                .await
                .map_err(|error| StartError::Listen {
                    address: config.listen,
                    detail: error.to_string(),
                })?;
        let address = listener.local_addr().map_err(|error| StartError::Listen {
            address: config.listen,
            detail: error.to_string(),
        })?;
        let clock = Arc::new(SystemClock);
        let due = overdue(&ports.stores.actions, clock.now())
            .await
            .map_err(|error| StartError::Reconciliation {
                detail: error.to_string(),
            })?;
        let mut core_config = CoreConfig::new(config.safety.clone(), context_command());
        core_config
            .camera_snapshot_url
            .clone_from(&config.camera_snapshot_url);
        let supervisor = Supervisor::new(
            core_config,
            Arc::clone(&ports.printer),
            ports.stores.clone(),
            Arc::clone(&ports.vision) as Arc<dyn printobserver_vision_api::VisionPort>,
            Arc::clone(&ports.agent),
            clock,
        );
        // Every turn's runs are told the address this server bound, beside the
        // credential minted for them.
        let turns = TurnCredentials::new(Some(address));
        supervisor.install_turn_authority(Arc::new(turns.clone()));
        let reconciliation = reconcile(
            &supervisor,
            &ReconcileStores {
                prints: Arc::clone(&ports.stores.prints),
                sessions: Arc::clone(&ports.stores.sessions),
                events: Arc::clone(&ports.stores.events),
            },
            due,
        )
        .await
        .map_err(|error| StartError::Reconciliation {
            detail: error.to_string(),
        })?;

        let ingress = IngressState::start(
            Arc::clone(&supervisor),
            Arc::clone(&ports.stores.events),
            Arc::clone(&ports.vision),
            config.ingress_shared_secret.clone(),
            config.ingress_answer_bound,
        );
        let completions = ingress.completions();
        let application = router(ApiState {
            supervisor: Arc::clone(&supervisor),
            prints: Arc::clone(&ports.stores.prints),
            events: Arc::clone(&ports.stores.events),
            images: Arc::clone(&ports.stores.images),
            sessions: Arc::clone(&ports.stores.sessions),
            operator: Arc::new(operator),
            turns,
        })
        .merge(
            Router::new()
                .route(INGRESS_PATH, post(receive))
                .with_state(ingress),
        );

        let (stop, stopped) = oneshot::channel::<()>();
        let serving = tokio::spawn(async move {
            let _ = axum::serve(listener, application)
                .with_graceful_shutdown(async move {
                    let _ = stopped.await;
                })
                .await;
        });
        Ok(Running {
            address,
            config,
            warnings,
            supervisor,
            stores: ports.stores,
            reconciliation,
            completions,
            stop: Some(stop),
            serving,
        })
    }
}

/// Ask the machine whether the address and the key are the ones it answers to.
///
/// Only the two answers that are *about the configuration* refuse a start. A
/// printer that is off, in an error state, or answering something this adapter
/// cannot read is a machine having a bad day rather than a file with the wrong
/// value in it, and a supervisor that would not come up for one of those could
/// not be used to find out why.
async fn probe(printer: &dyn PrinterPort) -> Result<(), StartError> {
    match printer.snapshot().await {
        Err(PrinterError::Unreachable { detail }) => {
            Err(StartError::Configuration(ConfigError::about(
                ConfigField::OctoprintUrl,
                format!("nothing answers there: {detail}"),
            )))
        }
        Err(PrinterError::Unauthorized { detail }) => {
            Err(StartError::Configuration(ConfigError::about(
                ConfigField::OctoprintApiKey,
                format!("the OctoPrint instance refused it: {detail}"),
            )))
        }
        _ => Ok(()),
    }
}

/// Write one of the agent's committed assets into the state directory.
///
/// The port reads its template and its schema from paths, so that editing one
/// on a running host is a restart rather than a rebuild. An installed program
/// has no checkout to read them out of, so the bytes it carries are written
/// here — and an operator who configured a template of their own is pointed at
/// theirs instead.
fn materialize(
    directory: &Path,
    name: &str,
    contents: &str,
) -> Result<std::path::PathBuf, StartError> {
    let path = directory.join(name);
    std::fs::write(&path, contents).map_err(|error| StartError::State {
        detail: format!("{} could not be written: {error}", path.display()),
    })?;
    Ok(path)
}

/// The directory the harness runs in: the one holding the configured skill.
///
/// An installed skill is a directory — its `SKILL.md` and the `reference/`
/// documents that file links to by relative paths — so the agent stands where
/// those links resolve. A skill named by a bare file name sits in this
/// process's own working directory.
fn beside_the_skill(skill_path: &Path) -> std::path::PathBuf {
    match skill_path.parent() {
        Some(directory) if !directory.as_os_str().is_empty() => directory.to_path_buf(),
        _ => std::path::PathBuf::from("."),
    }
}

/// The supervising agent's harness, over the configured skill and the assets
/// this program carries.
fn agent_for(config: &ServerConfig) -> Result<OneharnessSupervisor, StartError> {
    OneharnessSupervisor::open(agent_config(config)?).map_err(|error| StartError::Supervisor {
        detail: error.to_string(),
    })
}

/// The supervising agent's configuration, as [`Server::start`] composes it.
///
/// The template and the assessment schema are written into the assets
/// directory; the skill is the installed one `supervisor.skill_path` names, and
/// every turn runs in the directory holding it — where an installed skill's
/// `reference/` documents sit, so the links the skill carries resolve from
/// where the agent stands. The harness is the one `supervisor.harness` names,
/// found by name, and a harness this program can sign in is pointed at the
/// sign-in kept under the state directory.
///
/// # Errors
///
/// Returns [`StartError::State`] when the assets or the sign-in directory
/// cannot be written, and [`StartError::Supervisor`] when the schema it wrote
/// constrains nothing.
pub fn agent_config(config: &ServerConfig) -> Result<SupervisorConfig, StartError> {
    let assets = config.assets_dir();
    std::fs::create_dir_all(&assets).map_err(|error| StartError::State {
        detail: format!("{} could not be created: {error}", assets.display()),
    })?;
    let prompt_template_path = match &config.prompt_template_path {
        Some(path) => path.clone(),
        None => materialize(&assets, PROMPT_FILE, TURN_PROMPT)?,
    };
    let schema_path =
        materialize(
            &assets,
            SCHEMA_FILE,
            &printobserver_types::serde_json::to_string_pretty(&without_dialect(
                printobserver_types::contract::schema_of::<
                    printobserver_supervisor_api::AgentAssessment,
                >(),
            ))
            .map_err(|error| StartError::State {
                detail: error.to_string(),
            })?,
        )?;
    let assessment_schema =
        AssessmentSchema::at(&schema_path).map_err(|error| StartError::Supervisor {
            detail: error.to_string(),
        })?;
    // A harness this program can sign in keeps that sign-in under the state
    // directory — the one place the service's unit lets it write — and every
    // turn is pointed at the directory `printobserver sign-in` wrote it to. A
    // harness outside the table is handed nothing extra.
    let mut harness_env = match HarnessSignIn::of(&config.harness) {
        Some(sign_in) => {
            let directory =
                sign_in
                    .prepare(&config.state_dir)
                    .map_err(|error| StartError::State {
                        detail: format!(
                            "{} could not be created: {error}",
                            sign_in.directory(&config.state_dir).display()
                        ),
                    })?;
            vec![
                sign_in
                    .assignment(&directory)
                    .map_err(|error| StartError::Supervisor {
                        detail: error.to_string(),
                    })?,
            ]
        }
        None => Vec::new(),
    };
    let search_path = std::env::var_os("PATH").unwrap_or_default();
    harness_env.extend(path_led_by_this_program(&search_path)?);
    Ok(SupervisorConfig {
        state_dir: config.state_dir.clone(),
        skill_path: config.skill_path.clone(),
        prompt_template_path,
        assessment_schema,
        harness: config.harness.clone(),
        model: config.model.clone(),
        // The skill's own directory rather than the state or assets directory,
        // so that the skill's relative links to the reference documents beside
        // it resolve from where the agent is standing as well as from the skill.
        working_dir: beside_the_skill(&config.skill_path),
        turn_timeout: printobserver_oneharness::TurnTimeout::DEFAULT,
        // Found by name, unless npm's launcher is all a search finds: then the
        // native program behind it, which `OneHarness` can start where the
        // launcher cannot be.
        harness_bin: HarnessSignIn::of(&config.harness)
            .and_then(|harness| harness.program_behind_launcher(&search_path)),
        harness_env,
        agent_commands: agent_commands()?,
    })
}

/// The key a JSON Schema declares its dialect under.
const DIALECT_KEY: &str = "$schema";

/// A schema with its dialect declaration taken off, and nothing else changed.
///
/// The generated assessment schema declares draft 2020-12, and Claude Code
/// refuses a `--json-schema` naming a dialect its validator was not given ("no
/// schema with key or ref `https://json-schema.org/draft/2020-12/schema`"), so
/// every turn ended before the agent was asked anything. Left undeclared, the
/// harness reads the schema in its own dialect and `OneHarness` in its default,
/// and the keywords this schema uses mean the same in both. The checked-in
/// schema keeps its declaration: only the copy a turn is handed drops it.
fn without_dialect(
    mut schema: printobserver_types::serde_json::Value,
) -> printobserver_types::serde_json::Value {
    if let Some(object) = schema.as_object_mut() {
        object.remove(DIALECT_KEY);
    }
    schema
}

/// The commands the supervising agent may run: this program's own, one per
/// operation the server serves, and no other.
///
/// Every one of them is a request to this server, so what the agent can change
/// is still exactly what the policy grants its actor. Derived from
/// [`OPERATIONS`] through the command-line program's own spelling, so an
/// operation the server gains is a command the agent may run from the same
/// change.
fn agent_commands() -> Result<Vec<AgentCommand>, StartError> {
    OPERATIONS
        .iter()
        .map(|operation| {
            AgentCommand::new(&format!(
                "{CONTEXT_PROGRAM} {}",
                command_for(operation.name)
            ))
        })
        .collect::<Result<_, _>>()
        .map_err(|error| StartError::Supervisor {
            detail: error.to_string(),
        })
}

/// A turn's `PATH`: the directory this program is running from, then the
/// search path this process was given.
///
/// The context command, and every command the agent may run, names this
/// program by name rather than by path, and nothing makes that name resolve in
/// a service's environment: neither installer puts the program's directory on
/// a `PATH`, and Windows' service control manager hands a service the
/// environment it had at boot. Leading the turn's `PATH` with this program's
/// own directory makes the name the program that is serving.
///
/// Nothing is added when the running program's path cannot be read, which
/// leaves the turn the `PATH` it inherits.
fn path_led_by_this_program(inherited: &OsStr) -> Result<Option<EnvAssignment>, StartError> {
    let Some(directory) = std::env::current_exe()
        .ok()
        .and_then(|program| program.parent().map(Path::to_path_buf))
    else {
        return Ok(None);
    };
    let refused = |detail: String| StartError::Supervisor {
        detail: format!(
            "a turn's PATH cannot lead with {}: {detail}",
            directory.display()
        ),
    };
    let joined = std::env::join_paths(
        std::iter::once(directory.clone()).chain(std::env::split_paths(inherited)),
    )
    .map_err(|error| refused(error.to_string()))?;
    let joined = joined
        .to_str()
        .ok_or_else(|| refused("it is not valid Unicode".to_owned()))?;
    EnvAssignment::new(&format!("PATH={joined}"))
        .map(Some)
        .map_err(|error| refused(error.to_string()))
}

/// One server, serving.
///
/// Dropping this stops it, and so does [`Running::stop`]; nothing here outlives
/// the handle a caller holds.
pub struct Running {
    /// Where it is answering.
    address: SocketAddr,
    /// What it was configured with.
    config: ServerConfig,
    /// What this start found that its operator should change, in words that
    /// quote no credential.
    warnings: Vec<String>,
    /// The supervision core.
    supervisor: Arc<Supervisor>,
    /// Durable state, one handle per aggregate.
    stores: Stores,
    /// What this start adopted.
    reconciliation: Reconciliation,
    /// How many posts the ingress worker has finished handling.
    completions: watch::Receiver<u64>,
    /// The graceful-shutdown signal, taken when it is sent.
    stop: Option<oneshot::Sender<()>>,
    /// The task serving requests.
    serving: JoinHandle<()>,
}

impl core::fmt::Debug for Running {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("Running")
            .field("address", &self.address)
            .field("reconciliation", &self.reconciliation)
            .finish_non_exhaustive()
    }
}

impl Running {
    /// Where this server is answering.
    #[must_use]
    pub const fn address(&self) -> SocketAddr {
        self.address
    }

    /// The base URL of its versioned surface.
    #[must_use]
    pub fn base_url(&self) -> String {
        format!(
            "http://{}{}",
            self.address,
            crate::operations::VERSION_PREFIX
        )
    }

    /// The URL its ingress endpoint answers on.
    #[must_use]
    pub fn ingress_url(&self) -> String {
        format!("http://{}{INGRESS_PATH}", self.address)
    }

    /// What it was configured with.
    #[must_use]
    pub const fn config(&self) -> &ServerConfig {
        &self.config
    }

    /// What this start found that its operator should change — a plaintext
    /// `api.credential` among them — in words that quote no credential. The
    /// program running the server writes each to its log on every start.
    #[must_use]
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// What this start adopted from the store.
    #[must_use]
    pub const fn reconciliation(&self) -> &Reconciliation {
        &self.reconciliation
    }

    /// The supervision core it is serving over.
    #[must_use]
    pub const fn supervisor(&self) -> &Arc<Supervisor> {
        &self.supervisor
    }

    /// Durable state, as it is serving over it.
    #[must_use]
    pub const fn stores(&self) -> &Stores {
        &self.stores
    }

    /// How many posts the ingress worker has finished handling, watchable while
    /// it works.
    #[must_use]
    pub fn completions(&self) -> watch::Receiver<u64> {
        self.completions.clone()
    }

    /// Stop serving, and wait for the last request to finish.
    pub async fn stop(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let _ = (&mut self.serving).await;
    }

    /// Serve until the operating system asks this process to stop.
    ///
    /// This is what the `server` subcommand runs. It answers however this
    /// platform asks a process to stop — see [`stop_requested`] — and every one
    /// of those ways enters the same shutdown: [`Running::stop`].
    ///
    /// # Errors
    ///
    /// Returns [`StartError::State`] when the stop handlers cannot be
    /// installed, which is a process that could not be stopped cleanly.
    pub async fn serve_until_signalled(self) -> Result<(), StartError> {
        stop_requested().await?;
        self.stop().await;
        Ok(())
    }
}

/// A stop handler that could not be installed, naming which.
fn unhandled(what: &str, error: &std::io::Error) -> StartError {
    StartError::State {
        detail: format!("{what} cannot be handled: {error}"),
    }
}

/// Wait until the operating system asks this process to stop.
///
/// On Unix that is the signal a service manager stops a unit with, or the
/// interrupt from the terminal that started it.
#[cfg(unix)]
async fn stop_requested() -> Result<(), StartError> {
    use tokio::signal::unix::{SignalKind, signal};

    let mut terminate = signal(SignalKind::terminate())
        .map_err(|error| unhandled("the termination signal", &error))?;
    let mut interrupt = signal(SignalKind::interrupt())
        .map_err(|error| unhandled("the interrupt signal", &error))?;
    tokio::select! {
        _ = terminate.recv() => {}
        _ = interrupt.recv() => {}
    }
    Ok(())
}

/// Wait until the operating system asks this process to stop.
///
/// Windows has no signals: a console process is stopped by a console control
/// event. `CTRL_C` and `CTRL_BREAK` are the terminal's interrupt — the second is
/// the one another process can send a process group started apart from its own
/// — and `CTRL_CLOSE` and `CTRL_SHUTDOWN` are the console closing and the
/// machine shutting down.
#[cfg(windows)]
async fn stop_requested() -> Result<(), StartError> {
    use tokio::signal::windows::{ctrl_break, ctrl_c, ctrl_close, ctrl_shutdown};

    let mut interrupt = ctrl_c().map_err(|error| unhandled("the CTRL_C event", &error))?;
    let mut breaking = ctrl_break().map_err(|error| unhandled("the CTRL_BREAK event", &error))?;
    let mut closing = ctrl_close().map_err(|error| unhandled("the CTRL_CLOSE event", &error))?;
    let mut shutting_down =
        ctrl_shutdown().map_err(|error| unhandled("the CTRL_SHUTDOWN event", &error))?;
    tokio::select! {
        _ = interrupt.recv() => {}
        _ = breaking.recv() => {}
        _ = closing.recv() => {}
        _ = shutting_down.recv() => {}
    }
    Ok(())
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        self.serving.abort();
    }
}
