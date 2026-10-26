//! A supervisor of a journey's own, which the journey may stop and start.
//!
//! The world's supervisor runs for the whole of a journey; a journey about a
//! stream that loses its server needs one it can take away and bring back at
//! the same address. This is that: the built program run as `server`, under
//! the world's own configuration — its machine, its camera, its credential —
//! over a state directory of its own and a port fixed when it is made, so a
//! restart answers where the first run did.
//!
//! Events are put in its record beside it through a second connection to its
//! store, which is the one way to record an event with a `received_at` of the
//! journey's choosing. Such a write raises none of the supervisor's own
//! signals, so [`Standalone::nudge`] asks it for a look at a print of its own,
//! which it records itself — the write that wakes a waiting read.

use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, PoisonError};

use printobserver_core::store::{EventDraft, EventStore as _, PrintStore as _};
use printobserver_store_sqlite::SqliteStore;
use printobserver_types::EventRecord;

use crate::announced;
use crate::world::{CREDENTIAL, OTHER_CREDENTIAL, World};

/// One supervisor of a journey's own.
pub struct Standalone {
    /// Its state directory.
    state: PathBuf,
    /// The configuration file it runs under.
    configuration: PathBuf,
    /// Where it answers, every time it is started.
    pub address: SocketAddr,
    /// The process, while it runs.
    server: Mutex<Option<Child>>,
    /// The client configuration under the credential it holds the verifier of.
    pub client: PathBuf,
    /// The client configuration under a credential it refuses.
    pub refused_client: PathBuf,
    /// The print every journey watches.
    pub print_id: String,
    /// A print beside it, which a nudge looks at.
    pub elsewhere: String,
}

impl Drop for Standalone {
    fn drop(&mut self) {
        if let Some(mut server) = self
            .server
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            let _ = server.kill();
            let _ = server.wait();
        }
    }
}

/// A loopback port nothing is bound to now.
fn free_address() -> SocketAddr {
    TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .expect("a loopback port")
}

/// One client configuration naming one server under one credential.
fn client_at(path: &Path, address: SocketAddr, credential: &str) {
    std::fs::write(
        path,
        format!("[client]\nserver = \"http://{address}\"\ncredential = \"{credential}\"\n"),
    )
    .expect("the client configuration is writable");
}

impl Standalone {
    /// One beside a world, started, with two prints in its record.
    ///
    /// # Panics
    ///
    /// Panics when it does not come up, which is a journey with nothing to
    /// watch.
    pub fn beside(world: &World, name: &str) -> Self {
        let root = world.root.path().join(name);
        let state = root.join("state");
        std::fs::create_dir_all(&state).expect("a state directory");
        let address = free_address();
        let text = std::fs::read_to_string(world.root.path().join("server.toml"))
            .expect("the world's configuration reads");
        let mut document: toml::Table = toml::from_str(&text).expect("the configuration parses");
        document.insert(
            "state_dir".to_owned(),
            toml::Value::String(state.display().to_string()),
        );
        document.insert(
            "listen".to_owned(),
            toml::Value::String(address.to_string()),
        );
        let configuration = root.join("server.toml");
        std::fs::write(
            &configuration,
            toml::to_string(&document).expect("the configuration renders"),
        )
        .expect("the configuration is writable");
        let (print_id, elsewhere) = {
            let store = SqliteStore::open(&state).expect("the store opens");
            printobserver_core::block_on(async {
                let watched = store
                    .open_print(Some(4501), Some(crate::machine::RUNNING_FILE.to_owned()))
                    .await
                    .expect("a print opens");
                let beside = store
                    .open_print(Some(4502), Some("beside.gcode".to_owned()))
                    .await
                    .expect("a print opens");
                (watched.id.to_string(), beside.id.to_string())
            })
        };
        let client = root.join("client.toml");
        client_at(&client, address, CREDENTIAL);
        let refused_client = root.join("refused-client.toml");
        client_at(&refused_client, address, OTHER_CREDENTIAL);
        let standalone = Self {
            state,
            configuration,
            address,
            server: Mutex::new(None),
            client,
            refused_client,
            print_id,
            elsewhere,
        };
        standalone.start();
        standalone
    }

    /// Start it, answering where it always answers.
    ///
    /// # Panics
    ///
    /// Panics when it does not come up there.
    pub fn start(&self) {
        let mut started = Command::new(env!("CARGO_BIN_EXE_printobserver"))
            .arg("server")
            .arg("--config")
            .arg(&self.configuration)
            // Nothing on its path: no journey here is about a supervision turn.
            .env("PATH", self.state.join("no-harness-on-this-path"))
            .stderr(Stdio::piped())
            .spawn()
            .expect("the command that runs the supervisor runs");
        let (address, _said) = announced::serving(&mut started, "a journey's own supervisor");
        assert_eq!(
            address, self.address,
            "the supervisor came up somewhere else"
        );
        *self.server.lock().unwrap_or_else(PoisonError::into_inner) = Some(started);
    }

    /// Stop it, as a host that lost it does: its process is gone, and nothing
    /// answers where it answered.
    pub fn stop(&self) {
        if let Some(mut server) = self
            .server
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            let _ = server.kill();
            let _ = server.wait();
        }
    }

    /// Put events in its record, each as given, and answer them as stored.
    pub fn record(&self, drafts: Vec<EventDraft>) -> Vec<EventRecord> {
        let store = SqliteStore::open(&self.state).expect("the store opens");
        printobserver_core::block_on(async {
            let mut stored = Vec::new();
            for draft in drafts {
                stored.push(store.append_event(draft).await.expect("an event is stored"));
            }
            stored
        })
    }

    /// Its store, read beside it.
    pub fn store(&self) -> SqliteStore {
        SqliteStore::open(&self.state).expect("the store opens")
    }

    /// Ask it for a look at the print beside the watched one, which it records
    /// itself — the write that wakes a waiting read.
    pub fn nudge(&self, world: &World) {
        let ran = crate::running::with(
            world,
            &[
                "look".to_owned(),
                "--print-id".to_owned(),
                self.elsewhere.clone(),
                "--wait-s".to_owned(),
                "0".to_owned(),
                "--config".to_owned(),
                self.client.display().to_string(),
            ],
            &[],
        );
        assert_eq!(ran.code, Some(0), "the look did not answer: {}", ran.said());
    }
}
