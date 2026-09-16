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
    AssessmentSchema, HarnessSignIn, OneharnessSupervisor, SupervisorConfig,
};
use printobserver_printer_api::{PrinterError, PrinterPort};
use printobserver_store_sqlite::SqliteStore;
use printobserver_supervisor_api::SupervisorPort;
use tokio::net::TcpListener;
use tokio::sync::{oneshot, watch};
use tokio::task::JoinHandle;

use crate::api::{ApiState, router};
use crate::config::{ApiCredential, ConfigError, ConfigField, ServerConfig};
use crate::ingress::{IngressState, receive};
use crate::operations::INGRESS_PATH;
use crate::reconcile::{ReconcileStores, Reconciliation, overdue, reconcile};

/// The program a supervision turn runs to read its print's context.
pub const CONTEXT_PROGRAM: &str = "printobserver";

/// The file this server writes the address it took and the credential in force
/// into, under the state directory, for the clients that run beside it.
pub const CLIENT_CONFIG_FILE: &str = "client.toml";

/// The file, under the state directory, holding the API credential this server
/// generated for itself — read when `api.credential` is not configured, and
/// written only when it is not there.
pub const API_CREDENTIAL_FILE: &str = "api-credential";

/// The mode both files carrying the credential are created with.
///
/// Unix alone has one. A file Windows creates carries the access its directory
/// grants rather than a mode, so there the state directory is what keeps both
/// files to the service's own account.
#[cfg(unix)]
const PRIVATE: u32 = 0o600;

/// The command a supervision turn runs to read its print's context.
///
/// This program's own context read, against the print the turn is about and
/// the configuration file naming the address this server took — which is the
/// bound address rather than the configured one, because a configuration may
/// ask for any free port and a turn has to reach the one that was taken — and
/// the credential in force.
/// `printobserver-oneharness` substitutes the print for the placeholder.
///
/// Both travel in a file rather than on the command line because no client
/// command of that program takes either: where a server is and what
/// authenticates to it are configuration, and a command line that could carry
/// them is a command line that could be pointed anywhere — and one that lands a
/// credential in a process table.
#[must_use]
pub fn context_command(client_config: &Path) -> String {
    format!(
        "{CONTEXT_PROGRAM} context --config {} --print-id {{print_id}}",
        client_config.display()
    )
}

/// Write the configuration the clients beside this server read it by.
///
/// One `[client]` table naming the address that was actually bound and the
/// credential in force, so a supervision turn — and an operator on this host
/// who can read the state directory — reaches and authenticates to this server
/// without being told either or copying a secret by hand. It is the one file
/// this program writes the credential into beside the one it generated, so it
/// is private to the service's own user from the moment it exists: created, or
/// narrowed when an earlier start left it, before a byte is written.
fn write_client_config(
    directory: &Path,
    address: SocketAddr,
    credential: &ApiCredential,
) -> Result<PathBuf, StartError> {
    use std::io::Write as _;

    let path = directory.join(CLIENT_CONFIG_FILE);
    let failing = |error: std::io::Error| StartError::State {
        detail: format!("{} could not be written: {error}", path.display()),
    };
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, PRIVATE);
    let mut file = options.open(&path).map_err(failing)?;
    #[cfg(unix)]
    file.set_permissions(std::os::unix::fs::PermissionsExt::from_mode(PRIVATE))
        .map_err(failing)?;
    file.write_all(
        format!(
            "[client]\nserver = \"http://{address}\"\ncredential = \"{}\"\n",
            toml_basic_string(credential.written())
        )
        .as_bytes(),
    )
    .map_err(failing)?;
    Ok(path)
}

/// Text as the body of a TOML basic string.
///
/// A credential is printable ASCII by construction, so the quote and the
/// backslash are the only two characters that need an escape.
fn toml_basic_string(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

/// The API credential this start serves under.
///
/// The configured one when `api.credential` names one, in which case the state
/// directory's file is neither read nor written. Otherwise the file: created
/// with a fresh credential when it is not there — exclusively, so that nothing
/// already at that path is ever replaced — and read as it is when it is.
///
/// # Errors
///
/// Returns [`StartError::Credential`] naming the file, and never what it holds,
/// when it cannot be created or read or holds nothing a header could present.
fn credential_in_force(config: &ServerConfig) -> Result<ApiCredential, StartError> {
    use std::io::Write as _;

    if let Some(configured) = &config.api_credential {
        return Ok(configured.clone());
    }
    let path = config.state_dir.join(API_CREDENTIAL_FILE);
    let refusing = |detail: String| StartError::Credential {
        path: path.clone(),
        detail,
    };
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, PRIVATE);
    match options.open(&path) {
        Ok(mut file) => {
            let generated = ApiCredential::generate().map_err(|error| {
                refusing(format!(
                    "no credential could be generated for it, because the operating system's \
                     random source refused: {error}"
                ))
            });
            let written = generated.and_then(|credential| {
                file.write_all(credential.written().as_bytes())
                    .and_then(|()| file.sync_all())
                    .map(|()| credential)
                    .map_err(|error| refusing(format!("it could not be written: {error}")))
            });
            if written.is_err() {
                // The file is this start's own and holds nothing usable, so it
                // is taken back rather than left for the next start to refuse.
                let _ = std::fs::remove_file(&path);
            }
            written
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            // llmlint: ignore[least_privilege_grants] The contract reuses an existing credential file's contents unchanged and never rewrites a file this start did not write, so the server does not change a hand-provisioned file's mode. The installed path keeps the file private by its directory: the installer creates the state directory at mode 0700, owned by the service user, and a file this server generates is created 0600. A mode check on hand-provisioned files is recorded as a follow-up.
            let held = std::fs::read(&path)
                .map_err(|error| refusing(format!("it cannot be read: {error}")))?;
            let text = core::str::from_utf8(&held).ok().map(without_terminator);
            text.ok_or("it is not text")
                .and_then(ApiCredential::new)
                .map_err(|why| {
                    refusing(format!(
                        "{why}. This server does not replace a credential it did not write \
                         just now: correct the file, or remove it to have a new one generated"
                    ))
                })
        }
        Err(error) => Err(refusing(format!("it cannot be created: {error}"))),
    }
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

/// The file name the agent's skill is materialized under.
pub const SKILL_FILE: &str = "printobserver-skill.md";

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
    /// The API credential file in the state directory cannot be used.
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
                "the API credential file {} cannot be used: {detail}",
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
            ObicoVision::new(ObicoVisionConfig::default()).map_err(|error| StartError::State {
                detail: error.to_string(),
            })?,
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
        // The credential is settled before anything listens, so there is no
        // moment at which this server answers a request it has nothing to
        // check against.
        let credential = credential_in_force(&config)?;
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
        let client_config = write_client_config(&config.state_dir, address, &credential)?;
        let clock = Arc::new(SystemClock);
        let due = overdue(&ports.stores.actions, clock.now())
            .await
            .map_err(|error| StartError::Reconciliation {
                detail: error.to_string(),
            })?;
        let supervisor = Supervisor::new(
            CoreConfig::new(config.safety.clone(), context_command(&client_config)),
            Arc::clone(&ports.printer),
            ports.stores.clone(),
            Arc::clone(&ports.vision) as Arc<dyn printobserver_vision_api::VisionPort>,
            Arc::clone(&ports.agent),
            clock,
        );
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
            credential: Arc::new(credential),
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
/// The port reads its skill, its template and its schema from paths, so that
/// editing one on a running host is a restart rather than a rebuild. An
/// installed program has no checkout to read them out of, so the bytes it
/// carries are written here — and an operator who configured a path of their
/// own is pointed at theirs instead.
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

/// Write the reference documents the skill links to into the assets directory.
///
/// The skill is deliberately short and links out for everything else, so an
/// install that carried the skill and none of what it points at would hand the
/// agent a set of dead links — which is worse than no links, because it reads as
/// documentation right up to the moment it is opened. Each document goes to the
/// path the skill links to it by, relative to the assets directory, and the
/// harness runs with that directory as its working directory: so the one
/// relative link in the skill resolves both from the skill's own location and
/// from where the agent is standing, and it resolves to the same file.
fn materialize_references(assets: &Path) -> Result<(), StartError> {
    for (relative, contents) in printobserver_oneharness::DEFAULT_REFERENCES {
        let path = assets.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| StartError::State {
                detail: format!("{} could not be created: {error}", parent.display()),
            })?;
        }
        std::fs::write(&path, contents).map_err(|error| StartError::State {
            detail: format!("{} could not be written: {error}", path.display()),
        })?;
    }
    Ok(())
}

/// The supervising agent's harness, over the assets this program carries.
fn agent_for(config: &ServerConfig) -> Result<OneharnessSupervisor, StartError> {
    let assets = config.assets_dir();
    std::fs::create_dir_all(&assets).map_err(|error| StartError::State {
        detail: format!("{} could not be created: {error}", assets.display()),
    })?;
    materialize_references(&assets)?;
    let skill_path = match &config.skill_path {
        Some(path) => path.clone(),
        None => materialize(&assets, SKILL_FILE, printobserver_oneharness::DEFAULT_SKILL)?,
    };
    let prompt_template_path = match &config.prompt_template_path {
        Some(path) => path.clone(),
        None => materialize(&assets, PROMPT_FILE, TURN_PROMPT)?,
    };
    let schema_path = materialize(
        &assets,
        SCHEMA_FILE,
        &printobserver_types::serde_json::to_string_pretty(
            &printobserver_types::contract::schema_of::<
                printobserver_supervisor_api::AgentAssessment,
            >(),
        )
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
    let harness_env = match HarnessSignIn::of(&config.harness) {
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
    OneharnessSupervisor::open(SupervisorConfig {
        state_dir: config.state_dir.clone(),
        skill_path,
        prompt_template_path,
        assessment_schema,
        harness: config.harness.clone(),
        model: config.model.clone(),
        // The assets directory rather than the state directory, so that the
        // skill's own relative links to the reference documents beside it
        // resolve from where the agent is standing as well as from the skill.
        // llmlint: ignore[changed_behavior_has_e2e] A wrong cwd makes relative reference-file opens fail visibly on the first documentation read; the narrowed rule requires a silent failure. The installed-assets journey already proves the bundled links resolve without a checkout.
        working_dir: assets.clone(),
        turn_timeout: printobserver_oneharness::TurnTimeout::DEFAULT,
        harness_bin: None,
        harness_env,
    })
    .map_err(|error| StartError::Supervisor {
        detail: error.to_string(),
    })
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
