//! A real supervisor, started by the one command that starts one.
//!
//! Nothing here stands in for the program under test or for the server it
//! talks to. The supervisor is `printobserver server` as a subprocess, over the
//! store the installer creates and the printer port the composition root
//! chooses.
//!
//! # Two worlds, one walk
//!
//! [`World::open`] puts a socket on the far side of the printer port; that is
//! the one thing the fast tier does not have for real, and it is what lets it
//! run in every gate. [`World::against_the_scripted_instance`] puts the
//! `OctoPrint` `just octoprint-up` provisioned there instead, and the same walk
//! runs against both. Where the fast world is **told** what to report,
//! the scripted one is **driven** into it — through this same surface, by the
//! actions that get it there.
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

use crate::machine::{Machine, Reports};
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

/// How many times this world asks a real machine to be somewhere before it
/// gives up and says where it actually is.
const SETTLING_ROUNDS: usize = 4;

/// How many times it looks after each asking.
const SETTLING_POLLS: usize = 40;

/// How long it waits between looking.
const SETTLING_PAUSE: core::time::Duration = core::time::Duration::from_millis(500);

/// The reason an action taken only to settle the machine gives.
///
/// Distinct from every reason the walk drives, so an assertion about what a
/// command asked for cannot be satisfied by one of these.
const SETTLING_REASON: &str = "this walk is putting the machine where the next command needs it";

/// The manifest a settling start attaches, which narrows nothing.
const SETTLING_MANIFEST: &str = concat!(
    r#"{"file_name":"settling","material":"PLA","nozzle_diameter_mm":0.4,"#,
    r#""slicer_profile":"the profile settling drove","allowed":{},"metadata":{}}"#
);

/// The world whose printer port reaches a socket.
pub const STOOD_IN: &str = "stood-in";

/// The world whose printer port reaches the `OctoPrint` `just octoprint-up`
/// provisioned.
pub const SCRIPTED: &str = "scripted";

/// What is on the far side of the printer port.
pub enum Printer {
    /// A socket answering the documents a machine answers.
    StoodIn(Machine),
    /// The `OctoPrint` instance `just octoprint-up` provisioned.
    Scripted {
        /// Where it answers.
        url: String,
        /// The key it was provisioned with.
        api_key: String,
        /// The file it has to print, which its bring-up uploaded.
        file: String,
    },
}

impl Printer {
    /// Where the server is configured to find it.
    fn url(&self) -> String {
        match self {
            Self::StoodIn(machine) => machine.url(),
            Self::Scripted { url, .. } => url.clone(),
        }
    }

    /// The key the server authenticates to it with.
    fn api_key(&self) -> String {
        match self {
            Self::StoodIn(_) => "a-provisioned-key".to_owned(),
            Self::Scripted { api_key, .. } => api_key.clone(),
        }
    }
}

/// What the walk drives, and everything it was started from.
pub struct World {
    /// This world's own root, removed when it is dropped.
    pub root: TempDir,
    /// The machine on the far side of the printer port.
    pub printer: Printer,
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
    /// # Panics
    ///
    /// Panics when the scripted environment was asked for and is not up,
    /// naming the recipe that brings one up: a tier that quietly passed against
    /// no printer would prove nothing, so there is no fallback and no skip.
    pub fn open(world: &str) -> Self {
        Self::over(match world {
            STOOD_IN => Printer::StoodIn(Machine::start()),
            SCRIPTED => crate::scripted::scripted(),
            other => panic!("there is no `{other}` world to open"),
        })
    }

    /// A world over whatever is on the far side of the printer port.
    fn over(printer: Printer) -> Self {
        let root = TempDir::new().expect("this tier's own root");
        let state = root.path().join("state");
        std::fs::create_dir_all(&state).expect("a state directory");
        let (print_id, image_id, event_id) = seed(&state, &printable_file(&printer));

        let configuration = root.path().join("server.toml");
        std::fs::write(&configuration, server_document(&state, &printer))
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
            printer,
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

    /// The file this world's printer can be asked to print.
    pub fn printable_file(&self) -> String {
        printable_file(&self.printer)
    }

    /// An endpoint that is not the configured supervisor, for the variant of
    /// this program that connects to one.
    pub fn elsewhere(&self) -> String {
        self.printer
            .url()
            .trim_start_matches("http://")
            .trim_end_matches('/')
            .to_owned()
    }

    /// Put the machine into the state one command is valid from.
    ///
    /// A socket is told. A real printer is **driven** there, through this same
    /// surface, by the actions that get it there — which is what a print
    /// actually goes through — and then waited for, because a machine takes its
    /// own time.
    ///
    /// # Panics
    ///
    /// Panics when the machine did not reach that state, which is a walk whose
    /// next command could only be refused for being asked from the wrong one.
    pub fn wants(&self, state: Reports) {
        match &self.printer {
            Printer::StoodIn(machine) => machine.reports(state),
            Printer::Scripted { .. } => self.drive_to(state),
        }
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
        std::fs::write(&path, server_document(&state, &self.printer))
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

    /// Drive a real machine into one state, and wait until it reports it.
    ///
    /// Everything here goes through this program's own commands: reading what
    /// the machine is doing is a status read, and moving it is one of the
    /// vocabulary's own actions. So a walk that could not settle the machine is
    /// one whose own surface could not, rather than one whose test harness
    /// could not.
    fn drive_to(&self, state: Reports) {
        let mut said = String::new();
        for _ in 0..SETTLING_ROUNDS {
            if self.reported_state() == Some(state) {
                return;
            }
            said = self.one_step_towards(state);
            for _ in 0..SETTLING_POLLS {
                std::thread::sleep(SETTLING_PAUSE);
                if self.reported_state() == Some(state) {
                    return;
                }
            }
        }
        panic!(
            "the machine never reported {state:?}: it is reporting {:?}, and the last thing \
             asked of it said {said}",
            self.reported_state()
        );
    }

    /// Ask for the one action that takes the machine towards a state.
    ///
    /// Asked for **once** per round rather than on every poll: a machine takes
    /// its own time to pause, and a walk that asked again every half second
    /// would be asking a machine part-way through pausing to pause.
    fn one_step_towards(&self, state: Reports) -> String {
        let starting = &[
            "--file-name",
            &self.printable_file(),
            "--manifest",
            SETTLING_MANIFEST,
        ];
        match (state, self.reported_state()) {
            (Reports::Printing, Some(Reports::Paused)) => self.settle("resume", &[]),
            (Reports::Paused, Some(Reports::Printing)) => self.settle("pause", &[]),
            (Reports::Operational, _) => self.settle("cancel", &[]),
            // Anywhere else, what gets the machine printing is starting a
            // print — and a pause is only valid from printing, so that is the
            // step before pausing too.
            (Reports::Printing | Reports::Paused, _) => self.settle("start-print", starting),
        }
    }

    /// What the machine reports it is doing, read through this same surface.
    fn reported_state(&self) -> Option<Reports> {
        let read = Command::new(env!("CARGO_BIN_EXE_printobserver"))
            .args(["status", "--print-id", &self.print_id, "--json", "--config"])
            .arg(self.client_config())
            .output()
            .expect("a status read runs");
        let answer: Value =
            printobserver_types::serde_json::from_slice(&read.stdout).unwrap_or(Value::Null);
        match answer
            .pointer("/printer/connection")
            .and_then(Value::as_str)
        {
            Some("printing") => Some(Reports::Printing),
            Some("paused") => Some(Reports::Paused),
            Some("operational") => Some(Reports::Operational),
            _ => None,
        }
    }

    /// Ask for one action, for no reason but settling the machine.
    ///
    /// What it said is answered rather than dropped, so a walk that could not
    /// settle the machine says why the last attempt did not take.
    fn settle(&self, command: &str, also: &[&str]) -> String {
        let mut asked = vec![
            command,
            "--print-id",
            &self.print_id,
            "--actor",
            "operator",
            "--reason",
            SETTLING_REASON,
        ];
        asked.extend_from_slice(also);
        let ran = Command::new(env!("CARGO_BIN_EXE_printobserver"))
            .args(&asked)
            .arg("--config")
            .arg(self.client_config())
            .output()
            .expect("an action runs");
        format!(
            "`{command}` exited {:?}: {}",
            ran.status.code(),
            String::from_utf8_lossy(&ran.stderr)
        )
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

/// The file one printer can be asked to print.
fn printable_file(printer: &Printer) -> String {
    match printer {
        Printer::StoodIn(_) => crate::machine::RUNNING_FILE.to_owned(),
        Printer::Scripted { file, .. } => file.clone(),
    }
}

/// Open a print, append an event and store an image against it.
fn seed(state: &Path, file: &str) -> (String, String, String) {
    let store = printobserver_store_sqlite::SqliteStore::open(state).expect("the store opens");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("a runtime");
    runtime.block_on(async {
        let print = store
            .open_print(Some(4211), Some(file.to_owned()))
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
fn server_document(state: &Path, printer: &Printer) -> String {
    let document = json!({
        "state_dir": state.display().to_string(),
        "listen": "127.0.0.1:0",
        "octoprint": {
            "url": printer.url(),
            "api_key": printer.api_key(),
            "fan": "commandable",
        },
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
