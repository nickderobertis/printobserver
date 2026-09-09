//! The composition root: the one place an implementation crate is named.
//!
//! Everything below this file is written against the four ports. This is where
//! one implementation is chosen for each — `OctoPrint` behind the printer port,
//! `Obico` behind the vision port, `OneHarness` behind the supervisor port, and
//! `SQLite` behind the store port — and `just check-repo` refuses any other
//! crate that names one.
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
use printobserver_core::{Clock as _, CoreConfig, Supervisor, SystemClock};
use printobserver_obico::{ObicoVision, ObicoVisionConfig};
use printobserver_octoprint::OctoPrintPrinter;
use printobserver_oneharness::{AssessmentSchema, OneharnessSupervisor, SupervisorConfig};
use printobserver_printer_api::{PrinterError, PrinterPort};
use printobserver_store_api::StorePort;
use printobserver_store_sqlite::SqliteStore;
use printobserver_supervisor_api::SupervisorPort;
use tokio::net::TcpListener;
use tokio::sync::{oneshot, watch};
use tokio::task::JoinHandle;

use crate::api::{ApiState, router};
use crate::config::{ConfigError, ConfigField, ServerConfig};
use crate::ingress::{IngressState, receive};
use crate::operations::INGRESS_PATH;
use crate::reconcile::{Reconciliation, overdue, reconcile};

/// The program a supervision turn runs to read its print's context.
pub const CONTEXT_PROGRAM: &str = "printobserver";

/// The file this server writes the address it took into, under the state
/// directory, for the clients that run beside it.
pub const CLIENT_CONFIG_FILE: &str = "client.toml";

/// The command a supervision turn runs to read its print's context.
///
/// This program's own context read, against the print the turn is about and
/// the configuration file naming the address this server took — which is the
/// bound address rather than the configured one, because a configuration may
/// ask for any free port and a turn has to reach the one that was taken.
/// `printobserver-oneharness` substitutes the print for the placeholder.
///
/// The address travels in a file rather than on the command line because no
/// client command of that program takes one: where a server is and what
/// authenticates to it are configuration, and a command line that could carry
/// them is a command line that could be pointed anywhere.
#[must_use]
pub fn context_command(client_config: &Path) -> String {
    format!(
        "{CONTEXT_PROGRAM} context --config {} --print-id {{print_id}}",
        client_config.display()
    )
}

/// Write the configuration the clients beside this server read it by.
///
/// One file naming the address that was actually bound, so a supervision turn
/// — and an operator on this host — reaches this server without being told
/// where it is. It carries no credential: this server requires none of its API
/// callers, and a file this program wrote carrying one would be a secret
/// nobody chose to store.
fn write_client_config(directory: &Path, address: SocketAddr) -> Result<PathBuf, StartError> {
    let path = directory.join(CLIENT_CONFIG_FILE);
    std::fs::write(&path, format!("[client]\nserver = \"http://{address}\"\n")).map_err(
        |error| StartError::State {
            detail: format!("{} could not be written: {error}", path.display()),
        },
    )?;
    Ok(path)
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
    /// Durable state.
    pub store: Arc<dyn StorePort>,
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
        let store =
            Arc::new(
                SqliteStore::open(&config.state_dir).map_err(|error| StartError::Store {
                    detail: error.to_string(),
                })?,
            );
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
                store,
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
        // The listener is taken first, because the command a supervision turn
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
        let client_config = write_client_config(&config.state_dir, address)?;
        let clock = Arc::new(SystemClock);
        let due = overdue(&ports.store, clock.now()).await.map_err(|error| {
            StartError::Reconciliation {
                detail: error.to_string(),
            }
        })?;
        let supervisor = Supervisor::new(
            CoreConfig::new(config.safety.clone(), context_command(&client_config)),
            Arc::clone(&ports.printer),
            Arc::clone(&ports.store),
            Arc::clone(&ports.vision) as Arc<dyn printobserver_vision_api::VisionPort>,
            Arc::clone(&ports.agent),
            clock,
        );
        let reconciliation = reconcile(&supervisor, &ports.store, due)
            .await
            .map_err(|error| StartError::Reconciliation {
                detail: error.to_string(),
            })?;

        let ingress = IngressState::start(
            Arc::clone(&supervisor),
            Arc::clone(&ports.store),
            Arc::clone(&ports.vision),
            config.ingress_shared_secret.clone(),
            config.ingress_answer_bound,
        );
        let completions = ingress.completions();
        let application = router(ApiState {
            supervisor: Arc::clone(&supervisor),
            store: Arc::clone(&ports.store),
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
            store: ports.store,
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

/// The supervising agent's harness, over the assets this program carries.
fn agent_for(config: &ServerConfig) -> Result<OneharnessSupervisor, StartError> {
    let assets = config.assets_dir();
    std::fs::create_dir_all(&assets).map_err(|error| StartError::State {
        detail: format!("{} could not be created: {error}", assets.display()),
    })?;
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
            &printobserver_types::contract::schema_of::<printobserver_types::AgentAssessment>(),
        )
        .map_err(|error| StartError::State {
            detail: error.to_string(),
        })?,
    )?;
    let assessment_schema =
        AssessmentSchema::at(&schema_path).map_err(|error| StartError::Supervisor {
            detail: error.to_string(),
        })?;
    OneharnessSupervisor::open(SupervisorConfig {
        state_dir: config.state_dir.clone(),
        skill_path,
        prompt_template_path,
        assessment_schema,
        harness: config.harness.clone(),
        model: config.model.clone(),
        working_dir: config.state_dir.clone(),
        turn_timeout: printobserver_oneharness::TurnTimeout::DEFAULT,
        harness_bin: None,
        harness_env: Vec::new(),
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
    /// Durable state.
    store: Arc<dyn StorePort>,
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
    pub const fn store(&self) -> &Arc<dyn StorePort> {
        &self.store
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
    /// This is what the `server` subcommand runs. It answers when the service
    /// manager sends the signal it stops units with, or when the terminal that
    /// started it interrupts.
    ///
    /// # Errors
    ///
    /// Returns [`StartError::State`] when the signal handlers cannot be
    /// installed, which is a process that could not be stopped cleanly.
    pub async fn serve_until_signalled(self) -> Result<(), StartError> {
        use tokio::signal::unix::{SignalKind, signal};

        let mut terminate = signal(SignalKind::terminate()).map_err(|error| StartError::State {
            detail: format!("the termination signal cannot be handled: {error}"),
        })?;
        let mut interrupt = signal(SignalKind::interrupt()).map_err(|error| StartError::State {
            detail: format!("the interrupt signal cannot be handled: {error}"),
        })?;
        tokio::select! {
            _ = terminate.recv() => {}
            _ = interrupt.recv() => {}
        }
        self.stop().await;
        Ok(())
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        self.serving.abort();
    }
}
