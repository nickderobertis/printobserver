//! A real supervisor, started by the one command that starts one.
//!
//! Nothing here stands in for the program under test or for the server it
//! talks to. The supervisor is `printobserver server` as a subprocess, over the
//! store the installer creates and the printer port the composition root
//! chooses; what is not real is the machine on the far side of that port, which
//! is [`Machine`] — a socket rather than a printer.
//!
//! # Everything a command is configured with is written here
//!
//! A command in the walk is pointed at the recording proxy rather than at the
//! server directly, so that what the server received is read at the wire. Both
//! ways of configuring it are set up: a file this world writes, and the
//! variables the environment carries.

use std::io::{BufRead as _, BufReader};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use printobserver_store_api::StorePort as _;
use printobserver_types::serde_json::{Value, json};
use tempfile::TempDir;

use crate::machine::Machine;
use crate::proxy::Proxy;

/// The credential every run in the walk is configured with.
///
/// Thirty-two characters of nothing else, so that a search for any four
/// consecutive characters of it says something: a run of four this program
/// would have printed whatever it was configured with is what the control
/// credential below rules out.
pub const CREDENTIAL: &str = "qz7vk3xhw9mrbt2ycf5jdlgnps46auei";

/// A second credential, for the control run.
///
/// The same walk under this one is searched for the **first** credential's own
/// fragments: a fragment that appears there is one this program prints whatever
/// it is configured with, and is not a leak.
pub const OTHER_CREDENTIAL: &str = "hbxq82wntkr5vzc7jm4pldsgfj39aeoy";

/// The shared secret this world's ingress requires.
const SECRET: &str = "a-shared-secret-this-tier-configures";

/// The image this world stores, as bytes a digest can be taken of.
const IMAGE_BYTES: &[u8] = b"not a photograph, but the bytes of one";

/// What the walk drives, and everything it was started from.
pub struct World {
    /// This world's own root, removed when it is dropped.
    pub root: TempDir,
    /// The machine on the far side of the printer port.
    pub machine: Machine,
    /// What a command is configured to reach the supervisor through.
    pub proxy: Proxy,
    /// The print every command in the walk is about.
    pub print_id: String,
    /// The image the materialization commands are about.
    pub image_id: String,
    /// The event the acknowledgement command is about.
    pub event_id: String,
    /// The supervisor, running.
    server: Child,
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = self.server.kill();
        let _ = self.server.wait();
    }
}

impl World {
    /// Start one: a machine, a store with a print and an image in it, and the
    /// supervisor over both.
    ///
    /// # Panics
    ///
    /// Panics when the supervisor did not start, which is a world nothing in
    /// the walk could be driven against.
    pub fn open() -> Self {
        let root = TempDir::new().expect("this tier's own root");
        let machine = Machine::start();
        let state = root.path().join("state");
        std::fs::create_dir_all(&state).expect("a state directory");
        let (print_id, image_id, event_id) = seed(&state);

        let configuration = root.path().join("server.toml");
        std::fs::write(&configuration, server_document(&state, &machine.url()))
            .expect("the configuration is writable");
        let mut server = Command::new(env!("CARGO_BIN_EXE_printobserver"))
            .arg("server")
            .arg("--config")
            .arg(&configuration)
            .stderr(Stdio::piped())
            .spawn()
            .expect("the command that runs the supervisor runs");
        let address = serving_on(&mut server);
        let proxy = Proxy::in_front_of(address);

        let world = Self {
            root,
            machine,
            proxy,
            print_id,
            image_id,
            event_id,
            server,
        };
        std::fs::write(world.client_config(), world.client_document(CREDENTIAL))
            .expect("the client configuration is writable");
        std::fs::write(
            world.other_client_config(),
            world.client_document(OTHER_CREDENTIAL),
        )
        .expect("the client configuration is writable");
        world
    }

    /// The configuration file a command is given when it is given one.
    pub fn client_config(&self) -> PathBuf {
        self.root.path().join("client.toml")
    }

    /// The same, under the control credential.
    pub fn other_client_config(&self) -> PathBuf {
        self.root.path().join("other-client.toml")
    }

    /// A configuration file naming no supervisor at all.
    pub fn serverless_config(&self) -> PathBuf {
        let path = self.root.path().join("serverless.toml");
        std::fs::write(&path, "[client]\n").expect("the configuration is writable");
        path
    }

    /// The configuration a second supervisor is started under.
    ///
    /// Its own state directory, because two supervisors over one store is a
    /// second thing this journey would be about.
    pub fn second_server_config(&self) -> PathBuf {
        let state = self.root.path().join("second-state");
        std::fs::create_dir_all(&state).expect("a state directory");
        let path = self.root.path().join("second-server.toml");
        std::fs::write(&path, server_document(&state, &self.machine.url()))
            .expect("the configuration is writable");
        path
    }

    /// A configuration file this program refuses to read.
    pub fn refused_config(&self) -> PathBuf {
        let path = self.root.path().join("refused.toml");
        std::fs::write(&path, "[client]\nthis is not a document\n")
            .expect("the configuration is writable");
        path
    }

    /// The variables that configure a command when no file is named.
    pub fn environment(&self, credential: &str) -> Vec<(String, String)> {
        vec![
            ("PRINTOBSERVER_SERVER".to_owned(), self.proxy.url()),
            ("PRINTOBSERVER_CREDENTIAL".to_owned(), credential.to_owned()),
        ]
    }

    /// Where the image this world stored actually is.
    pub fn image_path(&self) -> PathBuf {
        let relative = self.image_relative_path();
        self.root.path().join("state").join(relative)
    }

    /// Store the image against a fresh alert, so it is one of the print's most
    /// recent events again.
    ///
    /// A context read carries the latest image the print's own recent events
    /// carry, and this walk writes hundreds of events; an alert from before all
    /// of them has fallen out of that window by the time a later journey looks.
    /// What a real print does between two failures is have another one, which
    /// is what this is. The bytes are the same, and the store addresses an
    /// image by its content, so the path does not move.
    pub fn freshen_the_image(&self) {
        self.restore_the_image();
        with_the_store(self.root.path().join("state"), |store, runtime| {
            runtime.block_on(async {
                let print = self.print_id.parse().expect("a print identifier");
                let event = store
                    .append_event(printobserver_store_api::EventDraft {
                        print_id: Some(print),
                        source: printobserver_types::EventSource::Obico,
                        received_at: printobserver_types::Timestamp::now(),
                        payload: printobserver_types::EventPayload::ObicoFailureAlert(
                            printobserver_types::contract::Sample::sample_full(),
                        ),
                        raw: None,
                    })
                    .await
                    .expect("an event is appended");
                store
                    .put_image(
                        print,
                        event.id,
                        Some("http://a-detector.invalid/snapshot.jpg".to_owned()),
                        "image/jpeg".to_owned(),
                        printobserver_types::RawBytes::new(IMAGE_BYTES.to_vec()),
                    )
                    .await
                    .expect("an image is stored");
            });
        });
    }

    /// Put the image's bytes back where the record says they are.
    pub fn restore_the_image(&self) {
        let path = self.image_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("the image's own directory");
        }
        std::fs::write(path, IMAGE_BYTES).expect("the image is writable");
    }

    /// The digest the image record declares.
    pub fn image_digest() -> String {
        use sha2::{Digest as _, Sha256};
        format!("{:x}", Sha256::digest(IMAGE_BYTES))
    }

    /// Where the image lives beneath the state directory.
    fn image_relative_path(&self) -> String {
        with_the_store(self.root.path().join("state"), |store, runtime| {
            let lookup = runtime
                .block_on(store.image(self.image_id.parse().expect("an image identifier")))
                .expect("the image is there");
            match lookup {
                printobserver_store_api::ImageLookup::Found { record, .. }
                | printobserver_store_api::ImageLookup::FileMissing { record } => {
                    record.relative_path
                }
            }
        })
    }

    /// The client configuration document, under one credential.
    fn client_document(&self, credential: &str) -> String {
        format!(
            "[client]\nserver = \"{}\"\ncredential = \"{credential}\"\n",
            self.proxy.url()
        )
    }
}

/// Do one thing with the store the running supervisor also holds.
///
/// A second connection rather than a second store: the schema is written under
/// a write-ahead log with a lock timeout, which is what lets a journey put
/// something in the record beside a supervisor that is serving from it.
fn with_the_store<T>(
    state: PathBuf,
    doing: impl FnOnce(&printobserver_store_sqlite::SqliteStore, &tokio::runtime::Runtime) -> T,
) -> T {
    let store = printobserver_store_sqlite::SqliteStore::open(state).expect("the store opens");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("a runtime");
    doing(&store, &runtime)
}

/// Open a print, append an event and store an image against it.
fn seed(state: &Path) -> (String, String, String) {
    let store = printobserver_store_sqlite::SqliteStore::open(state).expect("the store opens");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("a runtime");
    runtime.block_on(async {
        let print = store
            .open_print(Some(4211), Some(crate::machine::RUNNING_FILE.to_owned()))
            .await
            .expect("a print opens");
        let event = store
            .append_event(printobserver_store_api::EventDraft {
                print_id: Some(print.id),
                source: printobserver_types::EventSource::Obico,
                received_at: printobserver_types::Timestamp::now(),
                payload: printobserver_types::EventPayload::ObicoFailureAlert(
                    printobserver_types::contract::Sample::sample_full(),
                ),
                raw: None,
            })
            .await
            .expect("an event is appended");
        let image = store
            .put_image(
                print.id,
                event.id,
                Some("http://a-detector.invalid/snapshot.jpg".to_owned()),
                "image/jpeg".to_owned(),
                printobserver_types::RawBytes::new(IMAGE_BYTES.to_vec()),
            )
            .await
            .expect("an image is stored");
        (
            print.id.to_string(),
            image.id.to_string(),
            event.id.to_string(),
        )
    })
}

/// The configuration the supervisor is started under.
fn server_document(state: &Path, machine: &str) -> String {
    let document = json!({
        "state_dir": state.display().to_string(),
        "listen": "127.0.0.1:0",
        "octoprint": { "url": machine, "api_key": "a-provisioned-key", "fan": "commandable" },
        "supervisor": { "harness": "claude-code" },
        "ingress": { "shared_secret": SECRET, "answer_bound_ms": 1000 },
        "safety": {
            "agent_min_interval_s": 0,
            "allowed": {
                "feedrate": { "min": 0.5, "max": 1.5 },
                "flowrate": { "min": 0.9, "max": 1.1 },
                "fan": { "min": 0.0, "max": 100.0 },
                "bed_target": { "min": 0.0, "max": 110.0 },
                "tool_target:0": { "min": 0.0, "max": 260.0 },
            },
            // The operator may ask for everything and the agent for nothing,
            // which is what gives this walk both a success path and a policy
            // rejection for every action of the vocabulary.
            "actions": {
                "operator": [
                    "pause", "resume", "cancel", "start_print", "set_feedrate_factor",
                    "set_flowrate_factor", "set_tool_target_c", "set_bed_target_c",
                    "set_fan_percent", "acknowledge_failure",
                ],
                "agent": [],
                // The supervisor puts a prior value back through the same
                // policy every other action passes, so a class granted nothing
                // is one whose bounded changes could never be restored.
                "system": [
                    "set_feedrate_factor", "set_flowrate_factor", "set_tool_target_c",
                    "set_bed_target_c", "set_fan_percent",
                ],
            },
        },
    });
    toml_of(&document)
}

/// One JSON document, as the TOML the supervisor reads it in.
fn toml_of(document: &Value) -> String {
    toml::to_string(document).expect("a configuration document renders")
}

/// Where the started program says it is serving.
fn serving_on(child: &mut Child) -> SocketAddr {
    let stderr = child.stderr.take().expect("the program's own output");
    let mut lines = BufReader::new(stderr).lines();
    for _ in 0..20 {
        let Some(line) = lines.next() else { break };
        let line = line.expect("the program's output reads");
        if let Some(address) = line.strip_prefix("printobserver is serving on ") {
            return address.trim().parse().expect("an address and a port");
        }
        assert!(
            !line.contains("will not start"),
            "the command that runs the supervisor refused to start: {line}"
        );
    }
    panic!("the supervisor never said where it was serving");
}
